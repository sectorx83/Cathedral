//! Versioned experiment manifest parsing and validation.

use crate::{CathedralError, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest_version: u32,
    pub experiment: ExperimentSection,
    #[serde(default)]
    pub environment: EnvironmentSection,
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

impl Manifest {
    pub fn minimal(name: impl Into<String>) -> Self {
        Self {
            manifest_version: 1,
            experiment: ExperimentSection {
                name: name.into(),
                description: String::new(),
            },
            environment: EnvironmentSection::default(),
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

    pub fn write_to(&self, path: &Path) -> Result<()> {
        let encoded = toml::to_string_pretty(self)
            .map_err(|error| CathedralError::Manifest(error.to_string()))?;
        fs::write(path, encoded)?;
        Ok(())
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
                || relative.is_absolute()
                || relative.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::RootDir
                    )
                })
            {
                return Err(CathedralError::Manifest(format!(
                    "working_directory must be a safe relative path, got '{path}'"
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

    #[test]
    fn accepts_valid_manifest() {
        let parsed = Manifest::parse(
            "manifest_version = 1\n[experiment]\nname = \"demo-lab\"\ndescription = \"A test\"\n[environment]\nworking_directory = \"src\"\n[environment.variables]\nRUST_LOG = \"info\""
        ).unwrap();
        assert_eq!(parsed.experiment.name, "demo-lab");
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
        assert!(validate_name("../bad").is_err());
        assert!(validate_name("Uppercase").is_err());
    }
}
