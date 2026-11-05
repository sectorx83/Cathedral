//! SQLite-backed local experiment registry.

use crate::{CathedralError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experiment {
    pub id: String,
    pub name: String,
    pub description: String,
    pub workspace_path: String,
    pub created_at: i64,
    pub manifest_version: u32,
}

pub struct Registry {
    connection: Connection,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version(version)
                 SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);
             CREATE TABLE IF NOT EXISTS experiments (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 description TEXT NOT NULL DEFAULT '',
                 workspace_path TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 manifest_version INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_experiments_created_at
                 ON experiments(created_at);",
        )?;
        let version: i64 =
            connection.query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })?;
        if version != 1 {
            return Err(CathedralError::Manifest(format!(
                "unsupported registry schema version {version}"
            )));
        }
        Ok(Self { connection })
    }

    pub fn insert(&self, item: &Experiment) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO experiments
             (id, name, description, workspace_path, created_at, manifest_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    item.id,
                    item.name,
                    item.description,
                    item.workspace_path,
                    item.created_at,
                    item.manifest_version
                ],
            )
            .map_err(|error| {
                if matches!(&error, rusqlite::Error::SqliteFailure(code, _)
                if code.code == rusqlite::ErrorCode::ConstraintViolation)
                {
                    CathedralError::AlreadyExists(item.name.clone())
                } else {
                    CathedralError::Sqlite(error)
                }
            })?;
        Ok(())
    }

    pub fn get_by_name(&self, name: &str) -> Result<Option<Experiment>> {
        self.connection
            .query_row(
                "SELECT id, name, description, workspace_path, created_at, manifest_version
             FROM experiments WHERE name = ?1",
                [name],
                row_to_experiment,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list(&self) -> Result<Vec<Experiment>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, description, workspace_path, created_at, manifest_version
             FROM experiments ORDER BY created_at ASC, name ASC",
        )?;
        let rows = statement.query_map([], row_to_experiment)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn delete_by_id(&self, id: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM experiments WHERE id = ?1", [id])?;
        Ok(())
    }
}

fn row_to_experiment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Experiment> {
    Ok(Experiment {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        workspace_path: row.get(3)?,
        created_at: row.get(4)?,
        manifest_version: row.get(5)?,
    })
}

pub fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
