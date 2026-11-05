//! Cathedral's platform-neutral experiment management foundation.
//! Workload execution and sandbox isolation are deliberately out of scope here.

pub mod manifest;
pub mod platform;
pub mod registry;
pub mod runtime;
pub mod runner;
pub mod workspace;

use std::path::{Path, PathBuf};

use manifest::Manifest;
use registry::{Experiment, ExperimentState, Registry};
use thiserror::Error;
use workspace::Workspace;

#[derive(Debug, Error)]
pub enum CathedralError {
    #[error("invalid experiment name: {0}")]
    InvalidName(String),
    #[error("experiment already exists: {0}")]
    AlreadyExists(String),
    #[error("experiment not found: {0}")]
    NotFound(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("registry schema error: {0}")]
    RegistrySchema(String),
    #[error("registry integrity check failed: {0}")]
    RegistryIntegrity(String),
    #[error("unsafe workspace path: {0}")]
    UnsafePath(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, CathedralError>;

pub fn initialize(data_dir: &Path) -> Result<Registry> {
    tracing::debug!(data_dir = %data_dir.display(), "initializing Cathedral storage");
    std::fs::create_dir_all(data_dir)?;
    let canonical_data_dir = data_dir.canonicalize()?;
    Workspace::new(&canonical_data_dir)?.initialize()?;
    Registry::open(&canonical_data_dir.join("cathedral.sqlite3"))
}

pub fn create_experiment(
    data_dir: &Path,
    requested_name: &str,
    manifest: Manifest,
) -> Result<Experiment> {
    tracing::debug!(experiment = requested_name, "creating experiment");
    manifest.validate()?;
    if manifest.experiment.name != requested_name {
        return Err(CathedralError::Manifest(format!(
            "manifest name '{}' does not match requested name '{}'",
            manifest.experiment.name, requested_name
        )));
    }

    let registry = initialize(data_dir)?;
    if registry.get_by_name(requested_name)?.is_some() {
        return Err(CathedralError::AlreadyExists(requested_name.to_owned()));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let workspace = Workspace::new(data_dir)?;
    let path = workspace.create(&id)?;
    let experiment = Experiment {
        id,
        name: manifest.experiment.name.clone(),
        description: manifest.experiment.description.clone(),
        workspace_path: path.to_string_lossy().into_owned(),
        created_at: registry::unix_timestamp(),
        manifest_version: manifest.manifest_version,
        state: ExperimentState::Ready,
    };

    // Write the manifest before registering the experiment. If persistence
    // fails, the registry never exposes a partially initialized experiment.
    if let Err(error) = manifest.write_to(&path.join("cathedral.toml")) {
        let cleanup = workspace.remove(&path);
        return match cleanup {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(CathedralError::Manifest(format!(
                "{error}; workspace cleanup also failed: {cleanup_error}"
            ))),
        };
    }

    if let Err(error) = registry.insert(&experiment) {
        let cleanup = workspace.remove(&path);
        return match cleanup {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(CathedralError::Manifest(format!(
                "{error}; workspace cleanup also failed: {cleanup_error}"
            ))),
        };
    }

    Ok(experiment)
}

pub fn inspect_experiment(data_dir: &Path, name: &str) -> Result<Experiment> {
    initialize(data_dir)?
        .get_by_name(name)?
        .ok_or_else(|| CathedralError::NotFound(name.to_owned()))
}

pub fn list_experiments(data_dir: &Path) -> Result<Vec<Experiment>> {
    initialize(data_dir)?.list()
}

pub fn delete_experiment(data_dir: &Path, name: &str) -> Result<()> {
    tracing::debug!(experiment = name, "deleting experiment");
    let registry = initialize(data_dir)?;
    let experiment = registry
        .get_by_name(name)?
        .ok_or_else(|| CathedralError::NotFound(name.to_owned()))?;

    // Persist intent before touching the filesystem. If the process stops
    // midway, a later delete command can safely resume the cleanup.
    registry.mark_deleting(&experiment.id)?;
    Workspace::new(data_dir)?.remove_if_exists(Path::new(&experiment.workspace_path))?;
    registry.delete_by_id(&experiment.id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_rejects_duplicate_names_without_leaving_extra_workspaces() {
        let temp = tempdir().unwrap();
        let first =
            create_experiment(temp.path(), "repeat-lab", Manifest::minimal("repeat-lab")).unwrap();
        assert!(matches!(
            create_experiment(temp.path(), "repeat-lab", Manifest::minimal("repeat-lab")),
            Err(CathedralError::AlreadyExists(_))
        ));
        let items = list_experiments(temp.path()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, first.id);
        assert_eq!(
            std::fs::read_dir(temp.path().join("experiments"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn delete_resumes_after_a_persisted_deleting_state() {
        let temp = tempdir().unwrap();
        let item =
            create_experiment(temp.path(), "resume-lab", Manifest::minimal("resume-lab")).unwrap();
        initialize(temp.path())
            .unwrap()
            .mark_deleting(&item.id)
            .unwrap();

        delete_experiment(temp.path(), "resume-lab").unwrap();
        assert!(list_experiments(temp.path()).unwrap().is_empty());
        assert!(!Path::new(&item.workspace_path).exists());
    }

    #[test]
    fn delete_resumes_cleanup_when_workspace_was_already_removed() {
        let temp = tempdir().unwrap();
        let item = create_experiment(temp.path(), "missing-lab", Manifest::minimal("missing-lab"))
            .unwrap();
        std::fs::remove_dir_all(&item.workspace_path).unwrap();

        delete_experiment(temp.path(), "missing-lab").unwrap();
        assert!(matches!(
            inspect_experiment(temp.path(), "missing-lab"),
            Err(CathedralError::NotFound(_))
        ));
        assert!(list_experiments(temp.path()).unwrap().is_empty());
        assert_eq!(item.state, ExperimentState::Ready);
    }
}
