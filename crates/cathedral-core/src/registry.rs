//! SQLite-backed local experiment registry.

use crate::{CathedralError, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const REGISTRY_SCHEMA_VERSION: i64 = 1;

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

        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;

        // SQLite's user_version is a single, transactional schema marker. The
        // first release used a one-column schema_version table, so migrate it
        // without discarding or rewriting experiment records.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_version: i64 =
            transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if current_version == 0 {
            let legacy_table_exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version')",
                [],
                |row| row.get(0),
            )?;

            if legacy_table_exists {
                let mut statement =
                    transaction.prepare("SELECT version FROM schema_version ORDER BY rowid")?;
                let versions = statement
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                drop(statement);

                if versions.len() != 1 || versions[0] != REGISTRY_SCHEMA_VERSION {
                    return Err(CathedralError::RegistrySchema(format!(
                        "legacy schema marker is invalid or unsupported: {versions:?}"
                    )));
                }
                transaction.execute_batch("DROP TABLE schema_version;")?;
            }

            transaction.execute_batch(&format!(
                "CREATE TABLE IF NOT EXISTS experiments (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL UNIQUE,
                    description TEXT NOT NULL DEFAULT '',
                    workspace_path TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    manifest_version INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_experiments_created_at
                    ON experiments(created_at);
                PRAGMA user_version = {REGISTRY_SCHEMA_VERSION};"
            ))?;
        } else if current_version != REGISTRY_SCHEMA_VERSION {
            return Err(CathedralError::RegistrySchema(format!(
                "database schema version {current_version} is not supported; this build supports {REGISTRY_SCHEMA_VERSION}"
            )));
        } else {
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS experiments (
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
        }

        verify_experiment_columns(&transaction)?;
        transaction.commit()?;

        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Into::into)
    }

    pub fn integrity_check(&self) -> Result<()> {
        let result: String =
            self.connection
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if result != "ok" {
            return Err(CathedralError::RegistryIntegrity(result));
        }
        Ok(())
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

fn verify_experiment_columns(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare("PRAGMA table_info(experiments)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let required = [
        "id",
        "name",
        "description",
        "workspace_path",
        "created_at",
        "manifest_version",
    ];
    let missing: Vec<_> = required
        .iter()
        .filter(|column| !columns.iter().any(|found| found == **column))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(CathedralError::RegistrySchema(format!(
            "experiments table is missing required columns: {}",
            missing.join(", ")
        )));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn opens_new_registry_and_checks_integrity() {
        let temp = tempdir().unwrap();
        let registry = Registry::open(&temp.path().join("cathedral.sqlite3")).unwrap();
        assert_eq!(registry.schema_version().unwrap(), REGISTRY_SCHEMA_VERSION);
        registry.integrity_check().unwrap();
    }

    #[test]
    fn migrates_the_previous_schema_marker_without_losing_data() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("legacy.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version(version) VALUES (1);
             CREATE TABLE experiments (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL UNIQUE,
                 description TEXT NOT NULL DEFAULT '',
                 workspace_path TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 manifest_version INTEGER NOT NULL
             );
             INSERT INTO experiments VALUES ('id', 'legacy', '', '/tmp/legacy', 1, 1);",
        ).unwrap();
        drop(connection);

        let registry = Registry::open(&path).unwrap();
        assert_eq!(registry.get_by_name("legacy").unwrap().unwrap().id, "id");
        assert_eq!(registry.schema_version().unwrap(), REGISTRY_SCHEMA_VERSION);
    }

    #[test]
    fn rejects_ambiguous_legacy_schema_markers() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("corrupt.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version(version) VALUES (1), (1);",
        ).unwrap();
        drop(connection);

        assert!(matches!(
            Registry::open(&path),
            Err(CathedralError::RegistrySchema(_))
        ));
    }

    #[test]
    fn rejects_future_schema_versions() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("future.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA user_version = 99;").unwrap();
        drop(connection);

        assert!(matches!(
            Registry::open(&path),
            Err(CathedralError::RegistrySchema(_))
        ));
    }
}
