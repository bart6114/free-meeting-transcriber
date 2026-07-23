//! External-edit ingestion: vault file watcher -> DB import (Task 14).
//!
//! `plugins/notify` already recursively watches `vault_base` (debounced
//! ~900ms) and emits a `FileChanged { path }` event for every changed path
//! that isn't one of its own own-writes (`is_external_path` in
//! `plugins/notify/src/ext.rs` — filtered *before* the event is ever
//! emitted, using the same `mark_own_writes` TTL the Task 13 export worker
//! writes into). This module is that event's first listener: it coalesces a
//! burst of such events for ~2s, filters out vault-internal/file-native
//! paths this app never wants to reinterpret as a DB source, and hands the
//! survivors to `tauri_plugin_db::import_paths` — a single-path-scoped
//! version of the same files-win reconcile `sync_from_vault` runs at
//! startup (see that module's doc for the full apply/conflict path).
//!
//! # Loop-prevention, restated from this side
//!
//! 1. An export write is marked own-write *before* it happens
//!    (`vault_export.rs`'s `write_tracked`/`trash_if_exists`), so
//!    `plugins/notify` never emits a `FileChanged` for it in the first
//!    place — this module never even sees it.
//! 2. Even if that mark were somehow missed, a byte-identical export write
//!    is a no-op (`write_file_atomic`'s content check) — no new mtime, no
//!    filesystem event, nothing for the watcher to report.
//! 3. Even if *both* of those somehow missed and a `FileChanged` reached
//!    this module anyway, `import_paths` short-circuits on a path whose
//!    current sha256 matches the last successfully imported hash for that
//!    exact path (`hypr_db_app::legacy_source_already_imported`) — no DB
//!    write, so no new dirty-queue entry, so no re-export.
//!
//! Since `import_paths` never itself writes vault files (it only reads them
//! and writes rows), there is no cycle back through this module at all —
//! the three links above only guard against *importing an export's own
//! output*, not a ping-pong between the two workers.
//!
//! # What happens to the file after an external edit is imported
//!
//! Importing a live edit writes DB rows straight from the file's bytes,
//! which (via the `vault_export_dirty` triggers) queues that entity for the
//! Task 13 export worker to re-render. That render is a *subset projection*
//! of the DB's own fields (title/body/kind/template_id/...), **not** a copy
//! of the file's bytes — it will not, in general, reproduce whatever exact
//! frontmatter shape, key order, or trailing whitespace the user's editor or
//! a sync client wrote. So the export worker commonly *does* rewrite the
//! file to its own canonical rendering, overwriting the user's original
//! on-disk bytes (still recoverable from the DB, just not necessarily
//! byte-for-byte as they sat on disk). **This is expected, not a bug.**
//! Safety against a live re-import ping-pong comes entirely from
//! `mark_own_writes` — called unconditionally before that write, regardless
//! of whether the render happens to come out byte-identical — so the
//! watcher never sees it as external and never re-triggers `import_paths`
//! for it. `write_file_atomic`'s skip-if-byte-identical behavior (link 2
//! above) is an optimization for the common *unchanged* case (editing body
//! text below intact frontmatter often *does* round-trip identically), not
//! the mechanism that prevents the loop in general — that's link 1's
//! own-write mark, unconditionally. Either way — rewritten or left alone —
//! the vault settles at a fixed point: file content equal to the DB's
//! canonical rendering, which any later check (another live edit, or the
//! next full `sync_from_vault` startup reconcile) sees as unchanged.
//!
//! # Ignore list
//!
//! Beyond whatever `classify_source` itself declines to classify (already
//! covered by `import_paths`'s own tests — `.conflict-*` backups, the
//! legacy `search_index/` location, anything outside the recognized
//! calendars/events/templates/sessions/humans/organizations/chats shapes),
//! this module explicitly ignores a few things ahead of even trying:
//! `.trash/`, `.tmp`-prefixed basenames, the export marker file, audio
//! files (file-native — Task 13 never exports/imports session audio, the
//! recording itself is the source of truth), and anything under an
//! `attachments/` directory (also file-native: attachment metadata is
//! written by the app's own upload flow, not meant to be re-derived from an
//! arbitrary file a user drops into that folder).
//!
//! # Startup ordering
//!
//! Wired from `lib.rs`'s app-level `setup()` closure, after
//! `vault_export::spawn` — which itself runs after the plugin-level
//! `sync_from_vault` reconcile (`tauri_plugin_db::init_with_cloudsync`'s
//! `setup()`, per Tauri's plugin-then-app lifecycle). So by the time this
//! module's listener is registered, `app.db` has already been reconciled
//! from whatever the vault held at launch, and the export worker has
//! already started draining anything left dirty from a prior session — a
//! live edit only has to account for what changes *from here on*.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_notify::FileChanged;
use tauri_plugin_settings::SettingsPluginExt;
use tauri_specta::Event;

/// How long to wait for the *next* `FileChanged` event before treating a
/// burst of external edits as finished and importing everything seen so
/// far. This is a **sliding quiet-window**, not a fixed tick: `run`'s inner
/// loop resets a fresh `COALESCE_WINDOW` timeout after every event it
/// receives (see the `tokio::time::timeout(COALESCE_WINDOW, ...)` call), so
/// a batch is only flushed once `COALESCE_WINDOW` has elapsed with *no*
/// new events — an active, ongoing burst (e.g. a sync client delivering
/// several files back-to-back) keeps extending the wait rather than being
/// cut off at a fixed 2s mark from the first event. Wider than
/// `plugins/notify`'s own ~900ms internal debounce (which only coalesces
/// raw filesystem events into one `FileChanged` emission per path) because
/// a sync client or an editor's save-then-touch-metadata sequence can still
/// spread related events across a couple of emissions a few hundred ms
/// apart.
const COALESCE_WINDOW: Duration = Duration::from_secs(2);

const AUDIO_EXTENSIONS: &[&str] = &["aac", "m4a", "mp3", "wav", "webm", "ogg"];

fn is_trash_path(relative_path: &str) -> bool {
    relative_path == ".trash" || relative_path.starts_with(".trash/")
}

fn is_conflict_backup_path(relative_path: &str) -> bool {
    relative_path.contains(".conflict-")
}

fn has_tmp_prefixed_basename(relative_path: &str) -> bool {
    relative_path
        .rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with(".tmp"))
}

fn is_export_marker_path(relative_path: &str) -> bool {
    relative_path == crate::vault_export::EXPORT_MARKER_FILENAME
}

fn is_audio_path(relative_path: &str) -> bool {
    Path::new(relative_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| AUDIO_EXTENSIONS.contains(&extension.as_str()))
}

fn is_attachments_path(relative_path: &str) -> bool {
    relative_path.split('/').any(|segment| segment == "attachments")
}

/// Everything the watcher should never hand to `import_paths`, checked
/// ahead of (and independent from) whatever `classify_source` itself would
/// reject — see the module doc for why each rule exists.
fn is_ignored_relative_path(relative_path: &str) -> bool {
    is_trash_path(relative_path)
        || is_conflict_backup_path(relative_path)
        || has_tmp_prefixed_basename(relative_path)
        || is_export_marker_path(relative_path)
        || is_audio_path(relative_path)
        || is_attachments_path(relative_path)
}

/// Filters a coalesced batch of vault-relative changed paths down to the
/// ones worth handing to `import_paths`, and maps the survivors to absolute
/// paths. Pure and synchronous — the seam this module's tests exercise,
/// since the actual event-listener/timer loop below is Wry-pinned the same
/// way Task 13's export worker loop is (see that module's tests for the
/// same tradeoff).
fn select_import_candidates(vault_base: &Path, changed: &HashSet<String>) -> Vec<PathBuf> {
    let mut candidates = changed
        .iter()
        .filter(|relative_path| !relative_path.is_empty() && !is_ignored_relative_path(relative_path))
        .map(|relative_path| vault_base.join(relative_path))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
}

fn vault_base_path<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.settings()
        .vault_base()
        .ok()
        .map(|base| base.as_std_path().to_path_buf())
}

pub fn spawn(app: AppHandle, db: Arc<hypr_db_core::Db>) {
    let Some(vault_base) = vault_base_path(&app) else {
        tracing::error!(
            "vault watch: could not resolve vault base; external-edit ingestion is disabled"
        );
        return;
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    FileChanged::listen(&app, move |event| {
        let _ = tx.send(event.payload.path);
    });

    tauri::async_runtime::spawn(async move {
        run(vault_base, db, rx).await;
    });
}

async fn run(
    vault_base: PathBuf,
    db: Arc<hypr_db_core::Db>,
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

        let candidates = select_import_candidates(&vault_base, &changed);
        if candidates.is_empty() {
            continue;
        }

        match tauri_plugin_db::import_paths(db.pool(), &vault_base, &candidates).await {
            Ok(report) => {
                tracing::info!(
                    imported = report.imported_count,
                    matched = report.matched_count,
                    conflicts = report.conflict_count,
                    reconciled = report.reconciled_count,
                    deleted = report.deleted_count,
                    "vault watch: imported external vault edits"
                );
            }
            Err(error) => {
                tracing::error!(%error, "vault watch: failed to import external vault edits");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_trash_paths() {
        assert!(is_ignored_relative_path(".trash"));
        assert!(is_ignored_relative_path(".trash/2026-07-23/sessions/abc/_meta.json"));
        assert!(!is_ignored_relative_path("sessions/abc/_meta.json"));
    }

    #[test]
    fn ignores_conflict_backup_paths() {
        assert!(is_ignored_relative_path(
            "sessions/abc/_memo.conflict-2026-07-23T12-00-00Z.md"
        ));
        assert!(!is_ignored_relative_path("sessions/abc/_memo.md"));
    }

    #[test]
    fn ignores_tmp_prefixed_basenames() {
        assert!(is_ignored_relative_path(".tmp6s1cca"));
        assert!(is_ignored_relative_path("sessions/abc/.tmpABC123"));
        assert!(!is_ignored_relative_path("sessions/abc/_meta.json"));
    }

    #[test]
    fn ignores_the_export_marker_file() {
        assert!(is_ignored_relative_path(
            crate::vault_export::EXPORT_MARKER_FILENAME
        ));
    }

    #[test]
    fn ignores_audio_files_case_insensitively() {
        for extension in ["aac", "m4a", "mp3", "wav", "webm", "ogg", "WAV", "M4A"] {
            let path = format!("sessions/abc/audio.{extension}");
            assert!(is_ignored_relative_path(&path), "expected {path} to be ignored");
        }
        assert!(!is_ignored_relative_path("sessions/abc/_meta.json"));
    }

    #[test]
    fn ignores_anything_under_an_attachments_directory() {
        assert!(is_ignored_relative_path(
            "sessions/abc/attachments/notes.pdf"
        ));
        assert!(!is_ignored_relative_path("sessions/abc/_meta.json"));
    }

    #[test]
    fn select_import_candidates_filters_dedupes_and_sorts() {
        let vault_base = Path::new("/vault");
        let changed = HashSet::from([
            "sessions/abc/_memo.md".to_string(),
            ".trash/2026-07-23/sessions/abc/_meta.json".to_string(),
            "sessions/abc/attachments/notes.pdf".to_string(),
            "sessions/abc/audio.wav".to_string(),
            "sessions/abc/_meta.json".to_string(),
            String::new(),
        ]);

        let candidates = select_import_candidates(vault_base, &changed);

        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/vault/sessions/abc/_memo.md"),
                PathBuf::from("/vault/sessions/abc/_meta.json"),
            ]
        );
    }

    #[test]
    fn select_import_candidates_is_empty_when_everything_is_ignored() {
        let vault_base = Path::new("/vault");
        let changed = HashSet::from([".trash/gone.json".to_string()]);

        assert!(select_import_candidates(vault_base, &changed).is_empty());
    }
}
