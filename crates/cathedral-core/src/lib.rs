//! Cathedral's platform-neutral experiment management foundation.
//! Workload execution and sandbox isolation are deliberately out of scope here.

pub mod manifest;
pub mod registry;
pub mod workspace;

use std::path::{Path, PathBuf};

use manifest::Manifest;
use registry::{Experiment, Registry};
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
    #[error("unsafe workspace path: {0}")]
    UnsafePath(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, CathedralError>;

pub fn initialize(data_dir: &Path) -> Result<Registry> {
    std::fs::create_dir_all(data_dir)?;
    Workspace::new(data_dir)?.initialize()?;
    Registry::open(&data_dir.join("cathedral.sqlite3"))
}

pub fn create_experiment(
    data_dir: &Path,
    requested_name: &str,
    manifest: Manifest,
) -> Result<Experiment> {
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
    };

    if let Err(error) = registry.insert(&experiment) {
        let _ = workspace.remove(&path);
        return Err(error);
    }
    if let Err(error) = manifest.write_to(&path.join("cathedral.toml")) {
        let _ = registry.delete_by_id(&experiment.id);
        let _ = workspace.remove(&path);
        return Err(error);
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
    let registry = initialize(data_dir)?;
    let experiment = registry
        .get_by_name(name)?
        .ok_or_else(|| CathedralError::NotFound(name.to_owned()))?;
    Workspace::new(data_dir)?.remove(Path::new(&experiment.workspace_path))?;
    registry.delete_by_id(&experiment.id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifest::ExperimentSection;

    fn manifest(name: &str) -> Manifest {
        Manifest {
            manifest_version: 1,
            experiment: ExperimentSection {
                name: name.to_owned(),
                description: "test experiment".to_owned(),
            },
            environment: Default::default(),
        }
    }

    #[test]
    fn lifecycle_persists_across_registry_reopens() {
        let temp = tempfile::tempdir().unwrap();
        let created = create_experiment(temp.path(), "sample-lab", manifest("sample-lab")).unwrap();
        assert_eq!(created.name, "sample-lab");
        assert_eq!(list_experiments(temp.path()).unwrap().len(), 1);
        assert_eq!(inspect_experiment(temp.path(), "sample-lab").unwrap().id, created.id);
        delete_experiment(temp.path(), "sample-lab").unwrap();
        assert!(list_experiments(temp.path()).unwrap().is_empty());
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        create_experiment(temp.path(), "sample-lab", manifest("sample-lab")).unwrap();
        assert!(matches!(
            create_experiment(temp.path(), "sample-lab", manifest("sample-lab")),
            Err(CathedralError::AlreadyExists(_))
        ));
    }

    #[test]
    fn manifest_name_must_match_requested_name() {
        let temp = tempfile::tempdir().unwrap();
        assert!(create_experiment(temp.path(), "first", manifest("second")).is_err());
    }
}
