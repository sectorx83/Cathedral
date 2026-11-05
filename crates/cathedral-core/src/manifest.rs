//! Versioned experiment manifest parsing and validation.

use crate::{CathedralError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest_version: u32,
    pub experiment: ExperimentSection,
    #[serde(default)]
    pub environment: EnvironmentSection,
    #[serde(default)]
    pub dependencies: DependencySection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentSection {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSection {
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct DependencySection {
    /// Declared package names and version requirements. Phase 1 records these
    /// values but deliberately does not resolve or install packages.
    #[serde(default)]
    pub packages: BTreeMap<String, String>,
}

impl Manifest {
    pub fn minimal(name: impl Into<String>) -> Self {
        Self {
            manifest_version: 1,
            experiment: ExperimentSection {
                name: name.into(),
                description: String::new(),
            },
            environment: EnvironmentSection::default(),
            dependencies: DependencySection::default(),
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        let manifest: Self =
            toml::from_str(input).map_err(|error| CathedralError::Manifest(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::parse(&fs::read_to_string(path)?)
    }

    /// Persist a complete manifest using a temporary sibling file and rename,
    /// so a process interruption cannot leave a partially written TOML file.
    pub fn write_to(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let encoded = toml::to_string_pretty(self)
            .map_err(|error| CathedralError::Manifest(error.to_string()))?;
        let parent = path.parent().ok_or_else(|| {
            CathedralError::Manifest("manifest destination has no parent directory".to_owned())
        })?;
        let file_name = path.file_name().ok_or_else(|| {
            CathedralError::Manifest("manifest destination has no file name".to_owned())
        })?;
        let temporary = parent.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            uuid::Uuid::new_v4()
        ));

        let write_result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(encoded.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }

    pub fn validate(&self) -> Result<()> {
        if self.manifest_version != 1 {
            return Err(CathedralError::Manifest(format!(
                "unsupported manifest_version {}; supported version is 1",
                self.manifest_version
            )));
        }
        validate_name(&self.experiment.name)?;
        if self.experiment.description.len() > 4096 {
            return Err(CathedralError::Manifest(
                "experiment.description must be at most 4096 bytes".to_owned(),
            ));
        }
        if let Some(path) = &self.environment.working_directory {
            let relative = Path::new(path);
            if path.is_empty()
                || path.as_bytes().contains(&0)
                || relative.is_absolute()
                || path.starts_with('/')
                || path.contains('\\')
                || path.contains(':')
                || path.split('/').any(|part| part == ".." || part.is_empty())
            {
                return Err(CathedralError::Manifest(format!(
                    "working_directory must be a portable relative path without traversal, got '{path}'"
                )));
            }
        }
        for (name, requirement) in &self.dependencies.packages {
            let valid_name = !name.is_empty()
                && name.len() <= 128
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.'
                });
            if !valid_name {
                return Err(CathedralError::Manifest(format!(
                    "invalid dependency package name '{name}'"
                )));
            }
            if requirement.trim().is_empty()
                || requirement.len() > 256
                || requirement.as_bytes().contains(&0)
            {
                return Err(CathedralError::Manifest(format!(
                    "dependency requirement for '{name}' must be 1 to 256 non-empty bytes"
                )));
            }
        }
        for key in self.environment.variables.keys() {
            let mut chars = key.chars();
            let first_valid = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            if !first_valid || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(CathedralError::Manifest(format!(
                    "invalid environment variable name '{key}'"
                )));
            }
        }
        Ok(())
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 48
        && bytes[0].is_ascii_lowercase()
        && (bytes[bytes.len() - 1].is_ascii_lowercase() || bytes[bytes.len() - 1].is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if !valid {
        return Err(CathedralError::InvalidName(name.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn accepts_valid_manifest() {
        let parsed = Manifest::parse(
            "manifest_version = 1\n[experiment]\nname = \"demo-lab\"\ndescription = \"A test\"\n[environment]\nworking_directory = \"src\"\n[environment.variables]\nRUST_LOG = \"info\"\n[dependencies.packages]\nserde = \"^1.0\"",
        )
        .unwrap();
        assert_eq!(parsed.experiment.name, "demo-lab");
        assert_eq!(parsed.dependencies.packages["serde"], "^1.0");
    }

    #[test]
    fn rejects_unknown_fields_and_versions() {
        assert!(Manifest::parse(
            "manifest_version = 1\n[experiment]\nname = \"demo\"\nunknown = \"nope\""
        )
        .is_err());
        let unsupported = Manifest {
            manifest_version: 99,
            ..Manifest::minimal("demo")
        };
        assert!(unsupported.validate().is_err());
    }

    #[test]
    fn rejects_traversal_and_bad_names() {
        let mut manifest = Manifest::minimal("demo");
        manifest.environment.working_directory = Some("../outside".to_owned());
        assert!(manifest.validate().is_err());
        manifest.environment.working_directory = Some("C:\\Windows".to_owned());
        assert!(manifest.validate().is_err());
        manifest.environment.working_directory = Some("src//nested".to_owned());
        assert!(manifest.validate().is_err());
        assert!(validate_name("../bad").is_err());
        assert!(validate_name("Uppercase").is_err());
    }

    #[test]
    fn validates_dependency_metadata_without_resolving_it() {
        let mut manifest = Manifest::minimal("dependency-lab");
        manifest
            .dependencies
            .packages
            .insert("serde".to_owned(), "^1.0".to_owned());
        assert!(manifest.validate().is_ok());
        manifest
            .dependencies
            .packages
            .insert("../escape".to_owned(), "*".to_owned());
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn writes_and_reloads_manifest_atomically() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("cathedral.toml");
        let manifest = Manifest::minimal("atomic-lab");
        manifest.write_to(&path).unwrap();
        assert_eq!(Manifest::load(&path).unwrap(), manifest);
        assert_eq!(
            std::fs::read_dir(temp.path()).unwrap().count(),
            1,
            "temporary file should not remain after a successful write"
        );
    }
}
