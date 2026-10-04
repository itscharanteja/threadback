pub mod secrets;
pub mod settings;

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[cfg(all(target_os = "macos", feature = "keychain"))]
pub use secrets::KeychainSecretStore;
pub use secrets::{
    ApiKey, KEYCHAIN_SERVICE, MemorySecretStore, SecretStore, TOKEN_FACTORY_ACCOUNT,
    default_secret_store,
};
pub use settings::{
    DEFAULT_ENDPOINT, DEFAULT_MODEL_ID, DEFAULT_TIMEOUT_SECS, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS,
    ModelSettings, StructuredOutput, normalize_endpoint,
};

use crate::{Database, DomainError};

const SETTINGS_KEY: &str = "model_settings";

/// What the interface sends when the user saves the model connection. The key
/// travels only in this direction: nothing returned to the interface contains
/// it, and `Debug` hides it.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigInput {
    pub endpoint: String,
    pub model_id: String,
    pub timeout_secs: u32,
    pub structured_output: StructuredOutput,
    /// A new key to store. `None` keeps the stored key unchanged.
    pub api_key: Option<String>,
}

impl fmt::Debug for ModelConfigInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelConfigInput")
            .field("endpoint", &self.endpoint)
            .field("model_id", &self.model_id)
            .field("timeout_secs", &self.timeout_secs)
            .field("structured_output", &self.structured_output)
            .field("api_key", &self.api_key.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

impl ModelConfigInput {
    fn settings(&self) -> ModelSettings {
        ModelSettings {
            endpoint: self.endpoint.clone(),
            model_id: self.model_id.clone(),
            timeout_secs: self.timeout_secs,
            structured_output: self.structured_output,
        }
    }
}

/// The saved configuration as the interface sees it. There is no key in it,
/// only whether one is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationStatus {
    pub endpoint: String,
    pub model_id: String,
    pub timeout_secs: u32,
    pub structured_output: StructuredOutput,
    pub has_api_key: bool,
    /// Where evidence would be sent.
    pub destination_host: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    /// A sentence the user can act on.
    pub message: String,
    #[ts(type = "number | null")]
    pub latency_ms: Option<u64>,
    pub model_id: String,
}

/// A key must look like a key: no spaces, no control characters, a sensible
/// length. The message never repeats what was typed.
pub fn validate_api_key(raw: &str) -> Result<String, DomainError> {
    let key = raw.trim();
    let ok = (8..=512).contains(&key.len()) && key.chars().all(|c| c.is_ascii_graphic());
    if ok {
        Ok(key.to_owned())
    } else {
        Err(DomainError::invalid_input(
            "The API key does not look valid: it should be 8 to 512 characters with no spaces.",
            "apiKey",
        ))
    }
}

pub struct ConfigurationService<'a> {
    database: &'a Database,
    secrets: &'a dyn SecretStore,
}

impl<'a> ConfigurationService<'a> {
    pub fn new(database: &'a Database, secrets: &'a dyn SecretStore) -> Self {
        Self { database, secrets }
    }

    pub fn settings(&self) -> Result<ModelSettings, DomainError> {
        match self.database.metadata_get(SETTINGS_KEY)? {
            Some(json) => serde_json::from_str::<ModelSettings>(&json)
                .ok()
                .and_then(|settings| settings.validated().ok())
                // Unreadable or no-longer-valid settings fall back to the defaults.
                .map_or_else(|| Ok(ModelSettings::default()), Ok),
            None => Ok(ModelSettings::default()),
        }
    }

    pub fn status(&self) -> Result<ConfigurationStatus, DomainError> {
        let settings = self.settings()?;
        Ok(ConfigurationStatus {
            has_api_key: self.secrets.exists(TOKEN_FACTORY_ACCOUNT)?,
            destination_host: settings.destination_host(),
            endpoint: settings.endpoint,
            model_id: settings.model_id,
            timeout_secs: settings.timeout_secs,
            structured_output: settings.structured_output,
        })
    }

    /// Saves the settings and, if one is given, the key. Everything is
    /// validated first, so a bad value changes nothing.
    pub fn store(&self, input: ModelConfigInput) -> Result<ConfigurationStatus, DomainError> {
        let settings = input.settings().validated()?;
        let key = input.api_key.as_deref().map(validate_api_key).transpose()?;

        if let Some(key) = key {
            self.secrets.set(TOKEN_FACTORY_ACCOUNT, &key)?;
        }
        let json = serde_json::to_string(&settings).map_err(|error| {
            DomainError::internal(format!("Settings serialization failed: {error}"))
        })?;
        self.database.metadata_set(SETTINGS_KEY, &json)?;
        self.status()
    }

    pub fn remove_api_key(&self) -> Result<ConfigurationStatus, DomainError> {
        self.secrets.delete(TOKEN_FACTORY_ACCOUNT)?;
        self.status()
    }

    /// The stored key, for the code that builds a request. It is never
    /// returned to the interface.
    pub(crate) fn api_key(&self) -> Result<Option<ApiKey>, DomainError> {
        self.secrets.get(TOKEN_FACTORY_ACCOUNT)
    }

    /// A client for the saved connection. A missing key is reported here,
    /// before any request is made.
    #[cfg(feature = "model-client")]
    pub fn build_client(
        &self,
        cancel: crate::CancelToken,
    ) -> Result<crate::TokenFactoryClient, DomainError> {
        let settings = self.settings()?;
        let key = self.api_key()?.ok_or_else(|| {
            DomainError::not_configured(
                "Add your model API key in Settings before generating a capsule. Nothing was sent.",
            )
        })?;
        Ok(crate::TokenFactoryClient::new(settings, key, cancel))
    }

    /// The settings and key to try for a connection test: the values in the
    /// form, with the stored key used when none was typed.
    pub fn resolve_for_test(
        &self,
        input: &ModelConfigInput,
    ) -> Result<(ModelSettings, Option<ApiKey>), DomainError> {
        let settings = input.settings().validated()?;
        let key = match input.api_key.as_deref() {
            Some(raw) => Some(ApiKey::new(validate_api_key(raw)?)),
            None => self.api_key()?,
        };
        Ok((settings, key))
    }
}
