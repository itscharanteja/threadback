use std::{collections::HashMap, fmt, sync::Mutex};

use crate::{DomainError, paths::APP_IDENTIFIER};

/// The Keychain service name. It matches the application identifier so the
/// item is recognisably ThreadBack's in Keychain Access.
pub const KEYCHAIN_SERVICE: &str = APP_IDENTIFIER;
pub const TOKEN_FACTORY_ACCOUNT: &str = "token-factory-api-key";

/// An API key. It cannot be printed: `Debug` and `Display` show nothing of
/// it, so it cannot reach a log or an error message through formatting.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The only way to read the key. Call it where the request is built.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<hidden>)")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
}

/// Where secrets live. The only implementation used by the app is the macOS
/// Keychain; the in-memory one exists for tests. Secrets are never kept in
/// SQLite or in a file.
pub trait SecretStore: Send + Sync {
    fn set(&self, account: &str, secret: &str) -> Result<(), DomainError>;
    fn get(&self, account: &str) -> Result<Option<ApiKey>, DomainError>;
    /// Removing a secret that is not there is not an error.
    fn delete(&self, account: &str) -> Result<(), DomainError>;

    fn exists(&self, account: &str) -> Result<bool, DomainError> {
        Ok(self.get(account)?.is_some())
    }
}

#[derive(Default)]
pub struct MemorySecretStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn set(&self, account: &str, secret: &str) -> Result<(), DomainError> {
        self.secrets
            .lock()
            .map_err(|_| DomainError::secret_store("The secret store is unavailable."))?
            .insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<ApiKey>, DomainError> {
        Ok(self
            .secrets
            .lock()
            .map_err(|_| DomainError::secret_store("The secret store is unavailable."))?
            .get(account)
            .map(|secret| ApiKey::new(secret.clone())))
    }

    fn delete(&self, account: &str) -> Result<(), DomainError> {
        self.secrets
            .lock()
            .map_err(|_| DomainError::secret_store("The secret store is unavailable."))?
            .remove(account);
        Ok(())
    }
}

/// The macOS Keychain, through the platform adapter.
#[cfg(all(target_os = "macos", feature = "keychain"))]
pub struct KeychainSecretStore {
    service: String,
}

#[cfg(all(target_os = "macos", feature = "keychain"))]
impl KeychainSecretStore {
    pub fn new() -> Self {
        Self::with_service(KEYCHAIN_SERVICE)
    }

    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
        }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, DomainError> {
        keyring::Entry::new(&self.service, account).map_err(keychain_error)
    }
}

#[cfg(all(target_os = "macos", feature = "keychain"))]
impl Default for KeychainSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

/// The Keychain's error text never contains the secret, but it is reduced to
/// a fixed sentence plus the kind of failure anyway.
#[cfg(all(target_os = "macos", feature = "keychain"))]
fn keychain_error(error: keyring::Error) -> DomainError {
    let kind = match error {
        keyring::Error::NoStorageAccess(_) => "access was denied",
        keyring::Error::PlatformFailure(_) => "the system reported a failure",
        keyring::Error::TooLong(_, _) => "the value is too long",
        keyring::Error::Invalid(_, _) => "the request was invalid",
        _ => "an unexpected failure occurred",
    };
    DomainError::secret_store(format!("The macOS Keychain could not be used: {kind}."))
}

#[cfg(all(target_os = "macos", feature = "keychain"))]
impl SecretStore for KeychainSecretStore {
    fn set(&self, account: &str, secret: &str) -> Result<(), DomainError> {
        self.entry(account)?
            .set_password(secret)
            .map_err(keychain_error)
    }

    fn get(&self, account: &str) -> Result<Option<ApiKey>, DomainError> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(ApiKey::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keychain_error(error)),
        }
    }

    fn delete(&self, account: &str) -> Result<(), DomainError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keychain_error(error)),
        }
    }
}

/// The store the application uses: the Keychain on macOS. Elsewhere there is
/// no adapter yet, so saving a key fails with a clear message rather than
/// falling back to a file.
pub fn default_secret_store() -> Box<dyn SecretStore> {
    #[cfg(all(target_os = "macos", feature = "keychain"))]
    {
        Box::new(KeychainSecretStore::new())
    }
    #[cfg(not(all(target_os = "macos", feature = "keychain")))]
    {
        Box::new(UnsupportedSecretStore)
    }
}

#[cfg(not(all(target_os = "macos", feature = "keychain")))]
struct UnsupportedSecretStore;

#[cfg(not(all(target_os = "macos", feature = "keychain")))]
impl SecretStore for UnsupportedSecretStore {
    fn set(&self, _: &str, _: &str) -> Result<(), DomainError> {
        Err(DomainError::secret_store(
            "Storing an API key is only supported on macOS in this version.",
        ))
    }
    fn get(&self, _: &str) -> Result<Option<ApiKey>, DomainError> {
        Ok(None)
    }
    fn delete(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_key_cannot_be_printed() {
        let key = ApiKey::new("super-secret-key-value-123");
        for text in [
            format!("{key:?}"),
            format!("{key}"),
            format!("{:#?}", Some(&key)),
        ] {
            assert!(!text.contains("super-secret"), "{text}");
        }
        assert_eq!(key.expose(), "super-secret-key-value-123");
    }

    #[test]
    fn the_memory_store_sets_gets_and_deletes() {
        let store = MemorySecretStore::new();
        assert_eq!(store.get("a").unwrap(), None);
        assert!(!store.exists("a").unwrap());
        store.set("a", "one").unwrap();
        store.set("a", "two").unwrap();
        assert_eq!(store.get("a").unwrap().unwrap().expose(), "two");
        assert!(store.exists("a").unwrap());
        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert!(!store.exists("a").unwrap());
    }

    #[test]
    fn the_keychain_service_is_the_application_identifier() {
        assert_eq!(KEYCHAIN_SERVICE, "com.threadback.desktop");
    }
}
