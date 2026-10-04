//! The real macOS Keychain. Skipped by default because it touches the user's
//! keychain and a permission prompt can appear:
//!
//!     cargo test -p threadback-core --test keychain -- --ignored
//!
//! It uses a service name of its own and removes what it creates.

#![cfg(all(target_os = "macos", feature = "keychain"))]

use threadback_core::SecretStore;
use threadback_core::config::KeychainSecretStore;

const ACCOUNT: &str = "token-factory-api-key";

#[test]
#[ignore = "touches the real macOS Keychain"]
fn the_key_round_trips_through_the_real_keychain_and_can_be_removed() {
    let service = format!("com.threadback.desktop.test.{}", std::process::id());
    let store = KeychainSecretStore::with_service(&service);
    let secret = "tfk-7F4A91C2-keychain-round-trip-0001";

    assert!(
        store.get(ACCOUNT).unwrap().is_none(),
        "nothing there to begin with"
    );
    store.set(ACCOUNT, secret).unwrap();
    assert_eq!(store.get(ACCOUNT).unwrap().unwrap().expose(), secret);
    assert!(store.exists(ACCOUNT).unwrap());

    store.set(ACCOUNT, "replaced-key-value-0002").unwrap();
    assert_eq!(
        store.get(ACCOUNT).unwrap().unwrap().expose(),
        "replaced-key-value-0002"
    );

    store.delete(ACCOUNT).unwrap();
    store.delete(ACCOUNT).unwrap();
    assert!(store.get(ACCOUNT).unwrap().is_none());
}
