use turso::Connection;
use turso::transaction::TransactionBehavior;

use crate::{CollabError, Result};

pub(crate) const SCHEMA_VERSION: u32 = 6;

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
];

#[cfg(test)]
pub(crate) const SCHEMA_V1: &str = include_str!("migrations/001_initial.sql");
#[cfg(test)]
pub(crate) const SCHEMA_V2: &str = include_str!("migrations/002_runtime_session_unique.sql");
#[cfg(test)]
pub(crate) const SCHEMA_V3: &str = include_str!("migrations/003_targets_and_threads.sql");
#[cfg(test)]
pub(crate) const SCHEMA_V4: &str = include_str!("migrations/004_change_ledger.sql");
#[cfg(test)]
pub(crate) const SCHEMA_V5: &str = include_str!("migrations/005_activity_inbox.sql");

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

    #[tokio::test]
    async fn failed_migration_rolls_back_schema_and_version() {
        let mut connection = memory_connection().await;
        migrate_to(&mut connection, 1)
            .await
            .expect("build version one schema");

        let result = apply_migration(
            &mut connection,
            Migration {
                version: 2,
                sql: "CREATE TABLE migration_probe (id INTEGER);\n\
                      INSERT INTO missing_table (id) VALUES (1);",
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            stored_version(&connection)
                .await
                .expect("read stored version after failure"),
            1
        );
        let mut rows = connection
            .query(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'migration_probe'",
                (),
            )
            .await
            .expect("query migration probe");
        let row = rows
            .next()
            .await
            .expect("read migration probe row")
            .expect("migration probe count row");
        assert_eq!(row.get::<i64>(0).expect("migration probe count"), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[tokio::test]
    async fn schema_v4_upgrades_change_ledger_for_activity_done() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("v4.db");
        let user_id = "018f0000-0000-7000-8000-000000000001";
        let target_id = "018f0000-0000-7000-8000-000000000002";
        {
            let database =
                turso::Builder::new_local(path.to_str().ok_or_else(|| {
                    CollabError::Filesystem("temporary path is not UTF-8".into())
                })?)
                .build()
                .await?;
            let mut connection = database.connect()?;
            connection.execute_batch(META_SCHEMA).await?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            transaction.execute_batch(SCHEMA_V1).await?;
            transaction.execute_batch(SCHEMA_V2).await?;
            transaction.execute_batch(SCHEMA_V3).await?;
            transaction.execute_batch(SCHEMA_V4).await?;
            transaction
                .execute(
                    "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                         VALUES (?1, 'user', 'legacy-owner', 'Legacy Owner', 1)",
                    [user_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO targets
                         (id, kind, name, created_by, created_at_ms, archived_at_ms)
                         VALUES (?1, 'channel', 'legacy', ?2, 1, NULL)",
                    (target_id, user_id),
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO memberships
                         (target_id, actor_id, role, joined_at_ms, left_at_ms)
                         VALUES (?1, ?2, 'owner', 1, NULL)",
                    (target_id, user_id),
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO messages
                         (id, target_id, author_id, client_request_id, body_json, created_at_ms)
                         VALUES ('018f0000-0000-7000-8000-000000000003', ?1, ?2,
                                 'legacy-request', '{\"kind\":\"text\",\"text\":\"legacy\"}', 1)",
                    (target_id, user_id),
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO change_events
                         (kind, target_id, entity_id, created_at_ms)
                         VALUES ('message_created', ?1,
                                 '018f0000-0000-7000-8000-000000000003', 1)",
                    [target_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO change_recipients (change_seq, actor_id) VALUES (1, ?1)",
                    [user_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '4')",
                    (),
                )
                .await?;
            transaction.commit().await?;
        }

        let core = CollabCore::open(&path).await?;
        let page = core.inbox_list(user_id, 20, None).await?;
        assert_eq!(page.active_count, 1);
        let through_seq = page.items[0].last_activity_seq;
        core.inbox_done(user_id, target_id, through_seq).await?;
        let changes = core.list_changes(user_id, 0, 10).await?;
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::MessageCreated);
        assert_eq!(changes[1].kind, ChangeKind::ActivityDoneChanged);
        Ok(())
    }

    #[tokio::test]
    async fn schema_v5_upgrades_agents_with_default_charter() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("v5.db");
        let agent_id = "018f0000-0000-7000-8000-000000000011";
        {
            let database =
                turso::Builder::new_local(path.to_str().ok_or_else(|| {
                    CollabError::Filesystem("temporary path is not UTF-8".into())
                })?)
                .build()
                .await?;
            let mut connection = database.connect()?;
            connection.execute_batch(META_SCHEMA).await?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            transaction.execute_batch(SCHEMA_V1).await?;
            transaction.execute_batch(SCHEMA_V2).await?;
            transaction.execute_batch(SCHEMA_V3).await?;
            transaction.execute_batch(SCHEMA_V4).await?;
            transaction.execute_batch(SCHEMA_V5).await?;
            transaction
                .execute(
                    "INSERT INTO actors (id, kind, handle, display_name, created_at_ms)
                         VALUES (?1, 'agent', 'legacy-agent', 'Legacy Agent', 1)",
                    [agent_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO agents
                         (actor_id, workspace_path, lifecycle, created_at_ms, updated_at_ms)
                         VALUES (?1, '/tmp/legacy-agent', 'active', 1, 1)",
                    [agent_id],
                )
                .await?;
            transaction
                .execute(
                    "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '5')",
                    (),
                )
                .await?;
            transaction.commit().await?;
        }

        let core = CollabCore::open(&path).await?;
        let profile = core.agent_profile(agent_id).await?;
        assert_eq!(profile.version, 1);
        assert_eq!(profile.charter, AgentCharter::default());
        let connection = core.connection.lock().await;
        let mut rows = connection
            .query(
                "SELECT value FROM collab_meta WHERE key = 'schema_version'",
                (),
            )
            .await?;
        assert_eq!(
            rows.next().await?.expect("schema row").get::<String>(0)?,
            SCHEMA_VERSION.to_string()
        );
        Ok(())
    }

    #[tokio::test]
    async fn schema_v1_file_upgrades_to_unique_session_bindings() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("v1.db");
        {
            let database =
                turso::Builder::new_local(path.to_str().ok_or_else(|| {
                    CollabError::Filesystem("temporary path is not UTF-8".into())
                })?)
                .build()
                .await?;
            let mut connection = database.connect()?;
            connection.execute_batch(META_SCHEMA).await?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?;
            transaction.execute_batch(SCHEMA_V1).await?;
            transaction
                .execute(
                    "INSERT INTO collab_meta (key, value) VALUES ('schema_version', '1')",
                    (),
                )
                .await?;
            transaction.commit().await?;
        }

        let core = CollabCore::open(&path).await?;
        let alpha = core
            .create_agent("v1-alpha", "Alpha", "/tmp/v1-alpha")
            .await?;
        let beta = core.create_agent("v1-beta", "Beta", "/tmp/v1-beta").await?;
        core.bind_runtime(&alpha.id, "unique-session", "openai", "codex", "default")
            .await?;
        assert!(
            core.bind_runtime(&beta.id, "unique-session", "openai", "codex", "default")
                .await
                .is_err()
        );
        let connection = core.connection.lock().await;
        let mut rows = connection
            .query(
                "SELECT value FROM collab_meta WHERE key = 'schema_version'",
                (),
            )
            .await?;
        assert_eq!(
            rows.next().await?.expect("schema row").get::<String>(0)?,
            SCHEMA_VERSION.to_string()
        );
        Ok(())
    }
}
