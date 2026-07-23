//! Write-through DB-to-vault file mirror (Task 13).
//!
//! Structured 1:1 on `search_index.rs`: subscribe to `db.change_notifier()`,
//! drain a dirty-queue table (`vault_export_dirty`, see the migration at
//! `crates/db-app/migrations/20260723150000_vault_export_dirty.sql`), project
//! rows to a target — here, vault files instead of a Tantivy index.
//!
//! # Loop-prevention analysis
//!
//! Two independent mechanisms exist and both are needed together:
//!
//! 1. **`notify.mark_own_writes(path)` before every write.** The vault
//!    watcher (`plugins/notify`) ignores any path marked within its own
//!    ~1.8s TTL window (`OWN_WRITES_TTL_MS` = 2x its 900ms debounce), so it
//!    never emits a `FileChanged` event for a file this worker just wrote —
//!    without this, every export write would look like an external edit and
//!    re-trigger `sync_from_vault`'s reconcile path.
//! 2. **Byte-identical skip in `write_file_atomic`.** Even if the mark
//!    somehow missed the window (a slow disk, a debounce race), re-rendering
//!    unchanged DB content produces byte-identical output, so the write is
//!    skipped outright — no new mtime, nothing for the watcher *or* a
//!    startup `sync_from_vault` pass to see as changed.
//!
//! Neither alone is sufficient: (1) only covers the live-watcher path, not a
//! reconcile that runs later (e.g. after a crash, before this worker's mark
//! TTL would still be active anyway, but conceptually the mark is
//! time-bounded while the identical-content check is not); (2) alone would
//! still cause a watcher event (and hence a wasted, self-correcting
//! reconcile pass) on every export, even one that changed nothing observable
//! to `sync_from_vault` (e.g. a key reordering). Together: exports that
//! change nothing produce no I/O and no events at all; exports that do
//! change content are pre-marked so the watcher stays quiet.
//!
//! # Startup ordering
//!
//! `sync_from_vault` (Task 12's reconcile) runs inside
//! `tauri_plugin_db::init_with_cloudsync`'s plugin `setup()` hook, which — by
//! Tauri's plugin lifecycle — completes before the *app*-level
//! `tauri::Builder::setup()` closure in `lib.rs` runs. `vault_export::spawn`
//! (like `search_index::spawn`) is called from that app-level closure, so
//! every reconcile-from-vault pass is guaranteed to finish before this worker
//! starts draining. Reconcile-first, then mirror.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sqlx::{Row, SqlitePool};
use tauri::{AppHandle, Manager};
use tauri_plugin_notify::NotifyPluginExt;
use tauri_plugin_settings::SettingsPluginExt;

use hypr_fs_sync_core::export;

/// Bump the marker file's contents if the vault file *layout* changes in a
/// way that requires re-exporting everything (mirrors search_index's
/// `PROJECTION_VERSION`). The marker's mere presence, not its value, is what
/// gates the one-time first-run export today.
const EXPORT_MARKER_VERSION: &str = "1";
/// `pub(crate)`: `vault_watch.rs` also needs this name to exclude the marker
/// file from what it hands to `import_paths` (it's a plain top-level file
/// `classify_source` would already reject, but named here explicitly rather
/// than relying on that incidentally).
pub(crate) const EXPORT_MARKER_FILENAME: &str = ".fmt-export-version";
const BATCH_SIZE: i64 = 8;
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);

type WorkerResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Copied from `plugins/fs-sync/src/commands.rs`: runs a blocking body on
/// tokio's dedicated blocking thread pool instead of the shared async
/// worker threads (whole-branch-review fix — this worker's filesystem calls
/// used to run directly on the runtime that also services the rest of the
/// app). A panic inside `$body` surfaces as a `WorkerResult` error (via
/// `JoinError`'s `Display` -> `String` -> `Box<dyn Error + Send + Sync>`)
/// rather than silently killing this worker's task forever.
macro_rules! spawn_blocking {
    ($body:expr) => {
        tokio::task::spawn_blocking(move || $body)
            .await
            .map_err(|error| error.to_string())?
    };
}

#[derive(Debug, Clone)]
struct DirtyEntity {
    entity_type: String,
    entity_id: String,
    generation: i64,
}

/// Exponential backoff for a permanently (or currently) failing entity, keyed
/// by `(entity_type, entity_id)`. Controller re-drill finding: without this,
/// a stuck row was reattempted (and re-logged as an `ERROR`) on every single
/// drain pass — every ~5s (`RETRY_INTERVAL`) forever — burning cycles and
/// spamming logs indefinitely for a row that will keep failing the exact
/// same way until something external fixes it (a code fix, or the
/// underlying data changing). Lives for the whole `run()` task lifetime (one
/// `RetryBackoff` created once in `run()`, threaded through every
/// `drain_queue`/`drain_with` call) — an in-memory heuristic that resets on
/// app restart is an intentional simplification, not an oversight.
#[derive(Debug, Default)]
struct RetryBackoff {
    state: std::collections::HashMap<(String, String), BackoffEntry>,
}

#[derive(Debug, Clone, Copy)]
struct BackoffEntry {
    consecutive_failures: u32,
    retry_after: std::time::Instant,
}

/// 5s, 10s, 20s, 40s, capping at 60s from the 5th consecutive failure
/// onward. `now`/`consecutive_failures` are both explicit parameters (no
/// internal `Instant::now()` calls) so the whole backoff state machine is
/// testable with constructed instants — no real sleeping required.
fn backoff_delay(consecutive_failures: u32) -> Duration {
    const BASE: Duration = Duration::from_secs(5);
    const MAX: Duration = Duration::from_secs(60);
    let exponent = consecutive_failures.saturating_sub(1).min(4);
    BASE.saturating_mul(1 << exponent).min(MAX)
}

impl RetryBackoff {
    fn new() -> Self {
        Self::default()
    }

    /// True if `key` failed recently enough that it's still within its
    /// backoff window and should be skipped entirely this pass — no export
    /// attempt, no log line, just left queued for a later retry.
    fn should_skip(&self, key: &(String, String), now: std::time::Instant) -> bool {
        self.state
            .get(key)
            .is_some_and(|entry| now < entry.retry_after)
    }

    /// Records a failed attempt and returns the delay before `key` should be
    /// attempted again.
    fn record_failure(&mut self, key: (String, String), now: std::time::Instant) -> Duration {
        let entry = self.state.entry(key).or_insert(BackoffEntry {
            consecutive_failures: 0,
            retry_after: now,
        });
        entry.consecutive_failures += 1;
        let delay = backoff_delay(entry.consecutive_failures);
        entry.retry_after = now + delay;
        delay
    }

    /// Clears any backoff history for `key` — a fresh failure after a
    /// success starts the exponential sequence over from the 5s base delay.
    fn record_success(&mut self, key: &(String, String)) {
        self.state.remove(key);
    }
}

pub fn spawn(app: AppHandle, db: Arc<hypr_db_core::Db>) {
    tauri::async_runtime::spawn(async move {
        run(app, db).await;
    });
}

async fn run(app: AppHandle, db: Arc<hypr_db_core::Db>) {
    let mut changes = db.change_notifier().subscribe();
    let mut backoff = RetryBackoff::new();

    if let Err(error) = ensure_first_run_full_export(&app, db.pool()).await {
        tracing::error!(%error, "failed to enqueue the first-run full vault export");
    }

    loop {
        if let Err(error) = drain_queue(&app, db.pool(), &mut backoff).await {
            tracing::error!(%error, "failed to export vault files");
        }

        tokio::select! {
            change = changes.recv() => {
                match change {
                    Ok(change) if change.table == "vault_export_dirty" => {}
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = tokio::time::sleep(RETRY_INTERVAL) => {}
        }

        // Debounce (~500ms): coalesce a burst of rapid edits (e.g.
        // keystroke-by-keystroke note autosave) into one export pass instead
        // of re-rendering on every single change.
        loop {
            match tokio::time::timeout(DEBOUNCE_WINDOW, changes.recv()).await {
                Err(_elapsed) => break,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return,
                Ok(_) => continue,
            }
        }
    }
}

fn vault_base_path<R: tauri::Runtime>(app: &AppHandle<R>) -> WorkerResult<PathBuf> {
    Ok(app.settings().vault_base()?.as_std_path().to_path_buf())
}

/// Marks `path` *and* the temp path it will be staged through (both relative
/// to `vault_base`) as our own write *before* performing it, per the notify
/// plugin's own-write TTL — see the loop prevention analysis in the module
/// doc. The tmp path needs marking too: `plugins/notify`'s watcher fires on
/// *any* filesystem event it doesn't otherwise skip, including the tmp
/// file's own create-then-rename-away, and unlike `path` it was never
/// previously marked by anything else.
///
/// Runs the actual filesystem write on tokio's blocking thread pool
/// (`spawn_blocking`), not the shared async runtime — whole-branch-review
/// fix; this worker used to block the same runtime the rest of the app's
/// async tasks (including the DB pool's own connections) share.
async fn write_tracked<R: tauri::Runtime>(
    app: &AppHandle<R>,
    vault_base: &Path,
    path: &Path,
    content: &[u8],
) -> WorkerResult<()> {
    let tmp_path = export::tmp_sibling_path(path);
    let relative = hypr_fs_sync_core::path::to_relative_path(path, vault_base);
    let relative_tmp = hypr_fs_sync_core::path::to_relative_path(&tmp_path, vault_base);
    app.notify().mark_own_writes(&[relative, relative_tmp]);

    let vault_base = vault_base.to_path_buf();
    let path_owned = path.to_path_buf();
    let content = content.to_vec();
    let display_path = path.display().to_string();

    Ok(spawn_blocking!({
        export::write_file_atomic(&vault_base, &path_owned, &tmp_path, &content)
            .map(|_| ())
            .map_err(|error| format!("failed to write {display_path}: {error}"))
    })?)
}

/// Moves `path` to `.trash/<date>/...` if it exists, marking it first so the
/// watcher doesn't treat the removal as an external deletion. Runs on
/// tokio's blocking thread pool, same rationale as `write_tracked`.
async fn trash_if_exists<R: tauri::Runtime>(
    app: &AppHandle<R>,
    vault_base: &Path,
    path: &Path,
) -> WorkerResult<()> {
    let relative = hypr_fs_sync_core::path::to_relative_path(path, vault_base);
    app.notify().mark_own_writes(&[relative]);

    let vault_base = vault_base.to_path_buf();
    let path_owned = path.to_path_buf();
    let display_path = path.display().to_string();

    Ok(spawn_blocking!({
        export::move_to_trash(&vault_base, &path_owned)
            .map(|_| ())
            .map_err(|error| format!("failed to trash {display_path}: {error}"))
    })?)
}

async fn ensure_first_run_full_export<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> WorkerResult<()> {
    let vault_base = vault_base_path(app)?;
    let marker = vault_base.join(EXPORT_MARKER_FILENAME);

    let marker_for_check = marker.clone();
    let already_marked: bool = spawn_blocking!(Ok::<bool, String>(marker_for_check.exists()))?;
    if already_marked {
        return Ok(());
    }

    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_export_dirty")
        .fetch_one(pool)
        .await?;
    if pending > 0 {
        return Ok(());
    }

    enqueue_all_entities(pool).await?;

    let vault_base_for_write = vault_base.clone();
    let marker_for_write = marker.clone();
    spawn_blocking!({
        std::fs::create_dir_all(&vault_base_for_write)
            .and_then(|()| std::fs::write(&marker_for_write, EXPORT_MARKER_VERSION))
            .map_err(|error| error.to_string())
    })?;

    tracing::info!("enqueued first-run full vault export");
    Ok(())
}

/// Enqueues every vault-exportable entity, like search_index's
/// `enqueue_all_entities` — used both for the first-run export above and
/// the `export_vault_now` command (Settings -> Storage -> "Re-export all
/// files").
async fn enqueue_all_entities(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO vault_export_dirty (entity_type, entity_id)
         SELECT 'session', id FROM sessions WHERE deleted_at IS NULL
         ON CONFLICT(entity_type, entity_id) DO UPDATE SET
           generation = vault_export_dirty.generation + 1,
           queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO vault_export_dirty (entity_type, entity_id)
         SELECT 'human', id FROM humans WHERE deleted_at IS NULL
         ON CONFLICT(entity_type, entity_id) DO UPDATE SET
           generation = vault_export_dirty.generation + 1,
           queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO vault_export_dirty (entity_type, entity_id)
         SELECT 'organization', id FROM organizations WHERE deleted_at IS NULL
         ON CONFLICT(entity_type, entity_id) DO UPDATE SET
           generation = vault_export_dirty.generation + 1,
           queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO vault_export_dirty (entity_type, entity_id)
         SELECT 'chat_group', id FROM chat_groups WHERE deleted_at IS NULL
         ON CONFLICT(entity_type, entity_id) DO UPDATE SET
           generation = vault_export_dirty.generation + 1,
           queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .execute(&mut *tx)
    .await?;

    for entity_type in [
        "calendars_file",
        "events_file",
        "daily_notes_file",
        "tasks_file",
        "settings_file",
    ] {
        sqlx::query(
            "INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES (?, 'all')
             ON CONFLICT(entity_type, entity_id) DO UPDATE SET
               generation = vault_export_dirty.generation + 1,
               queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(entity_type)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await
}

/// Core drain loop, generic over `export_one` so it's testable without a
/// Tauri `AppHandle` (see the tests module). Pages `vault_export_dirty` in
/// batches of `BATCH_SIZE`, calling `export_one` for each row.
///
/// Whole-branch-review fix (root cause of the on-device stalled-drain
/// defect): a per-entity failure used to propagate via `?` straight out of
/// the batch loop, which (a) permanently wedged that one entity at the head
/// of the FIFO queue — it's always the oldest `queued_at` row, so every
/// future batch hit the identical failure again before reaching anything
/// queued after it — and (b) meant **none** of that batch's entities got
/// acknowledged, even ones whose own export had already completed
/// successfully earlier in the same `for` loop. On-device this was observed
/// as: a session's `_memo.md` got re-rendered (so that document's render
/// succeeded), but the session's own dirty row plus five singleton-file rows
/// enqueued in the very same `enqueue_all_entities` transaction (and hence
/// ordered immediately after it by `queued_at`) never got created, for as
/// long as *something* in that session's export kept failing on every retry.
///
/// Now: a failing entity is logged and left queued for a later retry, but
/// never blocks its siblings — every entity that *did* succeed in the same
/// batch is acknowledged regardless. If an entire batch makes zero progress
/// (every entity in it fails), the loop stops rather than spin forever
/// re-querying the identical unacknowledged rows; the next change signal or
/// `RETRY_INTERVAL` tick tries again. This also guarantees that
/// `enqueue_all_entities` followed by one call here drains to empty in a
/// single pass (modulo entities that keep failing every attempt) with no
/// external DB write or change-notifier signal required — the second half
/// of the same fix.
///
/// Controller re-drill follow-up: a batch-level stop isn't enough on its
/// own — with `RETRY_INTERVAL` waking `run()`'s loop every ~5s, a
/// permanently-failing entity got reattempted (and re-logged as an
/// `ERROR`) every single cycle, forever. `backoff` (see `RetryBackoff`)
/// suppresses that: an entity within its backoff window is skipped
/// entirely (no attempt, no log) until the window elapses, and the delay
/// grows exponentially up to 60s the longer it keeps failing.
async fn drain_with<F, Fut>(
    pool: &SqlitePool,
    backoff: &mut RetryBackoff,
    mut export_one: F,
) -> Result<(), sqlx::Error>
where
    F: FnMut(DirtyEntity) -> Fut,
    Fut: std::future::Future<Output = WorkerResult<()>>,
{
    loop {
        let rows = sqlx::query(
            "SELECT entity_type, entity_id, generation
             FROM vault_export_dirty
             ORDER BY queued_at, entity_type, entity_id
             LIMIT ?",
        )
        .bind(BATCH_SIZE)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            return Ok(());
        }

        let dirty_entities = rows
            .into_iter()
            .map(|row| DirtyEntity {
                entity_type: row.get("entity_type"),
                entity_id: row.get("entity_id"),
                generation: row.get("generation"),
            })
            .collect::<Vec<_>>();

        let mut succeeded = Vec::with_capacity(dirty_entities.len());
        for entity in dirty_entities {
            let key = (entity.entity_type.clone(), entity.entity_id.clone());
            let now = std::time::Instant::now();
            if backoff.should_skip(&key, now) {
                continue;
            }

            let attempt = entity.clone();
            match export_one(attempt).await {
                Ok(()) => {
                    backoff.record_success(&key);
                    succeeded.push(entity);
                }
                Err(error) => {
                    let retry_in = backoff.record_failure(key, now);
                    tracing::error!(
                        entity_type = %entity.entity_type,
                        entity_id = %entity.entity_id,
                        %error,
                        retry_in_secs = retry_in.as_secs(),
                        "failed to export vault entity; backing off before retry"
                    );
                }
            }
        }

        if succeeded.is_empty() {
            return Ok(());
        }

        acknowledge_dirty_entities(pool, &succeeded).await?;
        tokio::task::yield_now().await;
    }
}

async fn drain_queue<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    backoff: &mut RetryBackoff,
) -> WorkerResult<()> {
    let vault_base = vault_base_path(app)?;
    // `vault_base_ref` is a `&Path` (Copy) rather than capturing the owned
    // `vault_base` `PathBuf` directly: the closure below is called once per
    // entity by `drain_with`'s loop, and an `async move` block moves
    // whatever it references — a non-Copy `PathBuf` could only be moved out
    // of the closure's environment once, breaking every call after the
    // first. A `&Path` copies trivially on every call instead.
    let vault_base_ref: &Path = &vault_base;
    drain_with(pool, backoff, move |entity| async move {
        export_entity(app, pool, vault_base_ref, &entity).await
    })
    .await
    .map_err(Into::into)
}

async fn acknowledge_dirty_entities(
    pool: &SqlitePool,
    dirty_entities: &[DirtyEntity],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    for dirty in dirty_entities {
        sqlx::query(
            "DELETE FROM vault_export_dirty
             WHERE entity_type = ? AND entity_id = ? AND generation = ?",
        )
        .bind(&dirty.entity_type)
        .bind(&dirty.entity_id)
        .bind(dirty.generation)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

async fn export_entity<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
    entity: &DirtyEntity,
) -> WorkerResult<()> {
    match entity.entity_type.as_str() {
        "session" => export_session(app, pool, vault_base, &entity.entity_id).await,
        "human" => export_human(app, pool, vault_base, &entity.entity_id).await,
        "organization" => export_organization(app, pool, vault_base, &entity.entity_id).await,
        "chat_group" => export_chat_group(app, pool, vault_base, &entity.entity_id).await,
        "calendars_file" => export_calendars_file(app, pool, vault_base).await,
        "events_file" => export_events_file(app, pool, vault_base).await,
        "daily_notes_file" => export_daily_notes_file(app, pool, vault_base).await,
        "tasks_file" => export_tasks_file(app, pool, vault_base).await,
        "settings_file" => export_settings_file(app, pool, vault_base).await,
        entity_type => {
            tracing::warn!(entity_type, "ignoring unknown vault export entity type");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// sessions/<folder>/<id>/*
// ---------------------------------------------------------------------------

async fn export_session<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
    session_id: &str,
) -> WorkerResult<()> {
    let sessions_base = vault_base.join("sessions");
    // `find_session_dir` errors for a non-UUID `session_id` (e.g. a
    // `legacy_vault.rs`-recovered session, whose id is a sha256 hex string,
    // not a UUID). Rather than propagate that and permanently wedge this
    // entity at the head of the FIFO dirty queue (drain_queue aborts the
    // whole batch on the first error and only acks entities it fully
    // processed), fall back to the same flat `sessions/<id>` layout
    // `find_session_dir` itself already falls back to when a *valid* UUID
    // just isn't found on disk yet.
    let session_dir = hypr_fs_sync_core::session::find_session_dir(&sessions_base, session_id)
        .unwrap_or_else(|_| sessions_base.join(session_id));

    let session_row = sqlx::query(
        "SELECT id, owner_user_id, title, created_at, started_at, ended_at,
                event_id, external_event_id, series_id, event_json
         FROM sessions WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = session_row else {
        trash_if_exists(app, vault_base, &session_dir).await?;
        return Ok(());
    };

    let session = export::SessionMeta {
        id: row.get("id"),
        owner_user_id: row.get("owner_user_id"),
        title: row.get("title"),
        created_at: row.get("created_at"),
        started_at: row.get("started_at"),
        ended_at: row.get("ended_at"),
        event_id: row.get("event_id"),
        external_event_id: row.get("external_event_id"),
        series_id: row.get("series_id"),
        event_json: row.get("event_json"),
    };

    let participants = sqlx::query(
        "SELECT id, owner_user_id, human_id, source, display_name, email, role
         FROM session_participants
         WHERE session_id = ? AND deleted_at IS NULL
         ORDER BY created_at, id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| export::SessionParticipant {
        id: row.get("id"),
        owner_user_id: row.get("owner_user_id"),
        human_id: row.get("human_id"),
        source: row.get("source"),
        display_name: row.get("display_name"),
        email: row.get("email"),
        role: row.get("role"),
    })
    .collect::<Vec<_>>();

    let tags: Vec<String> = sqlx::query_scalar(
        "SELECT tags.name
         FROM session_tags
         JOIN tags ON tags.id = session_tags.tag_id
         WHERE session_tags.session_id = ?
           AND session_tags.deleted_at IS NULL
           AND tags.deleted_at IS NULL
         ORDER BY tags.name",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    let key_facts = sqlx::query(
        "SELECT body, source_hash, created_by, created_at, updated_at
         FROM session_documents
         WHERE session_id = ? AND kind = 'key_facts' AND deleted_at IS NULL
         ORDER BY created_at, id
         LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?
    .map(|row| export::SessionKeyFacts {
        content: row.get("body"),
        source_hash: row.get("source_hash"),
        user_id: row.get("created_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    });

    let meta_value = export::render_session_meta(&session, &participants, &tags, key_facts.as_ref());
    let meta_content = hypr_fs_sync_core::json::serialize(meta_value)
        .map_err(|error| format!("failed to serialize _meta.json for {session_id}: {error}"))?;
    write_tracked(
        app,
        vault_base,
        &session_dir.join("_meta.json"),
        meta_content.as_bytes(),
    )
    .await?;

    export_session_documents(app, pool, vault_base, &session_dir, session_id).await?;
    export_session_transcript(app, pool, vault_base, &session_dir, session_id).await?;

    Ok(())
}

/// Renders every live `session_documents` row to its vault file and trashes
/// any stale `.md` file left over from a document that was deleted, or whose
/// `kind` changed since the last export (a known limitation: a document
/// that changes kind without changing id/content otherwise still gets
/// reconciled correctly here because we always recompute the *expected* file
/// set from scratch on every pass).
///
/// Whole-branch-review fix: a single document's render failure (e.g.
/// malformed prosemirror JSON `tiptap_json_to_md` can't convert) used to
/// propagate straight out of this function via `?`, aborting the *entire*
/// session's export — meta.json, every other document, and the transcript —
/// even though only one document was actually broken. Now a per-document
/// failure is logged and skipped; its filename slot is still protected from
/// the stale-file cleanup below (so a document we currently can't render
/// doesn't get its last-known-good vault file wrongly trashed as
/// "orphaned"), and every other document, plus the caller's meta/transcript
/// export, proceeds normally.
async fn export_session_documents<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
    session_dir: &Path,
    session_id: &str,
) -> WorkerResult<()> {
    let rows = sqlx::query(
        "SELECT id, session_id, kind, template_id, title, body_format, body, sort_order
         FROM session_documents
         WHERE session_id = ?
           AND kind NOT IN ('key_facts', 'meeting_chat')
           AND deleted_at IS NULL
         ORDER BY sort_order, created_at, id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    let mut seen_kind = HashSet::new();
    let mut expected_filenames = HashSet::new();

    for row in rows {
        let document = export::SessionDocument {
            id: row.get("id"),
            session_id: row.get("session_id"),
            kind: row.get("kind"),
            template_id: row.get("template_id"),
            title: row.get("title"),
            body_format: row.get("body_format"),
            body: row.get("body"),
            sort_order: row.get("sort_order"),
        };

        let is_first_of_kind = seen_kind.insert(document.kind.clone());
        let Some(filename) = export::session_document_filename(&document, is_first_of_kind) else {
            tracing::debug!(
                session_id,
                document_id = %document.id,
                kind = %document.kind,
                "vault has no file slot for this session_documents kind; skipping export"
            );
            continue;
        };

        // Protect this slot from the stale-cleanup scan below regardless of
        // whether the render below actually succeeds this pass.
        expected_filenames.insert(filename.clone());

        if let Err(error) =
            render_and_write_document(app, vault_base, session_dir, &document, &filename).await
        {
            tracing::error!(
                session_id,
                document_id = %document.id,
                kind = %document.kind,
                %error,
                "failed to export session document; leaving its vault file unchanged"
            );
        }
    }

    let expected_for_scan = expected_filenames.clone();
    let session_dir_owned = session_dir.to_path_buf();
    let stale_paths: Vec<PathBuf> = spawn_blocking!({
        let mut stale = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&session_dir_owned) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if !name.ends_with(".md") || name.contains(".conflict-") {
                    continue;
                }
                if expected_for_scan.contains(name) {
                    continue;
                }
                stale.push(path);
            }
        }
        stale
    });

    for path in stale_paths {
        trash_if_exists(app, vault_base, &path).await?;
    }

    Ok(())
}

async fn render_and_write_document<R: tauri::Runtime>(
    app: &AppHandle<R>,
    vault_base: &Path,
    session_dir: &Path,
    document: &export::SessionDocument,
    filename: &str,
) -> WorkerResult<()> {
    let rendered = export::render_session_document(document)
        .map_err(|error| format!("failed to render session document {}: {error}", document.id))?;
    let content = rendered.render().map_err(|error| {
        format!(
            "failed to render markdown for document {}: {error}",
            document.id
        )
    })?;
    write_tracked(app, vault_base, &session_dir.join(filename), content.as_bytes()).await
}

async fn export_session_transcript<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
    session_dir: &Path,
    session_id: &str,
) -> WorkerResult<()> {
    let rows = sqlx::query(
        "SELECT id, owner_user_id, session_id, created_at, started_at_ms, ended_at_ms,
                memo, words_json, speaker_hints_json
         FROM transcripts
         WHERE session_id = ? AND deleted_at IS NULL
         ORDER BY started_at_ms, created_at, id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    let transcript_path = session_dir.join("transcript.json");
    if rows.is_empty() {
        trash_if_exists(app, vault_base, &transcript_path).await?;
        return Ok(());
    }

    let transcripts = rows
        .into_iter()
        .map(|row| export::Transcript {
            id: row.get("id"),
            owner_user_id: row.get("owner_user_id"),
            session_id: row.get("session_id"),
            created_at: row.get("created_at"),
            started_at_ms: row.get("started_at_ms"),
            ended_at_ms: row.get("ended_at_ms"),
            memo: row.get("memo"),
            words_json: row.get("words_json"),
            speaker_hints_json: row.get("speaker_hints_json"),
        })
        .collect::<Vec<_>>();

    let value = export::render_transcripts(&transcripts);
    let content = hypr_fs_sync_core::json::serialize(value)
        .map_err(|error| format!("failed to serialize transcript.json for {session_id}: {error}"))?;
    write_tracked(app, vault_base, &transcript_path, content.as_bytes()).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// humans/<id>.md, organizations/<id>.md
// ---------------------------------------------------------------------------

async fn export_human<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
    id: &str,
) -> WorkerResult<()> {
    let path = vault_base.join("humans").join(format!("{id}.md"));
    let row = sqlx::query(
        "SELECT owner_user_id, organization_id, name, email, phone, job_title,
                linkedin_username, memo, pinned, pin_order, created_at
         FROM humans WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        trash_if_exists(app, vault_base, &path).await?;
        return Ok(());
    };

    let human = export::Human {
        owner_user_id: row.get("owner_user_id"),
        organization_id: row.get("organization_id"),
        name: row.get("name"),
        email: row.get("email"),
        phone: row.get("phone"),
        job_title: row.get("job_title"),
        linkedin_username: row.get("linkedin_username"),
        memo: row.get("memo"),
        pinned: row.get("pinned"),
        pin_order: row.get("pin_order"),
        created_at: row.get("created_at"),
    };

    let content = export::render_human(&human)
        .render()
        .map_err(|error| format!("failed to render human {id}: {error}"))?;
    write_tracked(app, vault_base, &path, content.as_bytes()).await?;
    Ok(())
}

async fn export_organization<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
    id: &str,
) -> WorkerResult<()> {
    let path = vault_base.join("organizations").join(format!("{id}.md"));
    let row = sqlx::query(
        "SELECT owner_user_id, name, memo, pinned, pin_order, created_at
         FROM organizations WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        trash_if_exists(app, vault_base, &path).await?;
        return Ok(());
    };

    let organization = export::Organization {
        owner_user_id: row.get("owner_user_id"),
        name: row.get("name"),
        memo: row.get("memo"),
        pinned: row.get("pinned"),
        pin_order: row.get("pin_order"),
        created_at: row.get("created_at"),
    };

    let content = export::render_organization(&organization)
        .render()
        .map_err(|error| format!("failed to render organization {id}: {error}"))?;
    write_tracked(app, vault_base, &path, content.as_bytes()).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// chats/<group>/messages.json
// ---------------------------------------------------------------------------

async fn export_chat_group<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
    id: &str,
) -> WorkerResult<()> {
    let chat_dir = vault_base.join("chats").join(id);
    let row = sqlx::query(
        "SELECT id, owner_user_id, title, created_at
         FROM chat_groups WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        trash_if_exists(app, vault_base, &chat_dir).await?;
        return Ok(());
    };

    let group = export::ChatGroup {
        id: row.get("id"),
        owner_user_id: row.get("owner_user_id"),
        title: row.get("title"),
        created_at: row.get("created_at"),
    };

    let messages = sqlx::query(
        "SELECT id, chat_group_id, owner_user_id, role, content, metadata_json,
                parts_json, status, created_at
         FROM chat_messages
         WHERE chat_group_id = ? AND deleted_at IS NULL
         ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| export::ChatMessage {
        id: row.get("id"),
        chat_group_id: row.get("chat_group_id"),
        owner_user_id: row.get("owner_user_id"),
        role: row.get("role"),
        content: row.get("content"),
        metadata_json: row.get("metadata_json"),
        parts_json: row.get("parts_json"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    })
    .collect::<Vec<_>>();

    let value = export::render_chat(&group, &messages);
    let content = hypr_fs_sync_core::json::serialize(value)
        .map_err(|error| format!("failed to serialize messages.json for chat {id}: {error}"))?;
    write_tracked(app, vault_base, &chat_dir.join("messages.json"), content.as_bytes()).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// calendars.json / events.json / daily_notes.json / tasks.json / settings.json
// ---------------------------------------------------------------------------

async fn export_calendars_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
) -> WorkerResult<()> {
    let path = vault_base.join("calendars.json");
    let rows = sqlx::query(
        "SELECT id, tracking_id_calendar, name, enabled, provider, source, color, connection_id
         FROM calendars WHERE deleted_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        trash_if_exists(app, vault_base, &path).await?;
        return Ok(());
    }

    let calendars = rows
        .into_iter()
        .map(|row| export::Calendar {
            id: row.get("id"),
            tracking_id_calendar: row.get("tracking_id_calendar"),
            name: row.get("name"),
            enabled: row.get("enabled"),
            provider: row.get("provider"),
            source: row.get("source"),
            color: row.get("color"),
            connection_id: row.get("connection_id"),
        })
        .collect::<Vec<_>>();

    let value = export::render_calendars(&calendars);
    let content = hypr_fs_sync_core::json::serialize(value)
        .map_err(|error| format!("failed to serialize calendars.json: {error}"))?;
    write_tracked(app, vault_base, &path, content.as_bytes()).await?;
    Ok(())
}

async fn export_events_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
) -> WorkerResult<()> {
    let path = vault_base.join("events.json");
    let rows = sqlx::query(
        "SELECT id, tracking_id_event, calendar_id, title, started_at, ended_at, location,
                meeting_link, description, note, recurrence_series_id, has_recurrence_rules,
                is_all_day, provider, participants_json
         FROM events WHERE deleted_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        trash_if_exists(app, vault_base, &path).await?;
        return Ok(());
    }

    let events = rows
        .into_iter()
        .map(|row| export::CalendarEvent {
            id: row.get("id"),
            tracking_id_event: row.get("tracking_id_event"),
            calendar_id: row.get("calendar_id"),
            title: row.get("title"),
            started_at: row.get("started_at"),
            ended_at: row.get("ended_at"),
            location: row.get("location"),
            meeting_link: row.get("meeting_link"),
            description: row.get("description"),
            note: row.get("note"),
            recurrence_series_id: row.get("recurrence_series_id"),
            has_recurrence_rules: row.get("has_recurrence_rules"),
            is_all_day: row.get("is_all_day"),
            provider: row.get("provider"),
            participants_json: row.get("participants_json"),
        })
        .collect::<Vec<_>>();

    let value = export::render_events(&events);
    let content = hypr_fs_sync_core::json::serialize(value)
        .map_err(|error| format!("failed to serialize events.json: {error}"))?;
    write_tracked(app, vault_base, &path, content.as_bytes()).await?;
    Ok(())
}

async fn export_daily_notes_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
) -> WorkerResult<()> {
    let path = vault_base.join("daily_notes.json");
    let rows = sqlx::query(
        "SELECT id, owner_user_id, note_date, body
         FROM daily_notes WHERE deleted_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        trash_if_exists(app, vault_base, &path).await?;
        return Ok(());
    }

    let notes = rows
        .into_iter()
        .map(|row| export::DailyNote {
            id: row.get("id"),
            owner_user_id: row.get("owner_user_id"),
            note_date: row.get("note_date"),
            body: row.get("body"),
        })
        .collect::<Vec<_>>();

    let value = export::render_daily_notes(&notes);
    let content = hypr_fs_sync_core::json::serialize(value)
        .map_err(|error| format!("failed to serialize daily_notes.json: {error}"))?;
    write_tracked(app, vault_base, &path, content.as_bytes()).await?;
    Ok(())
}

async fn export_tasks_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
) -> WorkerResult<()> {
    let path = vault_base.join("tasks.json");
    let items = fetch_action_items(pool).await?;

    if items.is_empty() {
        trash_if_exists(app, vault_base, &path).await?;
        return Ok(());
    }

    let value = export::render_tasks(&items);
    let content = hypr_fs_sync_core::json::serialize(value)
        .map_err(|error| format!("failed to serialize tasks.json: {error}"))?;
    write_tracked(app, vault_base, &path, content.as_bytes()).await?;
    Ok(())
}

/// Extracted from `export_tasks_file` so it's independently testable
/// against a real migrated database — the previous round-trip test built
/// `export::ActionItem` values directly and never actually ran this SQL,
/// which is exactly how a controller physical drill caught a live
/// `no such column: owner_user_id` error this SQL used to have.
/// `action_items` has no `owner_user_id` column (it's `created_by` — this
/// table doesn't follow the `owner_user_id`/`workspace_id` convention most
/// other tables do; verified against the live schema in
/// `crates/db-app/migrations/20260710223922_canonical_data_model.sql`, not
/// just the `LegacyActionItem`/`export::ActionItem` field name).
async fn fetch_action_items(pool: &SqlitePool) -> Result<Vec<export::ActionItem>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, created_by, source_type, source_id, source_order, status, text,
                body_json, due_at
         FROM action_items WHERE deleted_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| export::ActionItem {
            id: row.get("id"),
            owner_user_id: row.get("created_by"),
            source_type: row.get("source_type"),
            source_id: row.get("source_id"),
            source_order: row.get("source_order"),
            status: row.get("status"),
            text: row.get("text"),
            body_json: row.get("body_json"),
            due_at: row.get("due_at"),
        })
        .collect())
}

/// `parse_settings` only understands a single, whole-file blob keyed to the
/// `legacy_settings_document` row — see `export::render_settings`'s doc
/// comment for why the rest of `app_settings` intentionally isn't mirrored.
async fn export_settings_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    vault_base: &Path,
) -> WorkerResult<()> {
    let path = vault_base.join("settings.json");
    let value_json: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM app_settings WHERE id = 'legacy_settings_document'")
            .fetch_optional(pool)
            .await?;

    let Some(value_json) = value_json else {
        trash_if_exists(app, vault_base, &path).await?;
        return Ok(());
    };

    let value = export::render_settings(&value_json);
    let content = hypr_fs_sync_core::json::serialize(value)
        .map_err(|error| format!("failed to serialize settings.json: {error}"))?;
    write_tracked(app, vault_base, &path, content.as_bytes()).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri command: full re-export (Settings -> Storage -> "Re-export all
// files")
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn export_vault_now<R: tauri::Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let db = app.state::<Arc<hypr_db_core::Db>>();
    enqueue_all_entities(db.pool()).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> hypr_db_core::Db {
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        db
    }

    #[test]
    fn backoff_delay_grows_exponentially_and_caps_at_sixty_seconds() {
        assert_eq!(backoff_delay(1), Duration::from_secs(5));
        assert_eq!(backoff_delay(2), Duration::from_secs(10));
        assert_eq!(backoff_delay(3), Duration::from_secs(20));
        assert_eq!(backoff_delay(4), Duration::from_secs(40));
        assert_eq!(backoff_delay(5), Duration::from_secs(60));
        assert_eq!(backoff_delay(6), Duration::from_secs(60));
        assert_eq!(backoff_delay(1_000), Duration::from_secs(60));
    }

    /// Pure state-machine test using constructed `Instant`s (no real
    /// sleeping) — controller re-drill fix for "the stuck row retried every
    /// ~5.5s forever, one ERROR log line each time".
    #[test]
    fn retry_backoff_skips_within_the_window_grows_on_repeat_failure_and_resets_on_success() {
        let mut backoff = RetryBackoff::new();
        let key = ("session".to_string(), "s1".to_string());
        let t0 = std::time::Instant::now();

        assert!(
            !backoff.should_skip(&key, t0),
            "an entity with no failure history must never be skipped"
        );

        let delay = backoff.record_failure(key.clone(), t0);
        assert_eq!(delay, Duration::from_secs(5));
        assert!(
            backoff.should_skip(&key, t0 + Duration::from_secs(1)),
            "still within the 5s window"
        );
        assert!(
            !backoff.should_skip(&key, t0 + Duration::from_secs(6)),
            "window elapsed"
        );

        // A second consecutive failure, attempted after the first window
        // elapsed, grows the delay.
        let t1 = t0 + Duration::from_secs(6);
        let delay2 = backoff.record_failure(key.clone(), t1);
        assert_eq!(delay2, Duration::from_secs(10));
        assert!(backoff.should_skip(&key, t1 + Duration::from_secs(1)));

        // A success clears the history entirely.
        backoff.record_success(&key);
        assert!(!backoff.should_skip(&key, t1 + Duration::from_secs(1)));

        // The next failure after a success starts over at the 5s base delay,
        // not continuing from where the previous streak left off.
        let delay3 = backoff.record_failure(key, t1);
        assert_eq!(delay3, Duration::from_secs(5));
    }

    /// Integration-level regression test for the log-spam/burned-cycles
    /// fix: a repeatedly failing entity must not be reattempted (and hence
    /// not re-logged) on the very next `drain_with` pass, even though it's
    /// still sitting in `vault_export_dirty` the whole time. No real
    /// sleeping needed — the backoff window (5s minimum) trivially still
    /// covers the near-instantaneous gap between two consecutive test calls.
    #[tokio::test]
    async fn drain_with_does_not_immediately_reattempt_a_backed_off_entity() {
        let db = test_db().await;
        sqlx::query("INSERT INTO sessions (id, title) VALUES ('bad-1', 'B')")
            .execute(db.pool())
            .await
            .unwrap();
        // `test_db()`'s `prepare_schema` bootstraps a `cloudsync_workspace_binding`
        // row in `app_settings`, which the `settings_file` trigger also
        // enqueues — irrelevant here, so drop it and keep only the one
        // entity this test cares about (same pattern as
        // `tags_trigger_propagates_to_every_session_that_references_the_tag`).
        sqlx::query(
            "DELETE FROM vault_export_dirty WHERE NOT (entity_type = 'session' AND entity_id = 'bad-1')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut backoff = RetryBackoff::new();

        for _ in 0..2 {
            let attempts = attempts.clone();
            drain_with(db.pool(), &mut backoff, |_entity| {
                let attempts = attempts.clone();
                async move {
                    attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err("boom".into())
                }
            })
            .await
            .unwrap();
        }

        assert_eq!(
            attempts.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the second pass must skip the still-backed-off entity, not reattempt it"
        );

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_export_dirty")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(remaining, 1, "the entity is still queued for a later retry");
    }

    /// Controller physical-drill regression test: `fetch_action_items`
    /// (extracted from `export_tasks_file`) must run its SQL against a
    /// *real* migrated `action_items` table, not just a directly-constructed
    /// `export::ActionItem` value — that's exactly the gap that let a
    /// `no such column: owner_user_id` error reach a real device (the
    /// round-trip test built rows by hand and never executed this query).
    #[tokio::test]
    async fn fetch_action_items_runs_against_the_real_action_items_schema() {
        let db = test_db().await;
        sqlx::query(
            "INSERT INTO action_items
               (id, created_by, source_type, source_id, source_order, status, text, body_json, due_at)
             VALUES ('task-1', 'user-1', 'session', 'session-1', 2, 'done', 'Send follow-up',
                     '[{\"type\":\"text\",\"text\":\"Send follow-up\"}]', '2026-07-05')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let items = fetch_action_items(db.pool()).await.unwrap();

        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, "task-1");
        assert_eq!(item.owner_user_id, "user-1");
        assert_eq!(item.source_type, "session");
        assert_eq!(item.source_id, "session-1");
        assert_eq!(item.source_order, 2);
        assert_eq!(item.status, "done");
        assert_eq!(item.text, "Send follow-up");
        assert_eq!(item.due_at, "2026-07-05");
    }

    #[tokio::test]
    async fn fetch_action_items_excludes_soft_deleted_rows() {
        let db = test_db().await;
        sqlx::query(
            "INSERT INTO action_items (id, created_by, source_type, source_id, status, text, deleted_at)
             VALUES ('task-deleted', 'user-1', 'session', 'session-1', 'todo', 'Gone',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let items = fetch_action_items(db.pool()).await.unwrap();

        assert!(items.is_empty());
    }

    /// Regression test for the on-device stalled-drain defect: reproduces
    /// hypothesis 1 exactly (a render error on one entity used to abort the
    /// whole batch loop before `acknowledge_dirty_entities` ever ran, so
    /// nothing in that batch got acked — not even entities that had already
    /// exported successfully earlier in the same `for` loop). `drain_with`'s
    /// generic `export_one` lets this be reproduced headlessly, with no
    /// Tauri `AppHandle`/filesystem involved at all: one entity's "export"
    /// always fails, the other two always succeed, and the failing one must
    /// never block them.
    #[tokio::test]
    async fn drain_with_isolates_a_failing_entity_and_still_acks_its_siblings() {
        let db = test_db().await;
        sqlx::query(
            "INSERT INTO sessions (id, title) VALUES ('good-1', 'A'), ('bad-1', 'B'), ('good-2', 'C')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        drain_with(db.pool(), &mut RetryBackoff::new(), |entity| async move {
            if entity.entity_id == "bad-1" {
                Err("simulated render failure".into())
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();

        let remaining: Vec<String> = sqlx::query_scalar(
            "SELECT entity_id FROM vault_export_dirty ORDER BY entity_id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();

        assert_eq!(
            remaining,
            vec!["bad-1".to_string()],
            "only the entity that actually failed should still be queued"
        );
    }

    /// Regression test for the other half of the same on-device defect:
    /// after `enqueue_all_entities`, a single `drain_with` pass must empty
    /// the queue with no further DB write and no change-notifier signal —
    /// exactly the sequence `run()` does (`ensure_first_run_full_export` then
    /// `drain_queue`, unconditionally, before ever waiting on
    /// `changes.recv()`). Before the fix, a single always-failing entity
    /// among many (see the test above) would have left every entity queued
    /// at or after it permanently undrained.
    #[tokio::test]
    async fn drain_with_empties_the_queue_after_enqueue_all_entities() {
        let db = test_db().await;
        sqlx::query("INSERT INTO sessions (id, title) VALUES ('s1', 'A'), ('s2', 'B')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO humans (id, name) VALUES ('h1', 'Ada')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM vault_export_dirty")
            .execute(db.pool())
            .await
            .unwrap();

        enqueue_all_entities(db.pool()).await.unwrap();
        let queued_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_export_dirty")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(queued_before > 0, "enqueue_all_entities should have queued something");

        drain_with(db.pool(), &mut RetryBackoff::new(), |_entity| async { Ok(()) })
            .await
            .unwrap();

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_export_dirty")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(remaining, 0, "one drain pass must fully empty the queue");
    }

    /// Same scenario as the "empties the queue" test above, but with one
    /// entity (an aggregate singleton file, matching what was observed
    /// on-device) that always fails: the drain must still make maximal
    /// progress on every other entity rather than stalling entirely, and
    /// must terminate (not spin forever re-querying the one entity that can
    /// never succeed).
    #[tokio::test]
    async fn drain_with_makes_progress_on_other_entities_when_one_singleton_always_fails() {
        let db = test_db().await;
        sqlx::query("INSERT INTO sessions (id, title) VALUES ('s1', 'A')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM vault_export_dirty")
            .execute(db.pool())
            .await
            .unwrap();

        enqueue_all_entities(db.pool()).await.unwrap();

        drain_with(db.pool(), &mut RetryBackoff::new(), |entity| async move {
            if entity.entity_type == "settings_file" {
                Err("simulated permanent failure".into())
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();

        let remaining: Vec<(String, String)> = sqlx::query_as(
            "SELECT entity_type, entity_id FROM vault_export_dirty",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();

        assert_eq!(
            remaining,
            vec![("settings_file".to_string(), "all".to_string())],
            "every entity except the permanently-failing one should have drained"
        );
    }

    #[tokio::test]
    async fn enqueue_all_entities_covers_every_live_row_and_every_singleton_file() {
        let db = test_db().await;
        sqlx::query("INSERT INTO sessions (id, title) VALUES ('session-1', 'Planning')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO humans (id, name) VALUES ('human-1', 'Ada')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO organizations (id, name) VALUES ('org-1', 'Acme')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_groups (id, title) VALUES ('chat-1', 'Chat')")
            .execute(db.pool())
            .await
            .unwrap();
        // A deleted session must not be (re-)enqueued by a full re-export.
        sqlx::query(
            "INSERT INTO sessions (id, title, deleted_at) VALUES ('session-deleted', 'Gone', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .execute(db.pool())
        .await
        .unwrap();

        // The triggers above already enqueued these inserts; clear the slate
        // so this test only asserts what `enqueue_all_entities` itself does.
        sqlx::query("DELETE FROM vault_export_dirty")
            .execute(db.pool())
            .await
            .unwrap();

        enqueue_all_entities(db.pool()).await.unwrap();

        let mut rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT entity_type, entity_id FROM vault_export_dirty ORDER BY entity_type, entity_id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        rows.sort();

        assert_eq!(
            rows,
            vec![
                ("calendars_file".to_string(), "all".to_string()),
                ("chat_group".to_string(), "chat-1".to_string()),
                ("daily_notes_file".to_string(), "all".to_string()),
                ("events_file".to_string(), "all".to_string()),
                ("human".to_string(), "human-1".to_string()),
                ("organization".to_string(), "org-1".to_string()),
                ("session".to_string(), "session-1".to_string()),
                ("settings_file".to_string(), "all".to_string()),
                ("tasks_file".to_string(), "all".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn acknowledgement_does_not_drop_a_concurrent_change() {
        let db = test_db().await;
        sqlx::query("INSERT INTO sessions (id, title) VALUES ('session-1', 'Planning')")
            .execute(db.pool())
            .await
            .unwrap();

        let queued_generation: i64 = sqlx::query_scalar(
            "SELECT generation FROM vault_export_dirty
             WHERE entity_type = 'session' AND entity_id = 'session-1'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        sqlx::query("UPDATE sessions SET title = 'Updated' WHERE id = 'session-1'")
            .execute(db.pool())
            .await
            .unwrap();

        acknowledge_dirty_entities(
            db.pool(),
            &[DirtyEntity {
                entity_type: "session".to_string(),
                entity_id: "session-1".to_string(),
                generation: queued_generation,
            }],
        )
        .await
        .unwrap();

        let current_generation: i64 = sqlx::query_scalar(
            "SELECT generation FROM vault_export_dirty
             WHERE entity_type = 'session' AND entity_id = 'session-1'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(current_generation, queued_generation + 1);

        acknowledge_dirty_entities(
            db.pool(),
            &[DirtyEntity {
                entity_type: "session".to_string(),
                entity_id: "session-1".to_string(),
                generation: current_generation,
            }],
        )
        .await
        .unwrap();
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM vault_export_dirty
             WHERE entity_type = 'session' AND entity_id = 'session-1'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn tags_trigger_propagates_to_every_session_that_references_the_tag() {
        let db = test_db().await;
        sqlx::query("INSERT INTO sessions (id, title) VALUES ('session-1', 'A'), ('session-2', 'B')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM vault_export_dirty")
            .execute(db.pool())
            .await
            .unwrap();

        sqlx::query("INSERT INTO tags (id, name) VALUES ('urgent', 'urgent')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO session_tags (id, session_id, tag_id) VALUES
             ('session-1:urgent', 'session-1', 'urgent'),
             ('session-2:urgent', 'session-2', 'urgent')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query("DELETE FROM vault_export_dirty")
            .execute(db.pool())
            .await
            .unwrap();

        // Renaming isn't a real operation (id == name), but any UPDATE on
        // the tag row (e.g. touching an unrelated future column) must still
        // dirty every session that references it.
        sqlx::query("UPDATE tags SET name = 'urgent' WHERE id = 'urgent'")
            .execute(db.pool())
            .await
            .unwrap();

        let mut rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT entity_type, entity_id FROM vault_export_dirty ORDER BY entity_id",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        rows.sort();

        assert_eq!(
            rows,
            vec![
                ("session".to_string(), "session-1".to_string()),
                ("session".to_string(), "session-2".to_string()),
            ]
        );
    }
}
