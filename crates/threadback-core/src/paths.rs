use std::path::PathBuf;

/// Matches `identifier` in `src-tauri/tauri.conf.json`; a test keeps them equal.
pub const APP_IDENTIFIER: &str = "com.threadback.desktop";

/// Overrides the database location, for development, tests, and the demo.
pub const DATABASE_ENV: &str = "THREADBACK_DB";

/// Overrides the exports directory for tests.
pub const EXPORTS_ENV: &str = "THREADBACK_EXPORTS_DIR";

/// The one database file shared by the desktop app and the `tb` command, so
/// the two can never disagree about where evidence lives.
pub fn database_path() -> PathBuf {
    if let Some(path) = std::env::var_os(DATABASE_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    data_directory().join("threadback.db")
}

/// Directory where exported JSON and Markdown capsules are stored.
/// When `THREADBACK_DB` is set (and `THREADBACK_EXPORTS_DIR` is not), exports
/// live next to that database under `exports/` so tests stay isolated.
pub fn exports_directory() -> PathBuf {
    if let Some(path) = std::env::var_os(EXPORTS_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(db) = std::env::var_os(DATABASE_ENV).filter(|value| !value.is_empty()) {
        if let Some(parent) = PathBuf::from(db).parent() {
            return parent.join("exports");
        }
    }
    #[cfg(test)]
    {
        std::env::temp_dir().join("threadback-test-exports")
    }
    #[cfg(not(test))]
    {
        let preferred = data_directory().join("exports");
        if std::fs::create_dir_all(&preferred).is_ok() {
            preferred
        } else {
            std::env::temp_dir().join("threadback-exports")
        }
    }
}

/// Directory where exports for a specific thread are written.
pub fn thread_exports_directory(thread_id: &crate::ThreadId) -> PathBuf {
    exports_directory().join(&thread_id.0)
}

#[cfg(target_os = "macos")]
fn data_directory() -> PathBuf {
    home()
        .join("Library/Application Support")
        .join(APP_IDENTIFIER)
}

#[cfg(windows)]
fn data_directory() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(home)
        .join(APP_IDENTIFIER)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn data_directory() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
        .join(APP_IDENTIFIER)
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_matches_the_tauri_configuration() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../../src-tauri/tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], APP_IDENTIFIER);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn default_location_is_the_applications_support_folder() {
        let path = data_directory();
        assert!(path.ends_with("Library/Application Support/com.threadback.desktop"));
    }
}
