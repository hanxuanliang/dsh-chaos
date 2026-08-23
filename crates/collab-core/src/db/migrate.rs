use turso::Connection;
use turso::transaction::TransactionBehavior;

use crate::{CollabError, Result};

pub(crate) const SCHEMA_VERSION: u32 = 9;

pub(crate) const META_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS collab_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

#[derive(Clone, Copy)]
struct Migration {
    version: u32,
    sql: &'static str,
}

const MIGRATIONS: [Migration; SCHEMA_VERSION as usize] = [
    Migration {
        version: 1,
        sql: include_str!("migrations/001_initial.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("migrations/002_runtime_session_unique.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("migrations/003_targets_and_threads.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("migrations/004_change_ledger.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("migrations/005_activity_inbox.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("migrations/006_agent_profile.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("migrations/007_target_access_view.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("migrations/008_agent_avatar.sql"),
    },
    Migration {
        version: 9,
        sql: include_str!("migrations/009_channel_lifecycle.sql"),
    },
];

pub(crate) async fn migrate(connection: &mut Connection) -> Result<()> {
    migrate_to(connection, SCHEMA_VERSION).await
}

async fn migrate_to(connection: &mut Connection, target_version: u32) -> Result<()> {
    validate_migrations()?;
    if target_version > SCHEMA_VERSION {
        return Err(CollabError::SchemaVersionMismatch {
            found: target_version.to_string(),
            expected: SCHEMA_VERSION,
        });
    }

    connection.execute_batch(META_SCHEMA).await?;
    let mut version = stored_version(connection).await?;
    if version > SCHEMA_VERSION {
        return Err(CollabError::SchemaVersionMismatch {
            found: version.to_string(),
            expected: SCHEMA_VERSION,
        });
    }

    while version < target_version {
        let migration =
            MIGRATIONS
                .get(version as usize)
                .ok_or_else(|| CollabError::SchemaVersionMismatch {
                    found: version.to_string(),
                    expected: SCHEMA_VERSION,
                })?;
        apply_migration(connection, *migration).await?;
        version = migration.version;
    }
    Ok(())
}

fn validate_migrations() -> Result<()> {
    for (index, migration) in MIGRATIONS.iter().enumerate() {
        let expected =
            u32::try_from(index + 1).map_err(|error| CollabError::Database(error.to_string()))?;
        if migration.version != expected {
            return Err(CollabError::SchemaVersionMismatch {
                found: migration.version.to_string(),
                expected: SCHEMA_VERSION,
            });
        }
    }
    Ok(())
}

async fn stored_version(connection: &Connection) -> Result<u32> {
    let mut rows = connection
        .query(
            "SELECT value FROM collab_meta WHERE key = 'schema_version'",
            (),
        )
        .await?;
    let stored = match rows.next().await? {
        Some(row) => Some(row.get::<String>(0)?),
        None => None,
    };
    match stored {
        Some(found) => found
            .parse::<u32>()
            .map_err(|_| CollabError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            }),
        None => Ok(0),
    }
}

async fn apply_migration(connection: &mut Connection, migration: Migration) -> Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await?;
    transaction.execute_batch(migration.sql).await?;
    transaction
        .execute(
            "INSERT INTO collab_meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [migration.version.to_string()],
        )
        .await?;
    transaction.commit().await?;
    Ok(())
}

#[cfg(test)]
mod migration_contract_tests {
    use super::*;

    async fn memory_connection() -> Connection {
        let database = turso::Builder::new_local(":memory:")
            .build()
            .await
            .expect("create in-memory database");
        database.connect().expect("connect in-memory database")
    }

    async fn schema(connection: &Connection) -> Vec<(String, String, String)> {
        let mut rows = connection
            .query(
                "SELECT type, name, COALESCE(sql, '') FROM sqlite_schema
                 WHERE name NOT LIKE 'sqlite_%'
                 ORDER BY type, name",
                (),
            )
            .await
            .expect("query schema");
        let mut schema = Vec::new();
        while let Some(row) = rows.next().await.expect("read schema row") {
            schema.push((
                row.get(0).expect("schema type"),
                row.get(1).expect("schema name"),
                row.get(2).expect("schema SQL"),
            ));
        }
        schema
    }

    #[tokio::test]
    async fn every_historical_version_upgrades_to_the_latest_schema() {
        let mut fresh = memory_connection().await;
        migrate(&mut fresh).await.expect("migrate fresh database");
        let expected = schema(&fresh).await;

        for historical_version in 1..SCHEMA_VERSION {
            let mut upgraded = memory_connection().await;
            migrate_to(&mut upgraded, historical_version)
                .await
                .expect("build historical schema");
            migrate(&mut upgraded)
                .await
                .expect("upgrade historical schema");
            assert_eq!(
                schema(&upgraded).await,
                expected,
                "schema upgraded from version {historical_version} differs from fresh latest"
            );
            assert_eq!(
                stored_version(&upgraded)
                    .await
                    .expect("read stored version"),
                SCHEMA_VERSION
            );
        }
    }
}
