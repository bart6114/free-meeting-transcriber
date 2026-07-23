use std::sync::Arc;

use hypr_db_core::Db;

const DB_FILENAME: &str = "app.db";

pub async fn open_desktop_db(identifier: &str) -> Arc<Db> {
    let db_path = desktop_db_dir(identifier).map(|dir| {
        std::fs::create_dir_all(&dir).expect("failed to create app data dir");
        dir.join(DB_FILENAME)
    });

    let db = tauri_plugin_db::open_app_db(db_path.as_deref())
        .await
        .expect("failed to open app database");

    Arc::new(db)
}

/// CloudSync is permanently disabled in this fork: there is no server to sync
/// against, so this always returns `None`. Signature is kept so `lib.rs`
/// continues to compile unchanged and the CloudSync/E2EE commands it wires up
/// remain registered (they just report "not configured").
pub fn cloudsync_runtime_config_from_env()
-> Result<Option<hypr_db_core::CloudsyncRuntimeConfig>, String> {
    Ok(None)
}

fn desktop_db_dir(identifier: &str) -> Option<std::path::PathBuf> {
    let data_dir = dirs::data_dir().expect("data_dir must be available");
    let default_dir =
        hypr_storage::global::compute_default_base(identifier).expect("data_dir must be available");
    let identifier_dir = data_dir.join(identifier);

    if identifier_dir.join(DB_FILENAME).is_file() && !default_dir.join(DB_FILENAME).is_file() {
        Some(identifier_dir)
    } else {
        Some(default_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn dev_uses_an_isolated_persistent_database() {
        let db_dir = desktop_db_dir("org.freemeetingtranscriber.dev").unwrap();

        assert!(db_dir.ends_with("org.freemeetingtranscriber.dev"));
    }

    // Serializes mutation of the ANARLOG_CLOUDSYNC_* env vars below, since
    // `std::env::set_var`/`remove_var` mutate whole-process state and Rust
    // runs tests in parallel by default. Mirrors the `ENV_LOCK` pattern used
    // for the same reason in crates/codex/src/health.rs, crates/claude/src/health.rs,
    // crates/amp/src/health.rs, and crates/storage/src/vault/path.rs.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// The full set of env vars the pre-CloudSync-off `cloudsync_runtime_config`
    /// (deleted; see git history at 71f4bea28) used to honor, in the order it
    /// read them: an explicit opt-in flag, then the database id / credential
    /// pair, then the sync interval override.
    const LEGACY_CLOUDSYNC_ENV_VARS: &[(&str, &str)] = &[
        ("ANARLOG_CLOUDSYNC_ALLOW_STATIC_AUTH", "true"),
        ("ANARLOG_CLOUDSYNC_E2EE_DATABASE_ID", "managed-database-id"),
        ("ANARLOG_CLOUDSYNC_API_KEY", "api-key"),
        ("ANARLOG_CLOUDSYNC_TOKEN", "token"),
        ("ANARLOG_CLOUDSYNC_INTERVAL_MS", "15000"),
    ];

    #[test]
    fn cloudsync_is_always_off() {
        let _guard = ENV_LOCK.lock().expect("env lock");

        // A fully-populated, previously-valid CloudSync environment (the
        // exact combination the deleted `cloudsync_environment_config_enables_only_core_tables`
        // test asserted would enable CloudSync) must still fall through to
        // `None` unconditionally now: CloudSync is hardwired off regardless
        // of ambient env, not merely "off because nothing is set".
        let prev: Vec<(&str, Option<String>)> = LEGACY_CLOUDSYNC_ENV_VARS
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();

        for (key, value) in LEGACY_CLOUDSYNC_ENV_VARS {
            // SAFETY: env mutation is serialized by ENV_LOCK above.
            unsafe { std::env::set_var(key, value) };
        }

        let config = cloudsync_runtime_config_from_env().unwrap();

        for (key, prev_value) in prev {
            // SAFETY: env mutation is serialized by ENV_LOCK above.
            unsafe {
                match prev_value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }

        assert!(config.is_none());
    }
}
