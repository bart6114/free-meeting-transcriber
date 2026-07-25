use std::path::Path;

use hypr_db_core::{Db, DbOpenError, DbOpenOptions, DbStorage};
use hypr_db_execute::{DbExecutor, ProxyQueryMethod, ProxyQueryResult};
use hypr_db_reactive::{LiveQueryRuntime, QueryEventSink, SubscriptionRegistration};
use tauri::ipc::Channel;

use crate::{QueryEvent, Result, TransactionStatement};

#[derive(Clone)]
pub struct QueryEventChannel(Channel<QueryEvent>);

impl QueryEventChannel {
    pub fn new(channel: Channel<QueryEvent>) -> Self {
        Self(channel)
    }
}

impl QueryEventSink for QueryEventChannel {
    fn send_result(&self, rows: Vec<serde_json::Value>) -> std::result::Result<(), String> {
        self.0
            .send(QueryEvent::Result(rows))
            .map_err(|error| error.to_string())
    }

    fn send_error(&self, error: String) -> std::result::Result<(), String> {
        self.0
            .send(QueryEvent::Error(error))
            .map_err(|error| error.to_string())
    }
}

pub struct PluginDbRuntime {
    db: std::sync::Arc<Db>,
    schema_ready: tokio::sync::OnceCell<()>,
    executor: DbExecutor,
    live_query_runtime: LiveQueryRuntime<QueryEventChannel>,
}

impl PluginDbRuntime {
    pub fn new(db: std::sync::Arc<Db>) -> Self {
        Self {
            db: std::sync::Arc::clone(&db),
            schema_ready: tokio::sync::OnceCell::new(),
            executor: DbExecutor::new(std::sync::Arc::clone(&db)),
            live_query_runtime: LiveQueryRuntime::new(db),
        }
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.db.pool()
    }

    async fn ensure_app_schema(&self) -> Result<()> {
        self.schema_ready
            .get_or_try_init(|| async { hypr_db_app::prepare_schema(self.db.as_ref()).await })
            .await?;
        Ok(())
    }

    pub async fn execute(
        &self,
        sql: String,
        params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        self.ensure_app_schema().await?;
        Ok(self.executor.execute(sql, params).await?)
    }

    pub async fn execute_transaction(
        &self,
        statements: Vec<TransactionStatement>,
    ) -> Result<Vec<u64>> {
        self.ensure_app_schema().await?;
        let mut transaction = self.db.pool().begin_with("BEGIN IMMEDIATE").await?;
        let mut rows_affected = Vec::with_capacity(statements.len());

        for (statement_index, statement) in statements.into_iter().enumerate() {
            let result = bind_params(
                sqlx::query(sqlx::AssertSqlSafe(statement.sql.as_str())),
                &statement.params,
            )
            .execute(&mut *transaction)
            .await?;
            let actual = result.rows_affected();
            if let Some(expected) = statement.expected_rows_affected
                && actual != expected
            {
                return Err(crate::Error::UnexpectedRowsAffected {
                    statement_index,
                    expected,
                    actual,
                });
            }
            rows_affected.push(actual);
        }

        transaction.commit().await?;
        Ok(rows_affected)
    }

    pub async fn execute_proxy(
        &self,
        sql: String,
        params: Vec<serde_json::Value>,
        method: ProxyQueryMethod,
    ) -> Result<ProxyQueryResult> {
        self.ensure_app_schema().await?;
        Ok(self.executor.execute_proxy(sql, params, method).await?)
    }

    pub async fn subscribe(
        &self,
        sql: String,
        params: Vec<serde_json::Value>,
        sink: QueryEventChannel,
    ) -> Result<SubscriptionRegistration> {
        self.ensure_app_schema().await?;
        Ok(self.live_query_runtime.subscribe(sql, params, sink).await?)
    }

    pub async fn unsubscribe(&self, subscription_id: &str) -> hypr_db_reactive::Result<()> {
        self.live_query_runtime.unsubscribe(subscription_id).await
    }
}

fn bind_params<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    params: &[serde_json::Value],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    for param in params {
        query = match param {
            serde_json::Value::Null => query.bind(None::<String>),
            serde_json::Value::Bool(value) => query.bind(*value),
            serde_json::Value::Number(value) => {
                if let Some(integer) = value.as_i64() {
                    query.bind(integer)
                } else {
                    query.bind(value.as_f64().unwrap_or_default())
                }
            }
            serde_json::Value::String(value) => query.bind(value.clone()),
            other => query.bind(other.to_string()),
        };
    }

    query
}

pub async fn open_app_db(db_path: Option<&Path>) -> Result<Db> {
    let storage = match db_path {
        Some(path) => DbStorage::Local(path),
        None => DbStorage::Memory,
    };

    match Db::open(app_db_open_options(storage, true)).await {
        Ok(db) => {
            hypr_db_app::prepare_schema(&db).await?;
            Ok(db)
        }
        Err(cloudsync_error) => {
            let probe_error = match probe_cloudsync_extension().await {
                Ok(()) => return Err(cloudsync_error.into()),
                Err(error) => error,
            };
            open_app_db_without_cloudsync(storage, cloudsync_error, probe_error).await
        }
    }
}

fn app_db_open_options(storage: DbStorage<'_>, cloudsync_enabled: bool) -> DbOpenOptions<'_> {
    DbOpenOptions {
        storage,
        cloudsync_enabled,
        journal_mode_wal: true,
        foreign_keys: true,
        max_connections: Some(4),
    }
}

async fn probe_cloudsync_extension() -> std::result::Result<(), DbOpenError> {
    let db = Db::open(app_db_open_options(DbStorage::Memory, true)).await?;
    db.pool().close().await;
    Ok(())
}

async fn open_app_db_without_cloudsync(
    storage: DbStorage<'_>,
    cloudsync_error: DbOpenError,
    probe_error: DbOpenError,
) -> Result<Db> {
    let db = Db::open(app_db_open_options(storage, false)).await?;
    if database_uses_cloudsync_schema(&db).await? {
        db.pool().close().await;
        tracing::error!(
            %cloudsync_error,
            %probe_error,
            "cloudsync extension is unavailable for an initialized local replica"
        );
        return Err(cloudsync_error.into());
    }

    if let Err(error) = hypr_db_app::prepare_schema(&db).await {
        db.pool().close().await;
        return Err(error.into());
    }

    tracing::warn!(
        %cloudsync_error,
        %probe_error,
        "cloudsync extension is unavailable; opened the app database in local-only mode"
    );
    Ok(db)
}

async fn database_uses_cloudsync_schema(db: &Db) -> std::result::Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE (type = 'table' AND name = 'cloudsync_table_settings')
               OR (type = 'trigger' AND instr(lower(COALESCE(sql, '')), 'cloudsync_') > 0)
        )",
    )
    .fetch_one(db.pool())
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable_extension_error() -> DbOpenError {
        DbOpenError::Io(std::io::Error::other("cloudsync extension unavailable"))
    }

    fn failed_extension_probe_error() -> DbOpenError {
        DbOpenError::Io(std::io::Error::other("cloudsync extension probe failed"))
    }

    #[tokio::test]
    async fn cloudsync_open_failure_falls_back_for_uninitialized_database() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("app.db");

        let db = open_app_db_without_cloudsync(
            DbStorage::Local(&db_path),
            unavailable_extension_error(),
            failed_extension_probe_error(),
        )
        .await
        .unwrap();

        assert!(!db.cloudsync_enabled());
        let sessions_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sessions'
            )",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(sessions_exists);
    }

    #[cfg(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_env = "gnu", target_arch = "aarch64"),
        all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"),
        all(target_os = "linux", target_env = "musl", target_arch = "aarch64"),
        all(target_os = "linux", target_env = "musl", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
    ))]
    #[tokio::test]
    async fn extension_open_without_initialized_tables_allows_plain_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("app.db");
        let db = Db::open(DbOpenOptions {
            storage: DbStorage::Local(&db_path),
            cloudsync_enabled: true,
            journal_mode_wal: true,
            foreign_keys: true,
            max_connections: Some(1),
        })
        .await
        .unwrap();
        db.pool().close().await;
        drop(db);

        let db = open_app_db_without_cloudsync(
            DbStorage::Local(&db_path),
            unavailable_extension_error(),
            failed_extension_probe_error(),
        )
        .await
        .unwrap();

        assert!(!db.cloudsync_enabled());
        assert!(!database_uses_cloudsync_schema(&db).await.unwrap());
    }

    #[cfg(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_env = "gnu", target_arch = "aarch64"),
        all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"),
        all(target_os = "linux", target_env = "musl", target_arch = "aarch64"),
        all(target_os = "linux", target_env = "musl", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
    ))]
    #[tokio::test]
    async fn cloudsync_open_failure_does_not_migrate_initialized_replica_plainly() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("app.db");
        let db = Db::open(DbOpenOptions {
            storage: DbStorage::Local(&db_path),
            cloudsync_enabled: true,
            journal_mode_wal: true,
            foreign_keys: true,
            max_connections: Some(1),
        })
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE items (
                id TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL DEFAULT ''
            )",
        )
        .execute(db.pool())
        .await
        .unwrap();
        db.cloudsync_init("items", None, None).await.unwrap();
        db.pool().close().await;
        drop(db);

        let error = open_app_db_without_cloudsync(
            DbStorage::Local(&db_path),
            unavailable_extension_error(),
            failed_extension_probe_error(),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, crate::Error::Db(DbOpenError::Io(_))));
        let plain = Db::connect_local_plain(&db_path).await.unwrap();
        let sessions_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sessions'
            )",
        )
        .fetch_one(plain.pool())
        .await
        .unwrap();
        assert!(!sessions_exists);
    }
}
