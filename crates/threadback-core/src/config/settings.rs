use serde::{Deserialize, Serialize};
use ts_rs::TS;
use url::{Host, Url};

use crate::DomainError;

pub const DEFAULT_ENDPOINT: &str = "https://api.tokenfactory.nebius.com/v1";
/// Nemotron 3 Super on Nebius Token Factory.
pub const DEFAULT_MODEL_ID: &str = "nvidia/nemotron-3-super-120b-a12b";
pub const DEFAULT_TIMEOUT_SECS: u32 = 60;
pub const MIN_TIMEOUT_SECS: u32 = 5;
pub const MAX_TIMEOUT_SECS: u32 = 300;

/// How to ask the service for JSON. Providers differ in what they accept, so
/// this is a setting, and the client steps down on its own if a mode is
/// rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StructuredOutput {
    /// Send the capsule schema as `response_format`.
    JsonSchema,
    /// Ask only for some JSON object.
    JsonObject,
    /// No `response_format`; rely on the prompt alone.
    Off,
}

/// Everything about the model connection except the API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettings {
    pub endpoint: String,
    pub model_id: String,
    pub timeout_secs: u32,
    pub structured_output: StructuredOutput,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.into(),
            model_id: DEFAULT_MODEL_ID.into(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            structured_output: StructuredOutput::JsonSchema,
        }
    }
}

impl ModelSettings {
    /// Checks every field and returns the settings in their normal form.
    pub fn validated(&self) -> Result<Self, DomainError> {
        let model_id = self.model_id.trim();
        let model_ok = !model_id.is_empty()
            && model_id.len() <= 200
            && model_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c));
        if !model_ok {
            return Err(DomainError::invalid_input(
                "The model ID must be 1 to 200 letters, digits, or . _ : / - characters.",
                "modelId",
            ));
        }
        if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&self.timeout_secs) {
            return Err(DomainError::invalid_input(
                format!(
                    "The timeout must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS} seconds."
                ),
                "timeoutSecs",
            ));
        }
        Ok(Self {
            endpoint: normalize_endpoint(&self.endpoint)?,
            model_id: model_id.to_owned(),
            timeout_secs: self.timeout_secs,
            structured_output: self.structured_output,
        })
    }

    /// The host evidence would be sent to, for showing the user.
    pub fn destination_host(&self) -> String {
        Url::parse(&self.endpoint)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The URL requests are posted to.
    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.endpoint.trim_end_matches('/'))
    }
}

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => *name == "localhost",
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    }
}

/// An endpoint must be `https`. Plain `http` is accepted only for a loopback
/// address, so a local server can stand in during development and tests but
/// evidence can never cross a network unencrypted. Credentials, a query, and a
/// fragment in the URL are refused.
pub fn normalize_endpoint(raw: &str) -> Result<String, DomainError> {
    let invalid = |message: &str| DomainError::invalid_input(message.to_owned(), "endpoint");
    let url = Url::parse(raw.trim()).map_err(|_| invalid("The endpoint is not a valid URL."))?;
    let host = url
        .host()
        .ok_or_else(|| invalid("The endpoint needs a host name."))?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&host) => {}
        "http" => return Err(invalid("The endpoint must use https.")),
        _ => return Err(invalid("The endpoint must start with https://.")),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid(
            "The endpoint must not contain a user name or password.",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(invalid(
            "The endpoint must not contain a query or a fragment.",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_valid_and_point_at_token_factory() {
        let settings = ModelSettings::default();
        assert_eq!(settings.validated().unwrap(), settings);
        assert_eq!(settings.destination_host(), "api.tokenfactory.nebius.com");
        assert_eq!(
            settings.chat_completions_url(),
            "https://api.tokenfactory.nebius.com/v1/chat/completions"
        );
    }

    #[test]
    fn endpoints_are_normalised() {
        for (input, expected) in [
            (
                "https://api.tokenfactory.nebius.com/v1/",
                "https://api.tokenfactory.nebius.com/v1",
            ),
            (
                "  https://api.tokenfactory.nebius.com/v1  ",
                "https://api.tokenfactory.nebius.com/v1",
            ),
            ("https://example.com", "https://example.com"),
            (
                "https://example.com:8443/api/",
                "https://example.com:8443/api",
            ),
            ("HTTPS://Example.COM/V1", "https://example.com/V1"),
        ] {
            assert_eq!(normalize_endpoint(input).unwrap(), expected, "{input}");
        }
    }

    #[test]
    fn plain_http_is_allowed_only_for_loopback() {
        for ok in [
            "http://127.0.0.1:8080/v1",
            "http://localhost:9000",
            "http://[::1]:3000/v1",
        ] {
            assert!(normalize_endpoint(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://api.tokenfactory.nebius.com/v1",
            "http://example.com",
            "http://10.0.0.5/v1",
            "http://localhost.evil.example/v1",
            "http://127.0.0.1.evil.example/v1",
        ] {
            let error = normalize_endpoint(bad).unwrap_err();
            assert_eq!(error.field.as_deref(), Some("endpoint"), "{bad}");
            assert!(error.message.contains("https"), "{bad}: {}", error.message);
        }
    }

    #[test]
    fn unsafe_or_malformed_endpoints_are_refused() {
        for bad in [
            "",
            "not a url",
            "ftp://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:pass@example.com/v1",
            "https://user@example.com/v1",
            "https://example.com/v1?api_key=abc",
            "https://example.com/v1#frag",
            "https://",
        ] {
            let error = normalize_endpoint(bad).unwrap_err();
            assert_eq!(error.field.as_deref(), Some("endpoint"), "{bad:?}");
        }
    }

    #[test]
    fn model_ids_and_timeouts_are_checked() {
        let with = |model: &str, timeout: u32| ModelSettings {
            model_id: model.into(),
            timeout_secs: timeout,
            ..ModelSettings::default()
        };
        assert!(
            with("nvidia/Nemotron-3-Ultra-550b-a55b", 60)
                .validated()
                .is_ok()
        );
        assert!(with("  nvidia/x  ", 60).validated().unwrap().model_id == "nvidia/x");
        for bad in [
            "",
            "   ",
            "has space",
            "semi;colon",
            "quo\"te",
            "new\nline",
            &"a".repeat(201),
        ] {
            let error = with(bad, 60).validated().unwrap_err();
            assert_eq!(error.field.as_deref(), Some("modelId"), "{bad:?}");
        }
        for bad in [0, 4, 301, u32::MAX] {
            assert_eq!(
                with("m", bad).validated().unwrap_err().field.as_deref(),
                Some("timeoutSecs"),
                "{bad}"
            );
        }
        assert!(with("m", 5).validated().is_ok() && with("m", 300).validated().is_ok());
    }

    #[test]
    fn settings_serialize_with_the_names_the_interface_uses() {
        let value = serde_json::to_value(ModelSettings::default()).unwrap();
        assert_eq!(value["modelId"], DEFAULT_MODEL_ID);
        assert_eq!(value["timeoutSecs"], 60);
        assert_eq!(value["structuredOutput"], "json_schema");
    }
}
