use chrono::Utc;

use crate::{
    CreateProjectInput, Database, DomainError, Project, ProjectBoundary, ProjectId,
    RelinkProjectInput, capture_snapshot,
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

/// Relinks an existing project to a moved or renamed folder on disk while
/// keeping all threads, events, checkpoints, and approved capsules intact.
pub fn relink_project(
    database: &Database,
    input: RelinkProjectInput,
) -> Result<Project, DomainError> {
    input.validate()?;
    if database.find_project(&input.project_id)?.is_none() {
        return Err(DomainError::not_found("That project no longer exists."));
    }
    let policy = database
        .find_project_policy(&input.project_id)
        .unwrap_or_else(|_| crate::ProjectPolicy::default_for(input.project_id.clone()));
    let boundary = ProjectBoundary::new(&input.root_path, &policy.exclusions)?;
    let canonical_root = boundary.root().to_string_lossy().into_owned();

    if let Some(existing) = database.find_project_by_root(&canonical_root)? {
        if existing.id != input.project_id {
            return Err(DomainError::invalid_input(
                "That folder is already linked to another approved project.",
                "rootPath",
            ));
        }
    }
    capture_snapshot(&boundary)
        .map_err(|error| DomainError::invalid_input(error.message, "rootPath"))?;

    database.update_project_root(&input.project_id, &canonical_root)
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

    #[test]
    fn relinks_a_moved_project_folder_and_preserves_project_identity() {
        let db = Database::open_in_memory().unwrap();
        let original = repository();
        let project = approve_project(&db, input("Payments", original.path())).unwrap();

        let moved = repository();
        let relinked = relink_project(
            &db,
            RelinkProjectInput {
                project_id: project.id.clone(),
                root_path: moved.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap();

        assert_eq!(relinked.id, project.id);
        assert_eq!(relinked.display_name, "Payments");
        assert_eq!(
            Path::new(&relinked.canonical_root),
            fs::canonicalize(moved.path()).unwrap()
        );
    }

    #[test]
    fn relink_rejects_unknown_project_non_git_folder_and_another_projects_root() {
        let db = Database::open_in_memory().unwrap();
        let repo_a = repository();
        let repo_b = repository();
        let plain = TempDir::new().unwrap();

        let project_a = approve_project(&db, input("Alpha", repo_a.path())).unwrap();
        let _project_b = approve_project(&db, input("Beta", repo_b.path())).unwrap();

        // Unknown project ID
        let err_missing = relink_project(
            &db,
            RelinkProjectInput {
                project_id: ProjectId("proj-does-not-exist".into()),
                root_path: repo_a.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(err_missing.code, ErrorCode::NotFound);

        // Non-Git folder
        let err_plain = relink_project(
            &db,
            RelinkProjectInput {
                project_id: project_a.id.clone(),
                root_path: plain.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(err_plain.code, ErrorCode::InvalidInput);
        assert_eq!(err_plain.field.as_deref(), Some("rootPath"));

        // Already linked to another approved project
        let err_dup = relink_project(
            &db,
            RelinkProjectInput {
                project_id: project_a.id.clone(),
                root_path: repo_b.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(err_dup.code, ErrorCode::InvalidInput);
        assert!(err_dup.message.contains("already linked"));
    }
}
