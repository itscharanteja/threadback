//! M-01: the model connection's settings and its write-only API key.

use std::fs;

use tempfile::TempDir;
use threadback_core::{
    ConfigurationService, Database, ErrorCode, MemorySecretStore, ModelConfigInput, ModelSettings,
    SecretStore, StructuredOutput,
};

const KEY: &str = "tfk-7F4A91C2-demo-secret-key-value-0001";

fn input(key: Option<&str>) -> ModelConfigInput {
    ModelConfigInput {
        endpoint: "https://api.tokenfactory.nebius.com/v1/".into(),
        model_id: "nvidia/nemotron-3-super-120b-a12b".into(),
        timeout_secs: 45,
        structured_output: StructuredOutput::JsonObject,
        api_key: key.map(str::to_owned),
    }
}

struct Setup {
    _dir: TempDir,
    path: std::path::PathBuf,
    db: Database,
    secrets: MemorySecretStore,
}

fn setup() -> Setup {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("threadback.db");
    let db = Database::open(&path).unwrap();
    Setup {
        _dir: dir,
        path,
        db,
        secrets: MemorySecretStore::new(),
    }
}

fn database_text(path: &std::path::Path) -> String {
    let mut bytes = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        if let Ok(content) = fs::read(format!("{}{suffix}", path.display())) {
            bytes.extend(content);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn a_fresh_install_has_the_token_factory_defaults_and_no_key() {
    let s = setup();
    let status = ConfigurationService::new(&s.db, &s.secrets)
        .status()
        .unwrap();
    let defaults = ModelSettings::default();
    assert_eq!(status.endpoint, defaults.endpoint);
    assert_eq!(status.model_id, "nvidia/nemotron-3-super-120b-a12b");
    assert_eq!(status.structured_output, StructuredOutput::JsonSchema);
    assert!(!status.has_api_key);
    assert_eq!(status.destination_host, "api.tokenfactory.nebius.com");
}

#[test]
fn saving_stores_the_settings_normalised_and_the_key_only_in_the_secret_store() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);
    let status = service.store(input(Some(&format!("  {KEY}  ")))).unwrap();

    assert_eq!(status.endpoint, "https://api.tokenfactory.nebius.com/v1");
    assert_eq!(status.timeout_secs, 45);
    assert_eq!(status.structured_output, StructuredOutput::JsonObject);
    assert!(status.has_api_key);
    assert_eq!(
        s.secrets
            .get("token-factory-api-key")
            .unwrap()
            .unwrap()
            .expose(),
        KEY,
        "trimmed, in the store"
    );
}

#[test]
fn the_key_is_never_returned_to_the_interface() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);
    let status = service.store(input(Some(KEY))).unwrap();

    let json = serde_json::to_string(&status).unwrap();
    assert!(!json.contains(KEY) && !json.contains("7F4A91C2"), "{json}");
    // The only thing it says about the key is whether one exists.
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let fields: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert!(
        !fields.iter().any(|name| *name == "apiKey"),
        "no key field: {fields:?}"
    );
    assert_eq!(value["hasApiKey"], true);
    assert_eq!(service.status().unwrap(), status);
    assert!(!format!("{status:?}").contains("7F4A91C2"));
}

#[test]
fn the_key_is_never_written_to_the_database_files() {
    let s = setup();
    {
        let service = ConfigurationService::new(&s.db, &s.secrets);
        service.store(input(Some(KEY))).unwrap();
        service.store(input(None)).unwrap();
    }
    drop(s.db);
    let text = database_text(&s.path);
    assert!(!text.contains(KEY));
    for piece in KEY.as_bytes().windows(10) {
        assert!(
            !text.contains(std::str::from_utf8(piece).unwrap()),
            "a piece of the key is in the database"
        );
    }
    assert!(
        text.contains("nemotron-3-super"),
        "the non-secret settings are stored"
    );
}

#[test]
fn the_input_cannot_leak_the_key_through_debug_output() {
    let printed = format!("{:?} {:#?}", input(Some(KEY)), input(Some(KEY)));
    assert!(
        !printed.contains("7F4A91C2") && !printed.contains(KEY),
        "{printed}"
    );
    assert!(printed.contains("<hidden>"));
    assert!(!format!("{:?}", input(None)).contains("<hidden>"));
}

#[test]
fn saving_without_a_key_keeps_the_stored_one() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);
    service.store(input(Some(KEY))).unwrap();
    let mut changed = input(None);
    changed.model_id = "nvidia/Nemotron-3-Ultra-550b-a55b".into();
    let status = service.store(changed).unwrap();
    assert!(status.has_api_key);
    assert_eq!(status.model_id, "nvidia/Nemotron-3-Ultra-550b-a55b");
    assert_eq!(
        s.secrets
            .get("token-factory-api-key")
            .unwrap()
            .unwrap()
            .expose(),
        KEY
    );
}

#[test]
fn a_new_key_replaces_the_old_one_and_the_key_can_be_removed() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);
    service.store(input(Some(KEY))).unwrap();
    service
        .store(input(Some("a-second-valid-key-1234")))
        .unwrap();
    assert_eq!(
        s.secrets
            .get("token-factory-api-key")
            .unwrap()
            .unwrap()
            .expose(),
        "a-second-valid-key-1234"
    );

    let status = service.remove_api_key().unwrap();
    assert!(!status.has_api_key);
    assert!(s.secrets.get("token-factory-api-key").unwrap().is_none());
    assert!(service.remove_api_key().is_ok(), "removing again is fine");
    assert_eq!(
        status.endpoint, "https://api.tokenfactory.nebius.com/v1",
        "settings survive"
    );
}

#[test]
fn a_bad_value_changes_nothing_not_even_the_key() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);
    service.store(input(Some(KEY))).unwrap();

    let mut bad = input(Some("a-brand-new-valid-key-9999"));
    bad.endpoint = "http://example.com/v1".into();
    let error = service.store(bad).unwrap_err();
    assert_eq!(
        (error.code, error.field.as_deref()),
        (ErrorCode::InvalidInput, Some("endpoint"))
    );
    assert_eq!(
        s.secrets
            .get("token-factory-api-key")
            .unwrap()
            .unwrap()
            .expose(),
        KEY,
        "the old key is intact"
    );
    assert_eq!(service.settings().unwrap().timeout_secs, 45);
}

#[test]
fn keys_that_do_not_look_like_keys_are_refused_without_echoing_them() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);
    for bad in [
        "",
        "   ",
        "short",
        "has a space in it",
        "tab\there-key-1234",
        &"k".repeat(513),
        "new\nline-key-1234",
    ] {
        let error = service.store(input(Some(bad))).unwrap_err();
        assert_eq!(error.field.as_deref(), Some("apiKey"), "{bad:?}");
        if bad.len() > 6 {
            assert!(!error.message.contains(bad.trim()), "{}", error.message);
        }
    }
    assert!(!service.status().unwrap().has_api_key);
}

#[test]
fn settings_survive_a_restart() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("threadback.db");
    let secrets = MemorySecretStore::new();
    {
        let db = Database::open(&path).unwrap();
        ConfigurationService::new(&db, &secrets)
            .store(input(Some(KEY)))
            .unwrap();
    }
    let db = Database::open(&path).unwrap();
    let status = ConfigurationService::new(&db, &secrets).status().unwrap();
    assert_eq!(status.timeout_secs, 45);
    assert_eq!(status.structured_output, StructuredOutput::JsonObject);
    assert!(status.has_api_key);
}

#[test]
fn unreadable_saved_settings_fall_back_to_the_defaults() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);
    service.store(input(None)).unwrap();
    // Simulate an older or damaged value.
    drop(service);
    let conn_path = s.path.clone();
    drop(s.db);
    let connection = rusqlite_open(&conn_path);
    connection
        .execute(
            "UPDATE metadata SET value = '{not json' WHERE key = 'model_settings'",
            [],
        )
        .unwrap();
    drop(connection);
    let db = Database::open(&conn_path).unwrap();
    let status = ConfigurationService::new(&db, &s.secrets).status().unwrap();
    assert_eq!(status.model_id, ModelSettings::default().model_id);
}

fn rusqlite_open(path: &std::path::Path) -> Connection {
    Connection::open(path).unwrap()
}
use rusqlite::Connection;

#[test]
fn a_connection_test_uses_the_typed_key_or_falls_back_to_the_stored_one() {
    let s = setup();
    let service = ConfigurationService::new(&s.db, &s.secrets);

    let (_, key) = service.resolve_for_test(&input(None)).unwrap();
    assert!(key.is_none(), "nothing typed and nothing stored");

    service.store(input(Some(KEY))).unwrap();
    let (_, key) = service.resolve_for_test(&input(None)).unwrap();
    assert_eq!(key.unwrap().expose(), KEY);

    let (settings, key) = service
        .resolve_for_test(&input(Some("typed-but-unsaved-key-77")))
        .unwrap();
    assert_eq!(key.unwrap().expose(), "typed-but-unsaved-key-77");
    assert_eq!(settings.timeout_secs, 45);
    assert_eq!(
        s.secrets
            .get("token-factory-api-key")
            .unwrap()
            .unwrap()
            .expose(),
        KEY,
        "testing does not save"
    );

    let mut bad = input(None);
    bad.endpoint = "ftp://x".into();
    assert!(service.resolve_for_test(&bad).is_err());
}
