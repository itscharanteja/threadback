use std::path::PathBuf;

/// Matches `identifier` in `src-tauri/tauri.conf.json`; a test keeps them equal.
pub const APP_IDENTIFIER: &str = "com.threadback.desktop";

/// Overrides the database location, for development, tests, and the demo.
pub const DATABASE_ENV: &str = "THREADBACK_DB";

/// The one database file shared by the desktop app and the `tb` command, so
/// the two can never disagree about where evidence lives.
pub fn database_path() -> PathBuf {
    if let Some(path) = std::env::var_os(DATABASE_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    data_directory().join("threadback.db")
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
