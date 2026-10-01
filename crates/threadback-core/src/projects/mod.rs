use chrono::Utc;

use crate::{
    CreateProjectInput, Database, DomainError, Project, ProjectBoundary, ProjectId,
    capture_snapshot,
};

/// Approves a folder as a ThreadBack project. Everything that would make a
/// later thread fail is checked here, so the user hears about it during
/// onboarding: the folder must exist, must not already be approved (compared
/// by canonical path, so symlinks cannot register it twice), and must be a
/// readable Git repository.
pub fn approve_project(
    database: &Database,
    input: CreateProjectInput,
) -> Result<Project, DomainError> {
    input.validate()?;
    let boundary = ProjectBoundary::new(&input.root_path, &[])?;
    let canonical_root = boundary.root().to_string_lossy().into_owned();

    if database.find_project_by_root(&canonical_root)?.is_some() {
        return Err(DomainError::invalid_input(
            "That folder is already an approved project.",
            "rootPath",
        ));
    }
    capture_snapshot(&boundary)
        .map_err(|error| DomainError::invalid_input(error.message, "rootPath"))?;

    let now = Utc::now();
    let project = Project {
        id: ProjectId::new(),
        display_name: input.display_name.trim().into(),
        canonical_root,
        created_at: now,
        updated_at: now,
    };
    database.insert_project(&project)?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;
    use std::{fs, path::Path, process::Command};
    use tempfile::TempDir;

    fn repository() -> TempDir {
        let dir = TempDir::new().unwrap();
        let status = Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        dir
    }

    fn input(name: &str, path: &Path) -> CreateProjectInput {
        CreateProjectInput {
            display_name: name.into(),
            root_path: path.to_string_lossy().into_owned(),
        }
    }

    #[test]
    fn approves_a_git_repository_with_its_canonical_root() {
        let db = Database::open_in_memory().unwrap();
        let repo = repository();
        let project = approve_project(&db, input("  Payments ", repo.path())).unwrap();
        assert_eq!(project.display_name, "Payments");
        assert_eq!(
            Path::new(&project.canonical_root),
            fs::canonicalize(repo.path()).unwrap()
        );
        assert_eq!(db.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn rejects_a_folder_that_is_not_a_git_repository() {
        let db = Database::open_in_memory().unwrap();
        let plain = TempDir::new().unwrap();
        let error = approve_project(&db, input("Plain", plain.path())).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(error.field.as_deref(), Some("rootPath"));
        assert!(error.message.contains("Git repository"));
        assert!(db.list_projects().unwrap().is_empty());
    }

    #[test]
    fn rejects_the_same_folder_twice_even_through_a_symlink() {
        let db = Database::open_in_memory().unwrap();
        let repo = repository();
        approve_project(&db, input("First", repo.path())).unwrap();

        let error = approve_project(&db, input("Again", repo.path())).unwrap_err();
        assert_eq!(error.field.as_deref(), Some("rootPath"));
        assert!(error.message.contains("already"));

        #[cfg(unix)]
        {
            let holder = TempDir::new().unwrap();
            let link = holder.path().join("alias");
            std::os::unix::fs::symlink(repo.path(), &link).unwrap();
            assert!(approve_project(&db, input("Alias", &link)).is_err());
        }
        assert_eq!(db.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn rejects_missing_folders_and_blank_names() {
        let db = Database::open_in_memory().unwrap();
        let repo = repository();
        let missing = repo.path().join("missing");
        assert_eq!(
            approve_project(&db, input("Name", &missing))
                .unwrap_err()
                .field
                .as_deref(),
            Some("rootPath")
        );
        assert_eq!(
            approve_project(&db, input("  ", repo.path()))
                .unwrap_err()
                .field
                .as_deref(),
            Some("displayName")
        );
        assert!(db.list_projects().unwrap().is_empty());
    }
}
