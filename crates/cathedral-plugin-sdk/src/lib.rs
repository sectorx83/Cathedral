//! Stable, declarative contracts for Cathedral extensions.
//!
//! This crate validates extension metadata and permission requests. It does not
//! load or execute extension code. The Studio host must enforce every grant.

use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const STUDIO_API_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    ReadExperiments,
    ReadRunLogs,
    ReadWorkspaceFiles,
    CreateExperiments,
    StartRuns,
    StopRuns,
    CreateSnapshots,
    RestoreSnapshots,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<PluginPermission>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginManifestError {
    #[error("unsupported plugin manifest schema version {0}")]
    UnsupportedSchema(u32),
    #[error("plugin id must be 2-64 lowercase ASCII letters, digits, or single hyphens")]
    InvalidId,
    #[error("plugin name must contain 1-96 characters")]
    InvalidName,
    #[error("plugin description must be at most 512 characters")]
    DescriptionTooLong,
    #[error("plugin version must be a valid semantic version")]
    InvalidVersion,
    #[error("plugin requires unsupported Studio API version {0}")]
    UnsupportedApi(u32),
    #[error("plugin permissions must not contain duplicates")]
    DuplicatePermission,
    #[error("invalid plugin manifest: {0}")]
    Parse(String),
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), PluginManifestError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(PluginManifestError::UnsupportedSchema(self.schema_version));
        }
        let id = self.id.as_bytes();
        if !(2..=64).contains(&id.len())
            || !id[0].is_ascii_lowercase()
            || !id[id.len() - 1].is_ascii_lowercase() && !id[id.len() - 1].is_ascii_digit()
            || !id.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
            || self.id.contains("--")
        {
            return Err(PluginManifestError::InvalidId);
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 96 {
            return Err(PluginManifestError::InvalidName);
        }
        if self.description.chars().count() > 512 {
            return Err(PluginManifestError::DescriptionTooLong);
        }
        if Version::parse(&self.version).is_err() {
            return Err(PluginManifestError::InvalidVersion);
        }
        if self.api_version != STUDIO_API_VERSION {
            return Err(PluginManifestError::UnsupportedApi(self.api_version));
        }
        let unique: BTreeSet<_> = self.permissions.iter().collect();
        if unique.len() != self.permissions.len() {
            return Err(PluginManifestError::DuplicatePermission);
        }
        Ok(())
    }

    pub fn from_toml(source: &str) -> Result<Self, PluginManifestError> {
        let manifest: Self = toml::from_str(source)
            .map_err(|error| PluginManifestError::Parse(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn to_toml(&self) -> Result<String, PluginManifestError> {
        self.validate()?;
        toml::to_string_pretty(self)
            .map_err(|error| PluginManifestError::Parse(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            id: "sample-inspector".to_owned(),
            name: "Sample Inspector".to_owned(),
            version: "1.2.3".to_owned(),
            api_version: 1,
            description: "Reads experiment metadata".to_owned(),
            permissions: vec![PluginPermission::ReadExperiments],
        }
    }

    #[test]
    fn validates_and_round_trips_manifest() {
        let original = manifest();
        let encoded = original.to_toml().unwrap();
        assert_eq!(PluginManifest::from_toml(&encoded).unwrap(), original);
    }

    #[test]
    fn rejects_duplicate_permissions() {
        let mut value = manifest();
        value.permissions.push(PluginPermission::ReadExperiments);
        assert_eq!(value.validate(), Err(PluginManifestError::DuplicatePermission));
    }

    #[test]
    fn rejects_invalid_ids_and_api_versions() {
        let mut value = manifest();
        value.id = "../escape".to_owned();
        assert_eq!(value.validate(), Err(PluginManifestError::InvalidId));
        value.id = "valid-plugin".to_owned();
        value.api_version = 99;
        assert_eq!(value.validate(), Err(PluginManifestError::UnsupportedApi(99)));
    }
}
