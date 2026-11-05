//! SQLite-backed local experiment registry.

use crate::{CathedralError, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const REGISTRY_SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperimentState {
    Ready,
    Deleting,
}

impl ExperimentState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Deleting => "deleting",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experiment {
    pub id: String,
    pub name: String,
    pub description: String,
    pub workspace_path: String,
    pub created_at: i64,
    pub manifest_version: u32,
    pub state: ExperimentState,
}

pub struct Registry {
    connection: Connection,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        tracing::debug!(registry_path = %path.display(), "opening experiment registry");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;

        // The first release used a one-row schema_version table. The second
        // schema uses SQLite's transactional user_version marker and persists
        // lifecycle state so interrupted deletions can be resumed safely.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_version: i64 =
            transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        match current_version {
            0 => {
                let legacy_table_exists = table_exists(&transaction, "schema_version")?;
                if legacy_table_exists {
                    let mut statement =
                        transaction.prepare("SELECT version FROM schema_version ORDER BY rowid")?;
                    let versions = statement
                        .query_map([], |row| row.get::<_, i64>(0))?
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    drop(statement);

                    if versions.len() != 1 || versions.first() != Some(&1) {
                        return Err(CathedralError::RegistrySchema(format!(
                            "legacy schema marker is invalid or unsupported: {versions:?}"
                        )));
                    }
                    transaction.execute_batch("DROP TABLE schema_version;")?;
                }
                migrate_to_v2(&transaction)?;
            }
            1 => migrate_to_v2(&transaction)?,
            REGISTRY_SCHEMA_VERSION => {
                create_experiments_table(&transaction)?;
                transaction.execute_batch(
                    "CREATE INDEX IF NOT EXISTS idx_experiments_created_at
                        ON experiments(created_at);",
                )?;
            }
            version => {
                return Err(CathedralError::RegistrySchema(format!(
                    "database schema version {version} is not supported; this build supports {REGISTRY_SCHEMA_VERSION}"
                )));
            }
        }

        verify_experiment_columns(&transaction)?;
        transaction.commit()?;

        tracing::debug!(
            schema_version = REGISTRY_SCHEMA_VERSION,
            "experiment registry ready"
        );
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<i64> {
        self.connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Into::into)
    }

    pub fn integrity_check(&self) -> Result<()> {
        let result: String = self
            .connection
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
                 (id, name, description, workspace_path, created_at, manifest_version, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    item.id,
                    item.name,
                    item.description,
                    item.workspace_path,
                    item.created_at,
                    item.manifest_version,
                    item.state.as_str()
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
                "SELECT id, name, description, workspace_path, created_at, manifest_version, state
                 FROM experiments WHERE name = ?1",
                [name],
                row_to_experiment,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list(&self) -> Result<Vec<Experiment>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, description, workspace_path, created_at, manifest_version, state
             FROM experiments ORDER BY created_at ASC, name ASC",
        )?;
        let rows = statement.query_map([], row_to_experiment)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn mark_deleting(&self, id: &str) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE experiments SET state = 'deleting' WHERE id = ?1",
            [id],
        )?;
        if changed == 0 {
            return Err(CathedralError::NotFound(id.to_owned()));
        }
        Ok(())
    }

    pub fn delete_by_id(&self, id: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM experiments WHERE id = ?1", [id])?;
        Ok(())
    }
}

fn table_exists(connection: &Connection, name: &str) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [name],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|found| found.as_str() == column))
}

fn create_experiments_table(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS experiments (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL DEFAULT '',
            workspace_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            manifest_version INTEGER NOT NULL,
            state TEXT NOT NULL DEFAULT 'ready'
                CHECK (state IN ('ready', 'deleting'))
        );
        CREATE INDEX IF NOT EXISTS idx_experiments_created_at
            ON experiments(created_at);",
    )?;
    Ok(())
}

fn migrate_to_v2(transaction: &Transaction<'_>) -> Result<()> {
    create_experiments_table(transaction)?;
    if !has_column(transaction, "experiments", "state")? {
        transaction.execute_batch(
            "ALTER TABLE experiments
             ADD COLUMN state TEXT NOT NULL DEFAULT 'ready'
             CHECK (state IN ('ready', 'deleting'));",
        )?;
    }
    transaction.execute_batch("PRAGMA user_version = 2;")?;
    Ok(())
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
        "state",
    ];
    let missing: Vec<_> = required
        .iter()
        .filter(|column| !columns.iter().any(|found| found.as_str() == **column))
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
    let state: String = row.get(6)?;
    let state = match state.as_str() {
        "ready" => ExperimentState::Ready,
        "deleting" => ExperimentState::Deleting,
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown experiment state '{state}'"),
                )),
            ));
        }
    };
    Ok(Experiment {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        workspace_path: row.get(3)?,
        created_at: row.get(4)?,
        manifest_version: row.get(5)?,
        state,
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
        connection
            .execute_batch(
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
            )
            .unwrap();
        drop(connection);

        let registry = Registry::open(&path).unwrap();
        let migrated = registry.get_by_name("legacy").unwrap().unwrap();
        assert_eq!(migrated.id, "id");
        assert_eq!(migrated.state, ExperimentState::Ready);
        assert_eq!(registry.schema_version().unwrap(), REGISTRY_SCHEMA_VERSION);
    }

    #[test]
    fn rejects_ambiguous_legacy_schema_markers() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("corrupt.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version(version) VALUES (1), (1);",
            )
            .unwrap();
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
        connection
            .execute_batch("PRAGMA user_version = 99;")
            .unwrap();
        drop(connection);

        assert!(matches!(
            Registry::open(&path),
            Err(CathedralError::RegistrySchema(_))
        ));
    }

    #[test]
    fn persists_lifecycle_state() {
        let temp = tempdir().unwrap();
        let registry = Registry::open(&temp.path().join("state.sqlite3")).unwrap();
        let experiment = Experiment {
            id: "id".to_owned(),
            name: "state-lab".to_owned(),
            description: String::new(),
            workspace_path: "/tmp/state-lab".to_owned(),
            created_at: 1,
            manifest_version: 1,
            state: ExperimentState::Ready,
        };
        registry.insert(&experiment).unwrap();
        registry.mark_deleting("id").unwrap();
        assert_eq!(
            registry.get_by_name("state-lab").unwrap().unwrap().state,
            ExperimentState::Deleting
        );
    }
    #[test]
    fn migrates_schema_version_one_to_lifecycle_schema_two() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("version-one.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE experiments (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL UNIQUE,
                    description TEXT NOT NULL DEFAULT '',
                    workspace_path TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    manifest_version INTEGER NOT NULL
                );
                INSERT INTO experiments VALUES ('id', 'version-one', '', '/tmp/version-one', 1, 1);
                PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let registry = Registry::open(&path).unwrap();
        let migrated = registry.get_by_name("version-one").unwrap().unwrap();
        assert_eq!(migrated.state, ExperimentState::Ready);
        assert_eq!(registry.schema_version().unwrap(), 2);
    }
}
