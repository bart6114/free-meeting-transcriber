# Filesystem-first session storage

**Date:** 2026-07-24
**Status:** Approved (design), pending implementation plan

## Motivation

On 2026-07-23 a recording's transcription was permanently lost. Root cause chain:

1. The vault export worker's own trash/rename activity on `sessions/<id>/_meta.json` was
   delivered by FSEvents after the 1.8 s own-write TTL expired, so the vault watcher
   classified it as an external edit.
2. The watcher read the momentarily-absent `_meta.json`, got `NotFound`, and soft-hid the
   session (`sessions.deleted_at`) — twice in one evening, while the file's content never
   changed.
3. With the session soft-deleted, `createTranscript`'s gated
   `INSERT INTO transcripts … SELECT … FROM sessions WHERE deleted_at IS NULL` inserted
   **0 rows with no error**. Every live-transcript delta after that was a silent no-op.
   The UI showed the in-memory transcript; nothing ever reached disk.

The structural problem is a half-migrated architecture: files are authoritative on read
(files-win reconcile at startup), but the only write path goes through SQLite and a
bidirectional sync (dirty-queue export + file watcher import). Two masters, loops in both
directions, and destructive verbs (soft-hide, trash) triggered by unverified observations.

Secondary latent bugs found in the same investigation (both die with this design):

- `render_transcripts` uses `unwrap_or_default()` on `words_json` — any unparseable word
  exports `words: []`, which the files-win reconcile then writes back over the DB.
- Int/float mismatch (`words_json` ints vs `TranscriptWord: f64`) makes every transcript a
  false "conflict" on every startup, so the files-win overwrite path runs constantly.

## Decisions (agreed with Bart)

| Decision | Choice |
| --- | --- |
| Scope | All session content (meta, note, summaries, transcript, recordings) filesystem-first in one project |
| Feature removals | Humans/orgs/contacts and calendar are dropped from the app entirely |
| Cloud cruft | cloudsync, e2ee, workspaces, session-sharing machinery ripped out |
| Note format | Markdown is canonical; `_memo.md` IS the note |
| External edits | Live watcher kept, but strictly index-only (non-destructive) |
| Storage architecture | Approach A: Rust-owned session store, single writer of files + derived SQLite index |

## Architecture

A new Rust module `apps/desktop/src-tauri/src/session_store/` is the **single owner** of
everything under `sessions/`. No other code — frontend, watcher, exporters — touches those
files. Every mutation is a Tauri command that:

1. writes the file atomically (tmp sibling + rename, `mkdir -p` semantics — writes have no
   preconditions and recreate the folder if needed), then
2. updates the derived index in SQLite through the same pool, so existing live-query
   subscriptions fire.

Files are the source of truth. `app.db` holds only config plus the rebuildable index:
deleting `app.db` and relaunching rebuilds it from the vault. That same one-way scan
(read files → upsert index) is the normal startup path, replacing `sync_from_vault`'s
conflict/reconcile machinery.

## Folder layout

```
sessions/<session-id>/
  _meta.json          # title, started/ended/created timestamps, tags
  _memo.md            # the note; editor loads from and saves to this file
  summary.md          # AI-enhanced summary (one file per generated document kind)
  transcript.json     # words + speaker labels (plain strings; no human_id links)
  audio/
    <started-at>.wav  # recordings; audio-retention policy deletes here
```

- Session existence = folder containing `_meta.json`.
- No `deleted_at` anywhere. Deleting a session is a user action only and moves the folder
  to `.trash/<date>/`.
- Speaker assignment becomes free-text rename stored in `transcript.json` speaker hints.
  No participants/contacts model.

## The index

Existing table names (`sessions`, `session_documents`, `transcripts`) are kept so frontend
read hooks and the search indexer keep working nearly unchanged, but:

- rows are written **only** by the session store (file → index, one direction);
- `workspace_id`, e2ee columns, and `deleted_at` are dropped by migration;
- the tables are documented as a derived mirror, rebuildable at any time.

## Write paths

- **Note:** editor autosave → `write_note(id, md)` → `_memo.md` + index row. On open the
  editor loads from the file, not the index.
- **Live transcript:** listener deltas → `append_transcript(id, delta)`. The store
  accumulates words in memory and flushes to `transcript.json` on a ~1 s debounce, with a
  forced flush on recording stop and on app exit. The flusher lives in Rust, so words
  survive a webview crash.
- **Meta/title, summaries, audio:** one command each; file first, index second.
- Every command returns a real error to the frontend. A write that persists nothing must
  be impossible to mistake for success. A word that fails to serialize is an error, never
  an empty array. Recording audio is retained whenever any transcript write errored.

## Watcher (index-only)

The live watcher reports to the session store. Rules:

- **Own-write filtering by journal, not TTL:** every store write records
  `(path, content-hash)`; an event whose file matches the journal is dropped regardless of
  FSEvents latency.
- **External change** → re-read that file → update the index row. This is the watcher's
  only verb.
- **Missing file** → remove the index row (the index mirrors the filesystem). The watcher
  never writes to the vault, never trashes, never touches other files. False alarms heal
  on the next event or rescan and cannot cascade: open editors keep their buffer, and a
  running recording keeps writing because store writes recreate the folder.
- Full rescan at startup and on window focus, so missed events self-correct.

## Deletions

- Bidirectional sync apparatus: `vault_export.rs` worker, `vault_export_dirty` table +
  triggers, `sync_from_vault` files-win conflict/reconcile, `import_paths` soft-hide and
  the `external_soft_hide` flag, trash cascades.
- Features: humans/orgs/contacts (tables, contacts UI, participant pickers,
  speaker→human assignment), calendar (plugin, `calendars`/`events` tables, meeting
  notifications).
- Cloud machinery: `cloudsync_*`, `e2ee_*`, `workspaces`, `workspace_memberships`,
  `session_share_*`, `shared_session_*` tables and code; attachment transfer jobs.
- `fs-format`/render helpers survive only as far as the store needs them to write
  `transcript.json`, with the `unwrap_or_default()` bug fixed.

## Migration

One-time, on first launch of the new build:

1. Final export sweep from the old DB tables to the vault, reusing today's renderers, so
   content never exported (e.g. transcripts affected by the int/float conflict bug) lands
   in files.
2. Drop dead tables/columns.
3. Rebuild the index from files.

## Testing

Rust-side integration tests where possible:

- write → file + index consistency; index rebuild idempotency (scan twice = identical
  index);
- watcher own-write journal (late event dropped); external edit refreshes index row;
  missing `_meta.json` hides the index row without touching any file;
- transcript debounce flush + forced flush on stop/exit;
- markdown round-trip stability for the editor;
- regression test for the incident: recording while the session's index row is absent
  still produces a complete `transcript.json`.
