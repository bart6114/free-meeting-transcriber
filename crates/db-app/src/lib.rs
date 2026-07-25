#![forbid(unsafe_code)]

mod session_ops;
mod session_types;
mod template_ops;
mod template_types;

pub use session_ops::*;
pub use session_types::*;
pub use template_ops::*;
pub use template_types::*;

pub const APP_MIGRATION_STEPS: &[hypr_db_migrate::MigrationStep] = &[
    hypr_db_migrate::MigrationStep {
        id: "20260413020000_templates",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260413020000_templates.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260414120000_calendars_events",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260414120000_calendars_events.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260524000000_default_templates",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260524000000_default_templates.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260624000000_repair_templates",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260624000000_repair_templates.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260710223922_canonical_data_model",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260710223922_canonical_data_model.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260710231809_import_target_audit",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260710231809_import_target_audit.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260711000000_calendar_event_tombstones",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260711000000_calendar_event_tombstones.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260712170000_template_icons",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260712170000_template_icons.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260713164500_repair_empty_session_titles",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260713164500_repair_empty_session_titles.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260714120000_search_index_queue",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260714120000_search_index_queue.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260714120100_search_index_sessions_triggers",
        // Was `CloudsyncAlter { table_name: "sessions" }`; downgraded to
        // `Plain` in Task 4, which removes CloudSync/E2EE and the multi-tenant
        // ownership layer built on top of it entirely (see the
        // `20260723150000_vault_export_dirty` step's comment:
        // CloudSync was permanently disabled in this fork even before this
        // task, so `CloudsyncAlter`'s `apply()` branch always fell through to
        // the same plain-apply path anyway). `checksum` is computed from
        // `sql` alone, so this doesn't affect already-applied installs.
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260714120100_search_index_sessions_triggers.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260714120200_search_index_session_documents_triggers",
        // See the `20260714120100_search_index_sessions_triggers` step above.
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!(
            "../migrations/20260714120200_search_index_session_documents_triggers.sql"
        ),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260714120300_search_index_transcripts_triggers",
        // See the `20260714120100_search_index_sessions_triggers` step above.
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260714120300_search_index_transcripts_triggers.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260714120400_search_index_humans_triggers",
        // Was `CloudsyncAlter { table_name: "humans" }`; downgraded to `Plain`
        // once Task 3 dropped `humans` from `E2EE_DOMAIN_TABLES` (the table
        // itself is dropped a few migrations later, in
        // `20260724100000_drop_calendar_humans`). Behaviorally identical
        // either way — see the `20260723150000_vault_export_dirty` step's
        // comment: CloudSync is permanently disabled in this fork, so
        // `CloudsyncAlter`'s `apply()` branch always fell through to the same
        // plain-apply path anyway. `checksum` is computed from `sql` alone,
        // so this doesn't affect already-applied installs.
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260714120400_search_index_humans_triggers.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260714120500_search_index_organizations_triggers",
        // See the `20260714120400_search_index_humans_triggers` step above.
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260714120500_search_index_organizations_triggers.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260716120000_personal_workspaces",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260716120000_personal_workspaces.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260716130000_cloudsync_session_evictions",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260716130000_cloudsync_session_evictions.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260716173000_shared_session_cache",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260716173000_shared_session_cache.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717120000_e2ee_replica",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717120000_e2ee_replica.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717140000_attachment_local_state",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717140000_attachment_local_state.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717192000_e2ee_replica_order",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717192000_e2ee_replica_order.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717193000_e2ee_freshness_witness",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717193000_e2ee_freshness_witness.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717150000_attachment_transfer_jobs",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717150000_attachment_transfer_jobs.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717170000_attachment_cloud_sync_intent",
        // See the `20260714120100_search_index_sessions_triggers` step above.
        // `session_attachments` is dropped entirely by
        // `20260724110000_drop_cloud_tables` a few steps down; downgrading
        // this step's scope doesn't change that, it just stops requiring the
        // (now-deleted) CloudSync alter-guard for a table on its way out.
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717170000_attachment_cloud_sync_intent.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717171000_shared_session_attachment_cache",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717171000_shared_session_attachment_cache.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717172000_shared_session_cache_attachments",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717172000_shared_session_cache_attachments.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717190000_shared_session_cache_web_edits",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717190000_shared_session_cache_web_edits.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260717191000_session_share_sync_state",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260717191000_session_share_sync_state.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260723150000_vault_export_dirty",
        // `Plain`, not `CloudsyncAlter`, even though some altered tables
        // (sessions, session_documents, transcripts, humans, organizations,
        // session_participants, action_items) used to be considered part of
        // the CloudSync-replicated domain: CloudSync was permanently
        // disabled in this fork even before Task 4 removed it outright, so
        // the `CloudsyncAlter` branch in `db-migrate`'s `apply()` always fell
        // through to the same plain-apply path anyway. `CloudsyncAlter` also
        // names exactly one table per step, which doesn't fit one migration
        // spanning fourteen tables. `validate_step` only enforces the
        // alter-table guard when a step opts into `CloudsyncAlter`, so
        // `Plain` here is unconditionally valid.
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260723150000_vault_export_dirty.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260724100000_drop_calendar_humans",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260724100000_drop_calendar_humans.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260724110000_drop_cloud_tables",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260724110000_drop_cloud_tables.sql"),
    },
    hypr_db_migrate::MigrationStep {
        id: "20260725120000_drop_sync_machinery",
        scope: hypr_db_migrate::MigrationScope::Plain,
        sql: include_str!("../migrations/20260725120000_drop_sync_machinery.sql"),
    },
];

/// No migration step opts into `MigrationScope::CloudsyncAlter` anymore —
/// Task 4 removed CloudSync entirely, downgrading every remaining
/// `CloudsyncAlter` step to `Plain` (see the step comments above). Kept as a
/// named function, rather than an inline closure, only because
/// `hypr_db_migrate::DbSchema::validate_cloudsync_table` is a plain `fn`
/// pointer, not a `Box<dyn Fn>`.
fn alter_guard_required(_table_name: &str) -> bool {
    false
}

pub fn schema() -> hypr_db_migrate::DbSchema {
    hypr_db_migrate::DbSchema {
        steps: APP_MIGRATION_STEPS,
        validate_cloudsync_table: alter_guard_required,
    }
}

#[derive(Debug)]
pub enum AppSchemaError {
    Migrate(hypr_db_migrate::MigrateError),
    Sqlx(sqlx::Error),
}

impl std::fmt::Display for AppSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Migrate(error) => write!(f, "{error}"),
            Self::Sqlx(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AppSchemaError {}

impl From<hypr_db_migrate::MigrateError> for AppSchemaError {
    fn from(error: hypr_db_migrate::MigrateError) -> Self {
        Self::Migrate(error)
    }
}

impl From<sqlx::Error> for AppSchemaError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}

pub async fn prepare_schema(db: &hypr_db_core::Db) -> Result<(), AppSchemaError> {
    let templates_missing_before_migration = !templates_table_exists(db.pool()).await?;
    hypr_db_migrate::migrate(db, schema()).await?;
    repair_missing_core_tables(db.pool(), templates_missing_before_migration).await?;
    Ok(())
}

async fn templates_table_exists(pool: &sqlx::SqlitePool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = 'templates'
        )",
    )
    .fetch_one(pool)
    .await
}

async fn repair_missing_core_tables(
    pool: &sqlx::SqlitePool,
    templates_missing_before_migration: bool,
) -> Result<(), sqlx::Error> {
    if !templates_table_exists(pool).await? {
        sqlx::query(include_str!("../migrations/20260413020000_templates.sql"))
            .execute(pool)
            .await?;
    }

    let has_icon_json = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('templates') WHERE name = 'icon_json')",
    )
    .fetch_one(pool)
    .await?;
    if !has_icon_json {
        sqlx::query(include_str!(
            "../migrations/20260712170000_template_icons.sql"
        ))
        .execute(pool)
        .await?;
    }

    if templates_missing_before_migration {
        sqlx::query(include_str!(
            "../migrations/20260524000000_default_templates.sql"
        ))
        .execute(pool)
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hypr_db_core::Db;

    async fn test_db() -> Db {
        let db = Db::open(hypr_db_core::DbOpenOptions {
            storage: hypr_db_core::DbStorage::Memory,
            cloudsync_enabled: false,
            journal_mode_wal: true,
            foreign_keys: true,
            max_connections: Some(1),
        })
        .await
        .unwrap();
        prepare_schema(&db).await.unwrap();
        db
    }

    fn migration_steps_before(id: &str) -> &'static [hypr_db_migrate::MigrationStep] {
        let index = APP_MIGRATION_STEPS
            .iter()
            .position(|step| step.id == id)
            .unwrap();
        &APP_MIGRATION_STEPS[..index]
    }

    async fn test_db_without_default_templates() -> Db {
        let db = Db::open(hypr_db_core::DbOpenOptions {
            storage: hypr_db_core::DbStorage::Memory,
            cloudsync_enabled: false,
            journal_mode_wal: true,
            foreign_keys: true,
            max_connections: Some(1),
        })
        .await
        .unwrap();
        hypr_db_migrate::migrate(
            &db,
            hypr_db_migrate::DbSchema {
                steps: &APP_MIGRATION_STEPS[..2],
                validate_cloudsync_table: alter_guard_required,
            },
        )
        .await
        .unwrap();
        sqlx::query(include_str!(
            "../migrations/20260712170000_template_icons.sql"
        ))
        .execute(db.pool())
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn schema_declares_core_tables() {
        let db = test_db().await;

        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();

        assert!(tables.contains(&"_sqlx_migrations".to_string()));
        assert!(tables.contains(&"templates".to_string()));
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"transcripts".to_string()));
    }

    #[tokio::test]
    async fn migrations_apply_cleanly() {
        let db = test_db().await;

        let tables: Vec<String> = sqlx::query_as::<_, (String,)>(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(db.pool())
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.0)
        .collect();

        assert_eq!(
            tables,
            vec![
                "_sqlx_migrations",
                "action_items",
                "app_settings",
                "chat_groups",
                "chat_messages",
                "daily_notes",
                "entity_mentions",
                "search_index_dirty",
                "search_index_state",
                "session_documents",
                "session_tags",
                "sessions",
                "tags",
                "templates",
                "transcripts",
            ]
        );
    }

    #[tokio::test]
    async fn migration_repairs_empty_titles_from_summary_headings() {
        let db = Db::connect_memory_plain().await.unwrap();
        hypr_db_migrate::migrate(
            &db,
            hypr_db_migrate::DbSchema {
                steps: migration_steps_before("20260713164500_repair_empty_session_titles"),
                validate_cloudsync_table: alter_guard_required,
            },
        )
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO sessions (id, title)
             VALUES ('json', ''), ('markdown', '   '), ('generic', ''), ('existing', 'Keep Me')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO session_documents
             (id, session_id, kind, body_format, body, sort_order)
             VALUES
             ('json-summary', 'json', 'summary', 'prosemirror_json',
              '{\"type\":\"doc\",\"content\":[{\"type\":\"heading\",\"attrs\":{\"level\":1},\"content\":[{\"type\":\"text\",\"text\":\"Transcript Test \"},{\"type\":\"text\",\"text\":\"Utterances\"}]}]}', 0),
             ('markdown-summary', 'markdown', 'summary', 'markdown',
              char(10) || '# Markdown Title' || char(10) || char(10) || 'Details', 0),
             ('generic-summary', 'generic', 'summary', 'markdown', '# Summary' || char(10) || 'Details', 0),
             ('existing-summary', 'existing', 'summary', 'markdown', '# Replacement' || char(10) || 'Details', 0)",
        )
        .execute(db.pool())
        .await
        .unwrap();

        hypr_db_migrate::migrate(&db, schema()).await.unwrap();

        let titles =
            sqlx::query_as::<_, (String, String)>("SELECT id, title FROM sessions ORDER BY id")
                .fetch_all(db.pool())
                .await
                .unwrap()
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(titles["json"], "Transcript Test Utterances");
        assert_eq!(titles["markdown"], "Markdown Title");
        assert_eq!(titles["generic"], "");
        assert_eq!(titles["existing"], "Keep Me");
    }

    #[tokio::test]
    async fn repair_migration_recreates_missing_templates_table() {
        let db = Db::connect_memory_plain().await.unwrap();
        hypr_db_migrate::migrate(
            &db,
            hypr_db_migrate::DbSchema {
                steps: &APP_MIGRATION_STEPS[..3],
                validate_cloudsync_table: alter_guard_required,
            },
        )
        .await
        .unwrap();

        sqlx::query("DROP TABLE templates")
            .execute(db.pool())
            .await
            .unwrap();

        hypr_db_migrate::migrate(&db, schema()).await.unwrap();

        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(row_count, 0);
    }

    #[tokio::test]
    async fn prepare_schema_recreates_templates_after_repair_migration_was_already_applied() {
        let db = test_db().await;

        sqlx::query("DROP TABLE templates")
            .execute(db.pool())
            .await
            .unwrap();

        prepare_schema(&db).await.unwrap();

        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(row_count > 0);

        let icon_json: String =
            sqlx::query_scalar("SELECT icon_json FROM templates ORDER BY id LIMIT 1")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(
            icon_json,
            r##"{"type":"icon","value":"notebook-tabs","color":"#9ca3af"}"##
        );
    }

    #[tokio::test]
    async fn prepare_schema_seeds_templates_when_repair_migration_creates_missing_table() {
        let db = Db::connect_memory_plain().await.unwrap();
        hypr_db_migrate::migrate(
            &db,
            hypr_db_migrate::DbSchema {
                steps: &APP_MIGRATION_STEPS[..3],
                validate_cloudsync_table: alter_guard_required,
            },
        )
        .await
        .unwrap();

        sqlx::query("DROP TABLE templates")
            .execute(db.pool())
            .await
            .unwrap();

        prepare_schema(&db).await.unwrap();

        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM templates")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(row_count > 0);
    }

    #[test]
    fn search_index_trigger_migrations_are_plain() {
        // CloudSync is gone (Task 4): every search-index trigger migration —
        // including the ones that used to opt into `CloudsyncAlter` for
        // sessions/session_documents/transcripts — is `Plain` now. See the
        // `20260714120100_search_index_sessions_triggers` step's comment in
        // `APP_MIGRATION_STEPS` for why downgrading the scope doesn't affect
        // already-applied installs.
        for id in [
            "20260714120000_search_index_queue",
            "20260714120100_search_index_sessions_triggers",
            "20260714120200_search_index_session_documents_triggers",
            "20260714120300_search_index_transcripts_triggers",
            "20260714120400_search_index_humans_triggers",
            "20260714120500_search_index_organizations_triggers",
        ] {
            let step = APP_MIGRATION_STEPS
                .iter()
                .find(|step| step.id == id)
                .unwrap();
            assert_eq!(step.scope, hypr_db_migrate::MigrationScope::Plain);
        }
    }

    #[tokio::test]
    async fn search_index_queue_coalesces_changes_and_tracks_session_moves() {
        let db = test_db().await;

        sqlx::query(
            "INSERT INTO sessions (id, title) VALUES ('session-1', 'One'), ('session-2', 'Two')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query("DELETE FROM search_index_dirty")
            .execute(db.pool())
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO session_documents (id, session_id, body) VALUES ('document-1', 'session-1', 'one')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE session_documents SET body = 'two' WHERE id = 'document-1'")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE session_documents SET session_id = 'session-2' WHERE id = 'document-1'",
        )
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO transcripts (id, session_id, words_json) VALUES ('transcript-1', 'session-1', '[]')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE transcripts SET session_id = 'session-2' WHERE id = 'transcript-1'")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM transcripts WHERE id = 'transcript-1'")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM session_documents WHERE id = 'document-1'")
            .execute(db.pool())
            .await
            .unwrap();

        let rows = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT entity_type, entity_id, generation
             FROM search_index_dirty
             ORDER BY entity_type, entity_id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();

        assert_eq!(
            rows,
            vec![
                ("session".to_string(), "session-1".to_string(), 5),
                ("session".to_string(), "session-2".to_string(), 4),
            ]
        );
    }

    #[tokio::test]
    async fn search_index_queue_tracks_entity_lifecycle_and_starts_unversioned() {
        let db = test_db().await;

        let projection_version: i64 = sqlx::query_scalar(
            "SELECT projection_version FROM search_index_state WHERE id = 'default'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(projection_version, 0);

        sqlx::query(
            "INSERT INTO sessions (id, title) VALUES ('session-1', 'One'), ('session-2', 'Two')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query("UPDATE sessions SET ended_at = '2026-07-14T00:00:00Z' WHERE id = 'session-1'")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET title = 'Updated' WHERE id = 'session-2'")
            .execute(db.pool())
            .await
            .unwrap();

        let rows = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT entity_type, entity_id, generation
             FROM search_index_dirty
             ORDER BY entity_type, entity_id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();

        assert_eq!(
            rows,
            vec![
                ("session".to_string(), "session-1".to_string(), 2),
                ("session".to_string(), "session-2".to_string(), 2),
            ]
        );
    }

    #[tokio::test]
    async fn drop_sync_machinery_migration_preserves_live_rows() {
        let db = Db::connect_memory_plain().await.unwrap();
        hypr_db_migrate::migrate(
            &db,
            hypr_db_migrate::DbSchema {
                steps: migration_steps_before("20260725120000_drop_sync_machinery"),
                validate_cloudsync_table: alter_guard_required,
            },
        )
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO sessions
             (id, workspace_id, owner_user_id, title, kind, status, created_at, updated_at,
              started_at, ended_at, timezone, language, event_id, external_event_id,
              external_provider, series_id, source_apps_json, event_json, folder_path, slug,
              metadata_json, deleted_at)
             VALUES
             ('live', 'ws-1', 'owner-1', 'Live session', 'meeting', 'active',
              '2026-07-01T00:00:00Z', '2026-07-02T00:00:00Z', '2026-07-01T09:00:00Z',
              '2026-07-01T10:00:00Z', 'Europe/Brussels', 'en', 'event-1', 'ext-event-1',
              'zoom', 'series-1', '[{\"app\":\"zoom\"}]', '{\"tracking_id\":\"t-1\"}',
              'folder/a', 'live-session', '{\"source\":\"test\"}', NULL),
             ('hidden', '', '', 'Soft-hidden session', 'meeting', 'active',
              '2026-07-01T00:00:00Z', '2026-07-02T00:00:00Z', '', '', '', '', '', '',
              '', '', '[]', '', '', '', '{}', '2026-07-03T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO session_documents
             (id, session_id, kind, title, template_id, body_format, body, source_hash,
              generation_metadata_json, sort_order, created_by, updated_by, updated_at,
              deleted_at)
             VALUES
             ('live:key_facts', 'live', 'key_facts', '', '', 'md', 'facts', 'hash-1', '{}',
              0, '', '', '2026-07-02T00:00:00Z', NULL),
             ('live:meeting-chat:abc', 'live', 'meeting_chat', '', '', 'md', 'chat', 'hash-2',
              '{}', 0, '', '', '2026-07-02T00:00:00Z', NULL),
             ('summary-1', 'live', 'summary', 'Summary', 'template-1', 'prosemirror_json',
              'summary body', '', '{}', 3, 'owner-1', 'owner-1', '2026-07-02T00:00:00Z',
              '2026-07-04T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transcripts
             (id, session_id, owner_user_id, started_at_ms, ended_at_ms, memo, words_json,
              speaker_hints_json, updated_at, deleted_at)
             VALUES
             ('transcript-live', 'live', 'owner-1', 100, 200, 'memo',
              '[{\"text\":\"hello\"}]', '[{\"type\":\"speaker\"}]', '2026-07-02T00:00:00Z',
              NULL),
             ('transcript-superseded', 'live', '', 0, NULL, '', '[]', '[]',
              '2026-07-02T00:00:00Z', '2026-07-05T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        hypr_db_migrate::migrate(&db, schema()).await.unwrap();

        let session_identity = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
            ),
        >(
            "SELECT id, owner_user_id, title, kind, status, created_at, updated_at,
                    started_at, ended_at
             FROM sessions ORDER BY id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(
            session_identity,
            vec![(
                "live".to_string(),
                "owner-1".to_string(),
                "Live session".to_string(),
                "meeting".to_string(),
                "active".to_string(),
                "2026-07-01T00:00:00Z".to_string(),
                "2026-07-02T00:00:00Z".to_string(),
                "2026-07-01T09:00:00Z".to_string(),
                "2026-07-01T10:00:00Z".to_string(),
            )]
        );

        let session_details = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
            ),
        >(
            "SELECT timezone, language, external_provider, source_apps_json,
                    event_json, folder_path, slug, metadata_json
             FROM sessions WHERE id = 'live'",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(
            session_details,
            vec![(
                "Europe/Brussels".to_string(),
                "en".to_string(),
                "zoom".to_string(),
                "[{\"app\":\"zoom\"}]".to_string(),
                "{\"tracking_id\":\"t-1\"}".to_string(),
                "folder/a".to_string(),
                "live-session".to_string(),
                "{\"source\":\"test\"}".to_string(),
            )]
        );

        let documents = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
                String,
                String,
                String,
                Option<String>,
            ),
        >(
            "SELECT id, session_id, kind, title, template_id, body_format, body,
                    sort_order, created_by, updated_by, updated_at, deleted_at
             FROM session_documents ORDER BY id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(
            documents,
            vec![(
                "summary-1".to_string(),
                "live".to_string(),
                "summary".to_string(),
                "Summary".to_string(),
                "template-1".to_string(),
                "prosemirror_json".to_string(),
                "summary body".to_string(),
                3,
                "owner-1".to_string(),
                "owner-1".to_string(),
                "2026-07-02T00:00:00Z".to_string(),
                Some("2026-07-04T00:00:00Z".to_string()),
            )]
        );

        let transcripts = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                i64,
                Option<i64>,
                String,
                String,
                String,
                String,
                Option<String>,
            ),
        >(
            "SELECT id, owner_user_id, session_id, started_at_ms, ended_at_ms, memo,
                    words_json, speaker_hints_json, updated_at, deleted_at
             FROM transcripts ORDER BY id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(
            transcripts,
            vec![
                (
                    "transcript-live".to_string(),
                    "owner-1".to_string(),
                    "live".to_string(),
                    100,
                    Some(200),
                    "memo".to_string(),
                    "[{\"text\":\"hello\"}]".to_string(),
                    "[{\"type\":\"speaker\"}]".to_string(),
                    "2026-07-02T00:00:00Z".to_string(),
                    None,
                ),
                (
                    "transcript-superseded".to_string(),
                    "".to_string(),
                    "live".to_string(),
                    0,
                    None,
                    "".to_string(),
                    "[]".to_string(),
                    "[]".to_string(),
                    "2026-07-02T00:00:00Z".to_string(),
                    Some("2026-07-05T00:00:00Z".to_string()),
                ),
            ]
        );

        // The stranded vault_export_* triggers must be gone: writes to the
        // surviving tables they were attached to would otherwise fail with
        // "no such table: vault_export_dirty".
        sqlx::query("INSERT INTO tags (id, name) VALUES ('tag-1', 'Tag')")
            .execute(db.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn template_roundtrip() {
        let db = test_db().await;

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "template-1",
                title: "Standup",
                description: "Daily sync",
                pinned: true,
                pin_order: Some(2),
                category: Some("meetings"),
                targets_json: Some("[\"engineering\"]"),
                sections_json: "[{\"title\":\"Notes\",\"description\":\"...\"}]",
            },
        )
        .await
        .unwrap();

        let row = get_template(db.pool(), "template-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.title, "Standup");
        assert_eq!(row.targets_json.as_deref(), Some("[\"engineering\"]"));
        assert_eq!(
            row.sections_json,
            "[{\"title\":\"Notes\",\"description\":\"...\"}]"
        );
    }

    #[tokio::test]
    async fn migrations_seed_default_templates_without_overwriting_existing_rows() {
        let db = Db::connect_memory_plain().await.unwrap();
        hypr_db_migrate::migrate(
            &db,
            hypr_db_migrate::DbSchema {
                steps: &APP_MIGRATION_STEPS[..1],
                validate_cloudsync_table: alter_guard_required,
            },
        )
        .await
        .unwrap();

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "default-daily-standup",
                title: "Custom Standup",
                description: "Keep user edit",
                pinned: true,
                pin_order: Some(1),
                category: Some("Custom"),
                targets_json: Some("[\"Team\"]"),
                sections_json: "[{\"title\":\"Custom\",\"description\":\"Keep\"}]",
            },
        )
        .await
        .unwrap();

        hypr_db_migrate::migrate(&db, schema()).await.unwrap();

        let rows = list_templates(db.pool()).await.unwrap();
        assert_eq!(rows.len(), 17);
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec![
                "default-board-meeting",
                "default-brainstorming-session",
                "default-client-kickoff",
                "default-customer-discovery",
                "default-daily-standup",
                "default-executive-briefing",
                "default-incident-postmortem",
                "default-investor-pitch",
                "default-lecture-notes",
                "default-one-on-one-meeting",
                "default-performance-review",
                "default-product-roadmap-review",
                "default-project-kickoff",
                "default-sales-discovery-call",
                "default-sprint-planning",
                "default-sprint-retrospective",
                "default-technical-design-review",
            ]
        );

        let custom_row = get_template(db.pool(), "default-daily-standup")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(custom_row.title, "Custom Standup");
        assert_eq!(custom_row.description, "Keep user edit");

        let seeded_row = get_template(db.pool(), "default-sales-discovery-call")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(seeded_row.title, "Sales Discovery Call");
        assert_eq!(
            seeded_row.targets_json.as_deref(),
            Some("[\"Account Executive\",\"Sales Rep\",\"BDR\"]")
        );
    }

    #[tokio::test]
    async fn list_templates_returns_all_ordered_by_id() {
        let db = test_db_without_default_templates().await;

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "template-2",
                title: "Two",
                description: "",
                pinned: false,
                pin_order: None,
                category: None,
                targets_json: None,
                sections_json: "[]",
            },
        )
        .await
        .unwrap();

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "template-1",
                title: "One",
                description: "",
                pinned: false,
                pin_order: None,
                category: None,
                targets_json: None,
                sections_json: "[]",
            },
        )
        .await
        .unwrap();

        let rows = list_templates(db.pool()).await.unwrap();
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();

        assert_eq!(ids, vec!["template-1", "template-2"]);
    }

    #[tokio::test]
    async fn template_upsert_replaces_existing_row_by_id() {
        let db = test_db_without_default_templates().await;

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "template-1",
                title: "First",
                description: "A",
                pinned: false,
                pin_order: None,
                category: None,
                targets_json: None,
                sections_json: "[]",
            },
        )
        .await
        .unwrap();

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "template-1",
                title: "Second",
                description: "B",
                pinned: true,
                pin_order: Some(5),
                category: Some("sales"),
                targets_json: Some("[\"exec\"]"),
                sections_json: "[{\"title\":\"Summary\",\"description\":\"Updated\"}]",
            },
        )
        .await
        .unwrap();

        let row = get_template(db.pool(), "template-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.title, "Second");
        assert_eq!(row.description, "B");
        assert!(row.pinned);
        assert_eq!(row.pin_order, Some(5));
        assert_eq!(row.category.as_deref(), Some("sales"));
        assert_eq!(row.targets_json.as_deref(), Some("[\"exec\"]"));
        assert_eq!(
            row.sections_json,
            "[{\"title\":\"Summary\",\"description\":\"Updated\"}]"
        );
        assert_eq!(list_templates(db.pool()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn template_delete_removes_row() {
        let db = test_db().await;

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "template-1",
                title: "Delete Me",
                description: "",
                pinned: false,
                pin_order: None,
                category: None,
                targets_json: None,
                sections_json: "[]",
            },
        )
        .await
        .unwrap();

        delete_template(db.pool(), "template-1").await.unwrap();

        assert!(
            get_template(db.pool(), "template-1")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn template_insert_if_missing_preserves_existing_row() {
        let db = test_db().await;

        upsert_template(
            db.pool(),
            UpsertTemplate {
                id: "template-1",
                title: "Original",
                description: "A",
                pinned: false,
                pin_order: None,
                category: None,
                targets_json: None,
                sections_json: "[]",
            },
        )
        .await
        .unwrap();

        let inserted = insert_template_if_missing(
            db.pool(),
            UpsertTemplate {
                id: "template-1",
                title: "Replacement",
                description: "B",
                pinned: true,
                pin_order: Some(4),
                category: Some("meetings"),
                targets_json: Some("[\"exec\"]"),
                sections_json: "[{\"title\":\"Summary\",\"description\":\"Updated\"}]",
            },
        )
        .await
        .unwrap();

        assert!(!inserted);

        let row = get_template(db.pool(), "template-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.title, "Original");
        assert_eq!(row.description, "A");
        assert!(!row.pinned);
        assert_eq!(row.pin_order, None);
        assert_eq!(row.category, None);
        assert_eq!(row.targets_json, None);
        assert_eq!(row.sections_json, "[]");
    }
}
