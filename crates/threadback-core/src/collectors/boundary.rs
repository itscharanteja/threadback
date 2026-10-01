use std::{
    ffi::OsString,
    fs, io,
    path::{Component, Path, PathBuf},
};

use crate::DomainError;

/// Directory names that are never captured, at any depth.
const DEFAULT_EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".cache",
    ".gradle",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    "Pods",
    "DerivedData",
];

/// File-name patterns for secrets and binary content, matched at any depth.
const DEFAULT_EXCLUDED_PATTERNS: &[&str] = &[
    ".env",
    ".env.*",
    ".netrc",
    ".npmrc",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.jks",
    "*.keystore",
    "id_rsa",
    "id_rsa.*",
    "id_ecdsa",
    "id_ecdsa.*",
    "id_ed25519",
    "id_ed25519.*",
    "*.png",
    "*.jpg",
    "*.jpeg",
    "*.gif",
    "*.webp",
    "*.ico",
    "*.icns",
    "*.pdf",
    "*.zip",
    "*.gz",
    "*.tgz",
    "*.bz2",
    "*.xz",
    "*.7z",
    "*.rar",
    "*.tar",
    "*.jar",
    "*.war",
    "*.class",
    "*.o",
    "*.a",
    "*.so",
    "*.dylib",
    "*.dll",
    "*.exe",
    "*.bin",
    "*.dmg",
    "*.pkg",
    "*.wasm",
    "*.mp3",
    "*.mp4",
    "*.mov",
    "*.avi",
    "*.sqlite",
    "*.sqlite3",
    "*.db",
    "*.ttf",
    "*.otf",
    "*.woff",
    "*.woff2",
];

/// Exclusions shown during onboarding so the user can review them.
pub fn default_exclusions() -> Vec<String> {
    DEFAULT_EXCLUDED_DIRECTORIES
        .iter()
        .chain(DEFAULT_EXCLUDED_PATTERNS)
        .map(|pattern| (*pattern).to_owned())
        .collect()
}

/// A path that passed every boundary check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    /// Canonical absolute path with all symlinks resolved.
    pub absolute: PathBuf,
    /// Forward-slash path relative to the project root; `.` for the root itself.
    pub relative: String,
}

/// The only gate through which a path may become evidence.
///
/// A path is accepted only when its fully resolved location is inside the
/// canonical project root and neither its link name nor its target matches an
/// exclusion. Paths containing `..` are rejected outright, even when they would
/// land inside the root, because lexical collapsing can disagree with the
/// filesystem when a symlink precedes the `..`.
#[derive(Debug, Clone)]
pub struct ProjectBoundary {
    root: PathBuf,
    exclusions: Vec<Exclusion>,
}

impl ProjectBoundary {
    /// `extra_exclusions` are user-configured patterns added to the defaults.
    pub fn new(root: impl AsRef<Path>, extra_exclusions: &[String]) -> Result<Self, DomainError> {
        let root = fs::canonicalize(root.as_ref()).map_err(|_| {
            DomainError::invalid_input(
                "The selected project folder does not exist or cannot be opened.",
                "rootPath",
            )
        })?;
        if !root.is_dir() {
            return Err(DomainError::invalid_input(
                "The selected project root must be a folder.",
                "rootPath",
            ));
        }
        if root.parent().is_none() {
            return Err(DomainError::invalid_input(
                "The filesystem root cannot be approved as a project.",
                "rootPath",
            ));
        }

        let mut exclusions = DEFAULT_EXCLUDED_DIRECTORIES
            .iter()
            .chain(DEFAULT_EXCLUDED_PATTERNS)
            .map(|pattern| Exclusion::parse(pattern))
            .collect::<Result<Vec<_>, _>>()?;
        for pattern in extra_exclusions {
            exclusions.push(Exclusion::parse(pattern)?);
        }

        Ok(Self { root, exclusions })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a relative or absolute candidate to a path inside the project.
    pub fn resolve(&self, candidate: impl AsRef<Path>) -> Result<ResolvedPath, DomainError> {
        let candidate = candidate.as_ref();
        if candidate.as_os_str().is_empty() {
            return Err(DomainError::boundary("A file path is required."));
        }
        if candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(DomainError::boundary(
                "Paths containing '..' are not allowed inside a project.",
            ));
        }

        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.root.join(candidate)
        };
        let canonical = canonicalize_lenient(&joined)?;
        let relative = canonical.strip_prefix(&self.root).map_err(|_| {
            if joined.starts_with(&self.root) {
                DomainError::boundary(
                    "A symbolic link inside the project points outside the project folder.",
                )
            } else {
                DomainError::boundary("That path is outside the approved project folder.")
            }
        })?;

        // The link name and its target are both checked, so a link called
        // `notes.txt` cannot expose `.env`, and a link called `.env` stays hidden.
        if let Ok(lexical) = joined.strip_prefix(&self.root) {
            self.check_exclusions(lexical)?;
        }
        self.check_exclusions(relative)?;

        Ok(ResolvedPath {
            absolute: canonical.clone(),
            relative: to_relative_string(relative),
        })
    }

    /// True when a root-relative path matches an exclusion.
    pub fn is_excluded(&self, relative: impl AsRef<Path>) -> bool {
        self.check_exclusions(relative.as_ref()).is_err()
    }

    fn check_exclusions(&self, relative: &Path) -> Result<(), DomainError> {
        let names = normal_components(relative);
        for exclusion in &self.exclusions {
            if exclusion.matches(&names) {
                return Err(DomainError::boundary(format!(
                    "That path is excluded from capture by the '{}' rule.",
                    exclusion.source
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct Exclusion {
    source: String,
    pattern: String,
    anchored: bool,
}

impl Exclusion {
    fn parse(raw: &str) -> Result<Self, DomainError> {
        let trimmed = raw.trim();
        let cleaned = trimmed
            .strip_prefix("./")
            .unwrap_or(trimmed)
            .trim_end_matches('/');
        if cleaned.is_empty()
            || cleaned.starts_with('/')
            || cleaned
                .split('/')
                .any(|part| part == ".." || part.is_empty())
        {
            return Err(DomainError::invalid_input(
                format!("'{trimmed}' is not a valid exclusion pattern."),
                "exclusions",
            ));
        }
        Ok(Self {
            source: cleaned.to_owned(),
            pattern: cleaned.to_lowercase(),
            anchored: cleaned.contains('/'),
        })
    }

    /// Patterns without `/` match any single path component by name; patterns
    /// with `/` are anchored at the project root and match any leading run of
    /// components. Matching ignores case because macOS volumes do.
    fn matches(&self, components: &[String]) -> bool {
        if self.anchored {
            (1..=components.len()).any(|end| {
                let prefix = components[..end].join("/").to_lowercase();
                wildcard_match(&self.pattern, &prefix)
            })
        } else {
            components
                .iter()
                .any(|name| wildcard_match(&self.pattern, &name.to_lowercase()))
        }
    }
}

/// `*` matches any run of characters except `/`; `?` matches one non-`/`.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;

    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                backtrack = Some((p, t));
                p += 1;
            }
            Some('?') if text[t] != '/' => {
                p += 1;
                t += 1;
            }
            Some(&literal) if literal != '?' && literal == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match backtrack {
                Some((star_p, star_t)) if text[star_t] != '/' => {
                    backtrack = Some((star_p, star_t + 1));
                    p = star_p + 1;
                    t = star_t + 1;
                }
                _ => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

fn normal_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn to_relative_string(relative: &Path) -> String {
    let names = normal_components(relative);
    if names.is_empty() {
        ".".to_owned()
    } else {
        names.join("/")
    }
}

/// Canonicalizes the deepest existing ancestor and re-appends the missing
/// remainder, so deleted or not-yet-created files still resolve against the
/// real location of their parent directory.
fn canonicalize_lenient(path: &Path) -> Result<PathBuf, DomainError> {
    let mut existing = path.to_path_buf();
    let mut remainder: Vec<OsString> = Vec::new();

    loop {
        match fs::canonicalize(&existing) {
            Ok(mut canonical) => {
                canonical.extend(remainder.iter().rev());
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A dangling symlink reports NotFound, but its target is unknown.
                if fs::symlink_metadata(&existing).is_ok() {
                    return Err(DomainError::boundary(
                        "A broken symbolic link cannot be verified to stay inside the project.",
                    ));
                }
                match (existing.file_name(), existing.parent()) {
                    (Some(name), Some(parent)) => {
                        remainder.push(name.to_owned());
                        existing = parent.to_path_buf();
                    }
                    _ => {
                        return Err(DomainError::boundary("That path cannot be resolved."));
                    }
                }
            }
            Err(_) => return Err(DomainError::boundary("That path cannot be read.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;
    use tempfile::TempDir;

    fn project() -> (TempDir, ProjectBoundary) {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "fn main() {}").unwrap();
        let boundary = ProjectBoundary::new(dir.path(), &[]).unwrap();
        (dir, boundary)
    }

    fn assert_rejected(boundary: &ProjectBoundary, candidate: impl AsRef<Path>) -> DomainError {
        let error = boundary.resolve(candidate).unwrap_err();
        assert_eq!(error.code, ErrorCode::ProjectBoundaryViolation);
        error
    }

    #[test]
    fn accepts_relative_and_absolute_paths_inside_the_root() {
        let (dir, boundary) = project();
        assert_eq!(
            boundary.resolve("src/lib.rs").unwrap().relative,
            "src/lib.rs"
        );
        assert_eq!(
            boundary
                .resolve(dir.path().join("src/lib.rs"))
                .unwrap()
                .relative,
            "src/lib.rs"
        );
        assert_eq!(
            boundary.resolve("./src//lib.rs").unwrap().relative,
            "src/lib.rs"
        );
    }

    #[test]
    fn accepts_paths_that_do_not_exist_yet() {
        let (_dir, boundary) = project();
        let resolved = boundary.resolve("src/deleted/gone.rs").unwrap();
        assert_eq!(resolved.relative, "src/deleted/gone.rs");
    }

    #[test]
    fn root_resolves_to_dot() {
        let (dir, boundary) = project();
        assert_eq!(boundary.resolve(dir.path()).unwrap().relative, ".");
    }

    #[test]
    fn rejects_parent_traversal_even_when_it_stays_inside() {
        let (_dir, boundary) = project();
        assert_rejected(&boundary, "../outside.txt");
        assert_rejected(&boundary, "src/../../outside.txt");
        assert_rejected(&boundary, "src/../src/lib.rs");
    }

    #[test]
    fn rejects_absolute_paths_outside_the_root() {
        let (_dir, boundary) = project();
        let other = TempDir::new().unwrap();
        fs::write(other.path().join("secret.txt"), "x").unwrap();
        assert_rejected(&boundary, other.path().join("secret.txt"));
        assert_rejected(&boundary, "/etc/hosts");
    }

    #[test]
    fn rejects_sibling_directory_sharing_the_root_name_prefix() {
        let parent = TempDir::new().unwrap();
        let root = parent.path().join("app");
        let sibling = parent.path().join("app-evil");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        let boundary = ProjectBoundary::new(&root, &[]).unwrap();
        assert_rejected(&boundary, sibling.join("file.txt"));
    }

    #[test]
    fn rejects_empty_path() {
        let (_dir, boundary) = project();
        assert_rejected(&boundary, "");
    }

    #[cfg(unix)]
    mod symlinks {
        use super::*;
        use std::os::unix::fs::symlink;

        #[test]
        fn rejects_symlinked_directory_pointing_outside() {
            let (dir, boundary) = project();
            let outside = TempDir::new().unwrap();
            fs::write(outside.path().join("secret.txt"), "x").unwrap();
            symlink(outside.path(), dir.path().join("escape")).unwrap();

            let error = assert_rejected(&boundary, "escape/secret.txt");
            assert!(error.message.contains("symbolic link"));
            assert_rejected(&boundary, "escape");
            assert_rejected(&boundary, dir.path().join("escape/new-file.txt"));
        }

        #[test]
        fn rejects_symlinked_file_pointing_outside() {
            let (dir, boundary) = project();
            let outside = TempDir::new().unwrap();
            let target = outside.path().join("secret.txt");
            fs::write(&target, "x").unwrap();
            symlink(&target, dir.path().join("notes.txt")).unwrap();
            assert_rejected(&boundary, "notes.txt");
        }

        #[test]
        fn rejects_dangling_symlink() {
            let (dir, boundary) = project();
            symlink("/definitely/not/here", dir.path().join("dangling")).unwrap();
            assert_rejected(&boundary, "dangling");
        }

        #[test]
        fn accepts_symlink_that_stays_inside() {
            let (dir, boundary) = project();
            symlink(dir.path().join("src"), dir.path().join("alias")).unwrap();
            assert_eq!(
                boundary.resolve("alias/lib.rs").unwrap().relative,
                "src/lib.rs"
            );
        }

        #[test]
        fn rejects_inside_symlink_whose_target_is_excluded() {
            let (dir, boundary) = project();
            fs::write(dir.path().join(".env"), "TOKEN=x").unwrap();
            symlink(dir.path().join(".env"), dir.path().join("notes.txt")).unwrap();
            assert_rejected(&boundary, "notes.txt");
        }

        #[test]
        fn rejects_excluded_link_name_pointing_at_ordinary_file() {
            let (dir, boundary) = project();
            symlink(dir.path().join("src/lib.rs"), dir.path().join(".env")).unwrap();
            assert_rejected(&boundary, ".env");
        }

        #[test]
        fn symlinked_root_is_canonicalized() {
            let real = TempDir::new().unwrap();
            let holder = TempDir::new().unwrap();
            let link = holder.path().join("link");
            symlink(real.path(), &link).unwrap();
            fs::write(real.path().join("a.txt"), "x").unwrap();
            let boundary = ProjectBoundary::new(&link, &[]).unwrap();
            assert_eq!(boundary.root(), fs::canonicalize(real.path()).unwrap());
            assert_eq!(
                boundary.resolve(link.join("a.txt")).unwrap().relative,
                "a.txt"
            );
        }
    }

    #[test]
    fn default_exclusions_cover_secrets_dependencies_and_binaries() {
        let (_dir, boundary) = project();
        for path in [
            ".git/config",
            "node_modules/react/index.js",
            "service/target/debug/app",
            ".env",
            ".env.production",
            "config/server.pem",
            "keys/id_rsa",
            "assets/Logo.PNG",
            "build/output.js",
        ] {
            assert_rejected(&boundary, path);
            assert!(boundary.is_excluded(path), "{path} should be excluded");
        }
        assert!(!boundary.is_excluded("src/lib.rs"));
        assert!(!boundary.is_excluded("environment.md"));
        assert!(!boundary.is_excluded("src/builder.rs"));
    }

    #[test]
    fn exclusion_messages_name_the_rule() {
        let (_dir, boundary) = project();
        let error = assert_rejected(&boundary, "config/.env.local");
        assert!(error.message.contains(".env.*"));
    }

    #[test]
    fn user_patterns_extend_the_defaults() {
        let dir = TempDir::new().unwrap();
        let boundary = ProjectBoundary::new(
            dir.path(),
            &["secrets".into(), "docs/private".into(), "*.log".into()],
        )
        .unwrap();
        assert!(boundary.is_excluded("secrets/a.txt"));
        assert!(boundary.is_excluded("pkg/secrets/a.txt"));
        assert!(boundary.is_excluded("docs/private/notes.md"));
        assert!(!boundary.is_excluded("pkg/docs/private/notes.md"));
        assert!(boundary.is_excluded("logs/app.log"));
        assert!(!boundary.is_excluded("docs/public.md"));
    }

    #[test]
    fn rejects_malformed_user_patterns() {
        let dir = TempDir::new().unwrap();
        for pattern in ["", "  ", "/abs", "../up", "a//b"] {
            let error = ProjectBoundary::new(dir.path(), &[pattern.into()]).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput, "pattern {pattern:?}");
        }
    }

    #[test]
    fn rejects_unusable_roots() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "x").unwrap();
        for root in [dir.path().join("missing"), file, PathBuf::from("/")] {
            let error = ProjectBoundary::new(&root, &[]).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput, "root {root:?}");
        }
    }

    #[test]
    fn wildcard_matching_does_not_cross_directories() {
        assert!(wildcard_match("*.rs", "lib.rs"));
        assert!(wildcard_match("a*c", "abbbc"));
        assert!(wildcard_match("a?c", "abc"));
        assert!(!wildcard_match("*.rs", "src/lib.rs"));
        assert!(!wildcard_match("a?c", "a/c"));
        assert!(wildcard_match("docs/*", "docs/a"));
        assert!(!wildcard_match("docs/*", "docs/a/b"));
    }
}
