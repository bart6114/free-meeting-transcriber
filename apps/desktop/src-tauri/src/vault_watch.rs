//! External-edit ingestion: vault file watcher -> index-only refresh.
//!
//! `plugins/notify` recursively watches `vault_base` (debounced ~900ms) and
//! emits a `FileChanged { path }` event for every changed path that isn't
//! one of its own TTL-filtered own-writes (`is_external_path` in
//! `plugins/notify/src/ext.rs`, driven by `mark_own_writes`). This module is
//! that event's first listener.
//!
//! # The incident this rewrite fixes
//!
//! The *previous* version of this module misread the app's own export/trash
//! renames as "session folder removed externally" and soft-hid a live
//! session -- `plugins/notify`'s own-write TTL (1.8s) was shorter than the
//! FSEvents delivery latency it was racing, so a real own-write occasionally
//! arrived *after* the TTL window closed and got treated as an external
//! delete. That version also called `tauri_plugin_db::import_paths`, which
//! could itself write DB rows (a "files win" reconcile) in response.
//!
//! This version never does either of those things:
//!
//! 1. **The event -> action pipeline is pure and has exactly two outcomes**
//!    (`WatchAction::Ignore` / `WatchAction::Refresh(session_id)`) --
//!    `classify_event` below. There is no delete verb, no soft-hide, no
//!    write-to-vault path. The worst an incorrectly-classified event can do
//!    is call `store.refresh_session`, which is read-only on the filesystem
//!    and transactional on the index (see `session_store/rebuild.rs`): a
//!    session whose folder is genuinely gone loses its index rows (correct),
//!    a session whose folder is untouched gets re-indexed as a no-op
//!    (harmless). Files are **never** touched by this module, in either
//!    direction.
//!
//! 2. **The own-write filter is the write journal, not a TTL.**
//!    `SessionStore`'s `write_file` (used by every `write_meta`/`write_note`/
//!    `write_document` call) records the sha256 of exactly what it wrote to
//!    exactly that relative path, with no expiry
//!    (`session_store::journal::WriteJournal::matches_current_file`). A
//!    `FileChanged` for a path whose current on-disk bytes still match the
//!    last hash this store wrote there is *always* recognized as this
//!    store's own write, no matter how late the filesystem event arrives --
//!    that's what `own_write_is_ignored_even_if_late` below asserts.
//!    `plugins/notify`'s `mark_own_writes`/TTL mechanism still exists and
//!    `vault_export.rs` (the legacy DB-to-vault mirror, retired in Task 13)
//!    still calls it before its own writes -- this module may incidentally
//!    benefit from that upstream filtering (fewer events reach it at all),
//!    but its own correctness never depends on it: every event that *does*
//!    arrive here is re-checked against the journal from scratch.
//!
//! # What "external" means for a path outside `sessions/<id>/`
//!
//! Only paths under `sessions/<id>/...` (or the bare `sessions/<id>` folder
//! itself, e.g. a rename's old/new endpoint) ever produce a `Refresh`.
//! Everything else -- `.trash/`, this app's own index/export bookkeeping
//! (`app.db*`, `search_index/`, `AGENTS.md`, the export marker file), and
//! any in-flight `.tmp-`-prefixed atomic-write temp file -- is `Ignore`d
//! outright, independent of the journal check. `plugins/notify`'s own
//! `should_skip_path` already filters most of these upstream (tmp-prefixed
//! basenames, `search_index/`), but this module re-asserts the ones that
//! matter for the incident this rewrite fixes -- above all `.trash/`, since
//! that's exactly where the old watcher's misfire pointed.
//!
//! # Coalescing
//!
//! A burst of events (a sync client delivering several files back-to-back,
//! or a single folder move producing separate old-path/new-path events) is
//! collected for a sliding `COALESCE_WINDOW` quiet period and reduced to a
//! `HashSet` of distinct session ids before any `refresh_session` call is
//! made -- one refresh per session per burst, not one per raw path.
//!
//! # Startup ordering
//!
//! Wired from `lib.rs`'s app-level `setup()` closure, after the session
//! store is constructed and `.manage()`d and its startup `rebuild_index`
//! pass has completed, and after `vault_export::spawn` -- see `lib.rs`'s
//! comments at the `vault_watch::spawn` call site for the full ordering
//! rationale (a live edit only has to account for vault state from here
//! on).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_notify::FileChanged;
use tauri_specta::Event;

use crate::session_store::SessionStore;

/// How long to wait for the *next* `FileChanged` event before treating a
/// burst of external edits as finished and refreshing everything seen so
/// far. Sliding quiet-window, not a fixed tick: `run`'s inner loop resets a
/// fresh `COALESCE_WINDOW` timeout after every event it receives, so an
/// active, ongoing burst keeps extending the wait rather than being cut off
/// at a fixed mark from the first event. Wider than `plugins/notify`'s own
/// ~900ms internal debounce (which only coalesces raw filesystem events into
/// one `FileChanged` emission per path) because a sync client or an
/// editor's save-then-touch-metadata sequence can still spread related
/// events across a couple of emissions a few hundred ms apart.
const COALESCE_WINDOW: Duration = Duration::from_secs(2);

/// The only two things a vault-watch event can lead to. No delete/hide verb
/// exists here on purpose -- `Refresh` is the sole action, and
/// `refresh_session` itself (not this module) is what decides whether a
/// missing file means removing index rows.
#[derive(Debug, PartialEq, Eq)]
pub enum WatchAction {
    Ignore,
    Refresh(String),
}

/// Pure routing decision: given a vault-relative path and whether its
/// current on-disk bytes match this store's own last write there, decide
/// what (if anything) the watcher should do.
///
/// `journal_match` is checked first and unconditionally wins -- an own
/// write is ignored no matter how the path would otherwise classify. Only
/// after that does path shape matter: anything under `sessions/<id>/...`
/// (or `sessions/<id>` itself) that isn't own-write is a `Refresh` for that
/// id, whether the change is an edit, a create, or a delete (this function
/// never inspects the filesystem, so "deleted" and "edited" look identical
/// to it -- `refresh_session` is what tells them apart). Everything else --
/// non-session paths, `.trash/`, this app's own bookkeeping files, in-flight
/// atomic-write temp files -- is `Ignore`.
pub fn classify_event(relative: &str, journal_match: bool) -> WatchAction {
    if journal_match {
        return WatchAction::Ignore;
    }

    if is_ignored_relative_path(relative) {
        return WatchAction::Ignore;
    }

    match session_id_for_relative_path(relative) {
        Some(id) => WatchAction::Refresh(id),
        None => WatchAction::Ignore,
    }
}

/// Extracts `<id>` from a `sessions/<id>` or `sessions/<id>/...` relative
/// path. `None` for anything else, including the bare `sessions` root
/// itself (nothing session-specific to refresh) and empty segments.
fn session_id_for_relative_path(relative: &str) -> Option<String> {
    let mut segments = relative.split('/');
    if segments.next() != Some("sessions") {
        return None;
    }
    let id = segments.next()?;
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

fn is_trash_path(relative: &str) -> bool {
    relative == ".trash" || relative.starts_with(".trash/")
}

/// This app's own SQLite database file(s) -- normally live outside the
/// vault entirely (`db.rs`'s `desktop_db_dir`), but guarded here too in
/// case a deployment ever points the db path inside the vault.
fn is_app_db_path(relative: &str) -> bool {
    relative
        .split('/')
        .next()
        .is_some_and(|top| top.starts_with("app.db"))
}

/// Legacy location only: the search index now lives in the app-data dir,
/// not the vault (mirrors `plugins/notify/src/path.rs`'s `should_skip_path`
/// guard for the same stale-leftover case).
fn is_search_index_path(relative: &str) -> bool {
    relative == "search_index" || relative.starts_with("search_index/")
}

fn is_agents_md_path(relative: &str) -> bool {
    relative == "AGENTS.md"
}

fn is_export_marker_path(relative: &str) -> bool {
    relative == crate::vault_export::EXPORT_MARKER_FILENAME
}

/// `hypr_fs_sync_core::export::tmp_sibling_path` names atomic-write temp
/// files `.tmp-{pid}-{nonce}-{original_name}` as a *sibling* of the real
/// file, so the pattern can appear either as the whole basename (a
/// top-level tmp file) or as a `.`-prefixed segment ahead of another dotted
/// name -- `contains` catches both without needing to anchor at the start.
/// `plugins/notify`'s own `should_skip_path` already filters basenames that
/// simply *start* with `.tmp` before a `FileChanged` is even emitted; this
/// is a second, independent check on the exact pattern this app's own
/// writers use, kept here so this module's correctness doesn't depend on
/// that upstream filter either.
fn has_tmp_write_basename(relative: &str) -> bool {
    relative
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains(".tmp-"))
}

/// Everything this module ignores independent of the journal check -- see
/// the module doc's "What 'external' means" section for why each rule
/// exists.
fn is_ignored_relative_path(relative: &str) -> bool {
    is_trash_path(relative)
        || is_app_db_path(relative)
        || is_search_index_path(relative)
        || is_agents_md_path(relative)
        || is_export_marker_path(relative)
        || has_tmp_write_basename(relative)
}

/// Runs `classify_event` for every path in a coalesced batch against the
/// store's real journal, returning the distinct set of session ids that
/// need a refresh. Factored out from `run` so it's directly testable
/// against a real `SessionStore` (see the `real_journal_end_to_end` tests
/// below) without needing a live FSEvents stream.
async fn ids_to_refresh(store: &SessionStore, changed: &HashSet<String>) -> HashSet<String> {
    let mut ids = HashSet::new();
    for relative in changed {
        let journal_match = store.journal_matches_current_file(relative);
        if let WatchAction::Refresh(id) = classify_event(relative, journal_match) {
            ids.insert(id);
        }
    }
    ids
}

/// Refreshes every id in `ids`, one at a time. A failure on one session is
/// logged and never aborts the rest, and never crashes the watcher loop --
/// `refresh_session` failures are typically transient I/O (see its own
/// doc), and the next `FileChanged` burst for the same session (or the next
/// window-focus rescan) will simply retry.
async fn refresh_ids(store: &SessionStore, ids: HashSet<String>) {
    for id in ids {
        match store.refresh_session(&id).await {
            Ok(()) => {
                tracing::info!(session_id = %id, "vault watch: refreshed session index from external change");
            }
            Err(error) => {
                tracing::warn!(session_id = %id, %error, "vault watch: failed to refresh session index");
            }
        }
    }
}

async fn handle_batch(store: &SessionStore, changed: &HashSet<String>) {
    let ids = ids_to_refresh(store, changed).await;
    refresh_ids(store, ids).await;
}

pub fn spawn(app: AppHandle) {
    let Some(store) = app
        .try_state::<Arc<SessionStore>>()
        .map(|state| state.inner().clone())
    else {
        tracing::error!(
            "vault watch: session store is not managed; external-edit ingestion is disabled"
        );
        return;
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    FileChanged::listen(&app, move |event| {
        let _ = tx.send(event.payload.path);
    });

    tauri::async_runtime::spawn(async move {
        run(store, rx).await;
    });
}

async fn run(
    store: Arc<SessionStore>,
    mut changed_paths: tokio::sync::mpsc::UnboundedReceiver<String>,
) {
    loop {
        let Some(first) = changed_paths.recv().await else {
            break;
        };
        let mut changed = HashSet::from([first]);

        loop {
            match tokio::time::timeout(COALESCE_WINDOW, changed_paths.recv()).await {
                Ok(Some(path)) => {
                    changed.insert(path);
                }
                Ok(None) => break,
                Err(_elapsed) => break,
            }
        }

        handle_batch(&store, &changed).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::SessionMeta;

    // -- the four brief-mandated routing tests --

    #[test]
    fn own_write_is_ignored_even_if_late() {
        assert!(matches!(
            classify_event("sessions/s1/_memo.md", true),
            WatchAction::Ignore
        ));
    }

    #[test]
    fn external_session_edit_refreshes() {
        assert!(matches!(
            classify_event("sessions/s1/_meta.json", false),
            WatchAction::Refresh(id) if id == "s1"
        ));
    }

    #[test]
    fn deleted_meta_is_still_only_a_refresh() {
        // refresh_session handles absence by removing index rows; watcher has no delete verb
        assert!(matches!(
            classify_event("sessions/s1/_meta.json", false),
            WatchAction::Refresh(_)
        ));
    }

    #[test]
    fn non_session_paths_ignored() {
        assert!(matches!(
            classify_event("AGENTS.md", false),
            WatchAction::Ignore
        ));
        assert!(matches!(
            classify_event(".trash/2026-07-24/sessions/s1", false),
            WatchAction::Ignore
        ));
    }

    // -- additional routing coverage --

    #[test]
    fn bare_session_folder_path_refreshes() {
        assert!(matches!(
            classify_event("sessions/s1", false),
            WatchAction::Refresh(id) if id == "s1"
        ));
    }

    #[test]
    fn bare_sessions_root_is_ignored() {
        assert!(matches!(
            classify_event("sessions", false),
            WatchAction::Ignore
        ));
    }

    #[test]
    fn app_db_paths_are_ignored() {
        assert!(matches!(
            classify_event("app.db", false),
            WatchAction::Ignore
        ));
        assert!(matches!(
            classify_event("app.db-wal", false),
            WatchAction::Ignore
        ));
    }

    #[test]
    fn search_index_paths_are_ignored() {
        assert!(matches!(
            classify_event("search_index/abc.term", false),
            WatchAction::Ignore
        ));
    }

    #[test]
    fn export_marker_path_is_ignored() {
        assert!(matches!(
            classify_event(crate::vault_export::EXPORT_MARKER_FILENAME, false),
            WatchAction::Ignore
        ));
    }

    #[test]
    fn tmp_write_paths_under_a_session_are_ignored() {
        assert!(matches!(
            classify_event("sessions/s1/.tmp-1234-5678-_memo.md", false),
            WatchAction::Ignore
        ));
    }

    #[test]
    fn journal_match_wins_over_an_otherwise_refreshable_path() {
        // Same path as external_session_edit_refreshes, but own-write this time.
        assert!(matches!(
            classify_event("sessions/s1/_meta.json", true),
            WatchAction::Ignore
        ));
    }

    // -- integration: real journal, real SessionStore, no live FSEvents stream --

    fn meta(id: &str, title: &str) -> SessionMeta {
        SessionMeta {
            id: id.to_string(),
            title: title.to_string(),
            started_at: None,
            ended_at: None,
            created_at: "2026-07-24T00:00:00Z".to_string(),
            tags: vec![],
        }
    }

    async fn test_store() -> (SessionStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().to_path_buf();
        let db = hypr_db_core::Db::connect_memory_plain().await.unwrap();
        hypr_db_app::prepare_schema(&db).await.unwrap();
        let store = SessionStore::new(vault, db.pool().clone());
        (store, temp)
    }

    #[tokio::test]
    async fn real_journal_end_to_end_own_write_is_ignored() {
        let (store, _vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "hello").await.unwrap();

        // Simulate the FileChanged event vault_watch would receive for its own note write.
        let changed = HashSet::from(["sessions/s1/_memo.md".to_string()]);
        let ids = ids_to_refresh(&store, &changed).await;

        assert!(
            ids.is_empty(),
            "own write must never be queued for refresh, even though nothing marked it upstream"
        );
    }

    #[tokio::test]
    async fn real_journal_end_to_end_external_edit_is_queued_for_refresh() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();

        // Bypass write_meta entirely -- an external editor/sync client would too.
        std::fs::write(
            vault.path().join("sessions/s1/_meta.json"),
            serde_json::to_vec_pretty(&meta("s1", "Edited outside")).unwrap(),
        )
        .unwrap();

        let changed = HashSet::from(["sessions/s1/_meta.json".to_string()]);
        let ids = ids_to_refresh(&store, &changed).await;

        assert_eq!(ids, HashSet::from(["s1".to_string()]));
    }

    #[tokio::test]
    async fn real_journal_end_to_end_deleted_file_is_still_queued_and_refresh_clears_the_index() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();
        store.write_note("s1", "keep me").await.unwrap();
        std::fs::remove_file(vault.path().join("sessions/s1/_meta.json")).unwrap();

        let changed = HashSet::from(["sessions/s1/_meta.json".to_string()]);
        let ids = ids_to_refresh(&store, &changed).await;
        assert_eq!(ids, HashSet::from(["s1".to_string()]));

        handle_batch(&store, &changed).await;

        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id='s1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0, "index row must be gone");
        assert!(
            vault.path().join("sessions/s1/_memo.md").is_file(),
            "the watcher must never touch files -- only the index row is affected"
        );
    }

    #[tokio::test]
    async fn handle_batch_collapses_multiple_paths_for_the_same_session_into_one_refresh() {
        let (store, vault) = test_store().await;
        store.write_meta(&meta("s1", "One")).await.unwrap();

        std::fs::write(
            vault.path().join("sessions/s1/_meta.json"),
            serde_json::to_vec_pretty(&meta("s1", "Edited outside")).unwrap(),
        )
        .unwrap();
        std::fs::write(vault.path().join("sessions/s1/other.md"), b"note").unwrap();

        let changed = HashSet::from([
            "sessions/s1/_meta.json".to_string(),
            "sessions/s1/other.md".to_string(),
        ]);
        let ids = ids_to_refresh(&store, &changed).await;

        assert_eq!(ids, HashSet::from(["s1".to_string()]));
    }
}
