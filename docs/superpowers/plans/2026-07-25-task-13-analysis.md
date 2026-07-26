# Task 13 analysis: superset schema + key-facts/meeting-chat removal

Investigation only — no code changed. Ground truth verified by reading every raw-SQL
FE file touching `sessions`/`session_documents`/`transcripts`, plus
`crates/db-app/src/session_ops.rs`/`session_types.rs` (feeds `crates/agent-access`'s
MCP `list_meetings`/`get_meeting`/`get_meeting_transcript`) and
`apps/desktop/src-tauri/src/search_index.rs` (Tantivy projection worker) — neither of
the latter two is in Task 13's deletion list, so both survive and both read a wider
column set than the session-store's own Task 6-8 write paths.

## 1. Usage verdicts

### `sessions`

| column | writer | reader | verdict |
|---|---|---|---|
| `id,title,started_at,ended_at,created_at,updated_at` | session_store (content.rs) | everywhere | **keep** — store-owned |
| `owner_user_id` | nothing (store never sets it; "single-user" per Task 9 note) | `shared/owner-user.ts` (self-speaker ID), `meeting-float/hooks.ts`, `services/event-listeners.tsx` (live-capture identity), `stt/queries.ts` (transcript owner), `session/queries.ts` (`SessionRecord.user_id`, and copied into `session_documents.created_by/updated_by` on insert), `session_ops.rs` | **keep** — read by 4+ live FE hooks even though always empty today; dropping breaks them at the SQL level |
| `event_json` | `session/queries.ts::createSession`/`updateSession` (`SessionChanges.event_json`) | `onboarding/welcome-note.ts` (tracks the welcome note by `tracking_id`), `stt/useKeywords.ts` (keyword extraction), `session/insights/past-notes.ts` (recurrence/series matching — feature currently unmounted, see §2 note), `search_index.rs` (search-result timestamp derivation) | **keep** — real writer + real readers, incl. one outside the FE (search index). Also: Task 2 of this same plan already decided to keep it ("generic metadata envelope (real non-calendar callers)") |
| `folder_path` | `session/queries.ts::updateSession` (`SessionChanges.folder_id`) | `sidebar/timeline/queries.ts` (sidebar folder grouping), `session/queries.ts` (`SessionRecord.folder_id`) | **keep** — live, independent "organize into folders" feature, unrelated to key-facts/meeting-chat. `crates/fs-sync-core`/`plugins/fs-sync` also has a whole `move_session`/`rename_folder` subsystem behind this. Longer-term this probably belongs in `_meta.json` (file-canonical, like title), not a bare DB column — but that's a real design task, not a Task 13 mechanical fix. Flag for later, keep column now. |
| `deleted_at` | nothing anymore (old import/watch soft-hide dies in Task 13; `session_delete` is a hard `DELETE` + trash-move since Task 8/9) | every query, defensively | **drop** — this one really is dead weight post-Task-13, matches the brief's original intent. Migration must **not** carry rows forward where `deleted_at IS NOT NULL` (see §3) so an already-soft-hidden session doesn't un-hide itself. |
| `kind,status,timezone,language,external_provider,source_apps_json,slug,metadata_json` | nothing found anywhere (not in session_store's INSERT, not in any FE write) | only `session_ops.rs`'s generic `SESSION_COLUMNS`/`SESSION_LIST_COLUMNS` (→ `crates/agent-access` MCP tools) | **keep-for-now, functionally dead** — always at table DEFAULT. Real cleanup means trimming `session_ops.rs`'s SELECT + `SessionRow`/`SessionListItem` structs + `agent-access`'s mapping, which is out of scope for a "keep it safe" Task 13. Note `metadata_json` was also the old external-soft-hide flag's home (`legacy_import.rs`, dying) — no other consumer. |
| `workspace_id,event_id,external_event_id,series_id` | nothing | nothing (not even `session_ops.rs` selects these) | **fully dead, drop freely** — zero readers anywhere, unlike the bucket above |

### `session_documents`

| column | writer | reader | verdict |
|---|---|---|---|
| `id,session_id,kind,body_format,body,updated_at` | store | everywhere | **keep** — store-owned |
| `title` | `services/enhancer/storage.ts` (`ensureSummaryDocument`/`replaceSummaryDocumentTemplate`/`updateSummaryDocumentTitleIfCurrent`) | `session/queries.ts` (`useEnhancedNoteRecords`/`useEnhancedNote`) | **keep** — live "AI summary" feature (kind `summary`/`template_output`), independent of key-facts/meeting-chat |
| `template_id` | same enhancer/storage.ts flows | same, `EnhancedNoteRecord.templateId` | **keep** — ties a summary to the Templates feature (`crates/db-app/template_ops.rs`), unrelated to key-facts |
| `sort_order` | same enhancer/storage.ts flows | `session/queries.ts` (`ORDER BY sort_order`, `EnhancedNoteRecord.position`) | **keep** — summary ordering |
| `created_by,updated_by` | `session/queries.ts::createEmptyNoteStatement` (placeholder note row), `services/enhancer/storage.ts` (both echo `owner_user_id`, itself always empty) | not meaningfully read anywhere (only `past-notes.ts` reads `created_by`, and that's dropping) | **keep** — vestigial values but two surviving INSERT/UPDATE statements target them; dropping means editing those two files too. Low priority follow-up. |
| `source_hash,generation_metadata_json` | **only** `stt/meeting-chat-records.ts` and `session/insights/past-notes.ts` (key-facts) | `session_ops.rs`'s generic `SESSION_DOCUMENT_COLUMNS` only | **safe to drop**, but only *together with* trimming `session_ops.rs`'s SELECT + `SessionDocumentRow` struct (2 fields) + wherever `crates/agent-access` maps that struct — do this in the same pass as the key-facts/meeting-chat removal so nothing selects a column that no longer exists. This is the one place where "drop key-facts/meeting-chat" and "shrink the schema" are the same edit. |
| `deleted_at` | `session/queries.ts::deleteEnhancedNote` (summary soft-delete), `store/zustand/ai-task/task-configs/enhance-success.ts` (shadow-row hide) | everywhere (COALESCE note-fallback pattern, `useEnhancedNoteRecords`, `session_ops.rs`) | **keep** — active tombstone flag for a feature independent of key-facts/meeting-chat. These synthetic index-only rows have no file home to fall back on. |

### `transcripts`

| column | writer | reader | verdict |
|---|---|---|---|
| `id,session_id,started_at_ms,memo,words_json,speaker_hints_json,updated_at` | store | everywhere | **keep** — store-owned |
| `owner_user_id` | nothing (always empty) | `stt/queries.ts` (`TRANSCRIPT_COLUMNS` → `TranscriptRecord.ownerUserId` → `getSelfHumanId` in the transcript label/speaker-highlight context) | **keep** — same "read but store never writes it" situation as `sessions.owner_user_id` |
| `ended_at_ms` | nothing (store's INSERT list has no `ended_at_ms`) | `stt/queries.ts` (`TRANSCRIPT_COLUMNS`, mapped to `TranscriptRecord.endedAt`) | **keep** — read by a live hook even though always NULL today |
| `deleted_at` | `stt/queries.ts::createTranscript` with `replaceSession: true` (supersede-on-batch-rerun: tombstones every other transcript row for the session) | `stt/queries.ts` everywhere, `session_ops.rs` | **keep** — active mechanism, the store has no "replace whole transcript set" primitive of its own |
| `source,provider,model,language,audio_attachment_id,metadata_json` | nothing (`stt/queries.ts::createTranscript`'s own comment: "accepted for caller compatibility but not yet persisted — the store... has no columns for them") | only `session_ops.rs`'s generic `SESSION_TRANSCRIPT_COLUMNS` | **keep-for-now, functionally dead** — same bucket as the `sessions` MCP-only group above |

## 2. Drop inventory: key-facts + meeting-chat

Both are being removed like calendar/contacts before them: UI, flows, and DB rows, no
file-backed home needed.

### Key-facts ("past session notes" insight)
- **`apps/desktop/src/session/insights/past-notes.ts`** — the entire feature: `usePastSessionNotes`, `PastSessionNote*` types, `buildPastSessionNotes`, `generateAndSavePastSessionNotes`, `buildSessionKeyFactsStatements`, `generatePastSessionKeyFacts`.
- Jinja templates: `apps/desktop/src/session/insights/past-note-key-facts.system.md.jinja`, `...user.md.jinja` (confirm exact path/siblings at implementation time).
- Tests: `past-notes.test.ts`, `past-notes.test.tsx`.
- **DB footprint:** `session_documents` rows with `kind = 'key_facts'` (id `${sessionId}:key_facts`, one per session).
- **IMPORTANT — appears already unmounted:** exhaustive grep for `usePastSessionNotes`/`PastSessionNote`/`past-notes` across `apps/desktop/src` found **zero consumers outside the module's own two test files** — no component renders this hook today. Verify with one more targeted search (dynamic import / re-export) before deleting, but if confirmed, this is a pure dead-code removal, not a UI-surgery task.
- Also reads (but nothing writes) `session_documents` rows with `kind = 'enhanced_note'` (distinct from the *live* `summary`/`template_output` kinds used by the real "AI summary" feature) — this looks like a pre-existing dead/never-wired read path inside an already-unmounted feature; not worth chasing further, just delete it with the rest of the file.
- **No search-index touchpoint** (`search_index.rs` never queries `kind = 'key_facts'`).
- **No MCP touchpoint**: `crates/agent-access/src/lib.rs:213` already filters returned documents to `matches!(kind, "summary" | "template_output")`, so `key_facts` rows were never exposed there.

### Meeting-chat capture
- **`apps/desktop/src/stt/meeting-chat-records.ts`** — `useMeetingChatRecords`, `loadMeetingChatRecords`, `persistMeetingChatRecords`, `parseMeetingChatDocument`, `formatMeetingChatRecordsAsMarkdown`, `formatMeetingChatContext`.
- **`apps/desktop/src/stt/meeting-chat-capture.ts`** — `startMeetingChatCapture` (polls `@hypr/plugin-detect` every 5s during a live recording).
- **`apps/desktop/src/session/components/note-input/meeting-chat-highlights.tsx`** — the mounted UI (rendered from `session/components/note-input/raw.tsx`); has its own test file.
- **Wiring points to unpick:**
  - `apps/desktop/src/stt/useStartListening.ts` calls `startMeetingChatCapture` when recording starts.
  - `apps/desktop/src/store/zustand/ai-task/task-configs/enhance-transform.ts` folds `formatMeetingChatContext(await loadMeetingChatRecords(sessionId))` into the AI-enhance prompt context — must be removed from the prompt-building path, not just left calling a deleted module.
  - Settings toggle `capture_meeting_chat`: `apps/desktop/src/settings/schema.ts`, `apps/desktop/src/settings/general/index.tsx`.
  - Rust side: `plugins/detect/src/commands.rs`'s `send_meeting_chat_message`/`capture_meeting_chat_messages` commands and the `MeetingCapturedChatMessage` type are meeting-chat-specific — but **`plugins/detect` is a much broader plugin** (app detection, mic-usage tracking, DND, accessibility inspection, locale) used for unrelated live-listening features. Only remove the two chat-specific commands + type, not the plugin.
- **DB footprint:** `session_documents` rows with `kind = 'meeting_chat'` (id `${sessionId}:meeting-chat:${sourceHash}`, multiple per session).
- **Search-index touchpoint (real, must fix):** `apps/desktop/src-tauri/src/search_index.rs::build_session_document` runs a dedicated query — `SELECT body FROM session_documents WHERE session_id = ? AND kind = 'meeting_chat' AND deleted_at IS NULL` — and folds the results into the indexed document's `content_parts`. Remove that query + its contribution, and **bump `PROJECTION_VERSION`** (currently `3`) so existing Tantivy indexes rebuild without the stale meeting-chat content.
- **MCP touchpoint:** none — same `agent-access` filter as key-facts already excludes `meeting_chat` kind.

## 3. Proposed superset DDL + migration sketch

Keeps every column verdicted "keep" above; drops `sessions.deleted_at` (+ the dead
`workspace_id`/`event_id`/`external_event_id`/`series_id`) and
`session_documents.source_hash`/`generation_metadata_json` (contingent on the
`session_ops.rs` trim happening in the same commit). `session_documents.deleted_at`
and `transcripts.deleted_at` are **kept** — the one deviation from the original
brief's "drop deleted_at everywhere" instruction, justified per-table above.

```sql
-- 1. Copy surviving data into new tables (excludes key_facts/meeting_chat rows and
--    anything already soft-deleted, since deleted_at leaves the sessions table).
CREATE TABLE sessions_new (
  id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL DEFAULT '',
  started_at TEXT, ended_at TEXT,
  created_at TEXT NOT NULL DEFAULT '', updated_at TEXT NOT NULL DEFAULT '',
  owner_user_id TEXT NOT NULL DEFAULT '', event_json TEXT NOT NULL DEFAULT '',
  folder_path TEXT NOT NULL DEFAULT '',
  kind TEXT NOT NULL DEFAULT 'meeting', status TEXT NOT NULL DEFAULT 'active',
  timezone TEXT NOT NULL DEFAULT '', language TEXT NOT NULL DEFAULT '',
  external_provider TEXT NOT NULL DEFAULT '', source_apps_json TEXT NOT NULL DEFAULT '[]',
  slug TEXT NOT NULL DEFAULT '', metadata_json TEXT NOT NULL DEFAULT '{}'
);
INSERT INTO sessions_new
  SELECT id, title, started_at, ended_at, created_at, updated_at,
         owner_user_id, event_json, folder_path,
         kind, status, timezone, language, external_provider, source_apps_json,
         slug, metadata_json
  FROM sessions WHERE deleted_at IS NULL;
DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;

CREATE TABLE session_documents_new (
  id TEXT PRIMARY KEY NOT NULL, session_id TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'note', title TEXT NOT NULL DEFAULT '',
  template_id TEXT NOT NULL DEFAULT '',
  body_format TEXT NOT NULL DEFAULT 'md', body TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_by TEXT NOT NULL DEFAULT '', updated_by TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL DEFAULT '', deleted_at TEXT
);
INSERT INTO session_documents_new
  SELECT id, session_id, kind, title, template_id, body_format, body, sort_order,
         created_by, updated_by, updated_at, deleted_at
  FROM session_documents WHERE kind NOT IN ('key_facts', 'meeting_chat');
DROP TABLE session_documents;
ALTER TABLE session_documents_new RENAME TO session_documents;

CREATE TABLE transcripts_new (
  id TEXT PRIMARY KEY NOT NULL, session_id TEXT NOT NULL,
  started_at_ms REAL NOT NULL DEFAULT 0, ended_at_ms REAL,
  memo TEXT NOT NULL DEFAULT '', words_json TEXT NOT NULL DEFAULT '[]',
  speaker_hints_json TEXT NOT NULL DEFAULT '[]',
  owner_user_id TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL DEFAULT '', deleted_at TEXT
);
INSERT INTO transcripts_new
  SELECT id, session_id, started_at_ms, ended_at_ms, memo, words_json,
         speaker_hints_json, owner_user_id, updated_at, deleted_at
  FROM transcripts;
DROP TABLE transcripts;
ALTER TABLE transcripts_new RENAME TO transcripts;

-- 2. Old sync-machinery tables, unconditional per the original brief.
DROP TABLE IF EXISTS vault_export_dirty;
DROP TABLE IF EXISTS migration_import_runs;
DROP TABLE IF EXISTS migration_import_items;
DROP TABLE IF EXISTS migration_import_targets;
DROP TABLE IF EXISTS storage_migration_state;

-- 3. Recreate the search-index triggers (bodies copied verbatim from
--    20260714120100/120200/120300 — DROP TABLE already dropped the old ones
--    automatically, confirmed empirically: SQLite drops a table's triggers
--    with it, no explicit DROP TRIGGER needed first).
-- ... (search_index_sessions_insert/update/delete, _session_documents_*, _transcripts_*)
```

Notes on the sketch:
- `ALTER TABLE ... RENAME TO` keeps `sqlite_master` clean and avoids re-declaring
  indexes separately; adjust if the real migration prefers explicit `CREATE INDEX`
  statements afterward (the brief's original DDL had none beyond the old
  `canonical_data_model.sql` indexes, which also get dropped with their tables —
  worth re ­adding `idx_sessions_created_at`/`idx_session_documents_session_id`/
  `idx_transcripts_session_id` etc. since those aren't sync-machinery, they're
  plain read-performance indexes).
- Whoever implements this must also, **in the same commit**: trim
  `crates/db-app/src/session_ops.rs`'s `SESSION_DOCUMENT_COLUMNS` (drop
  `source_hash`, `generation_metadata_json`) and `SessionDocumentRow` in
  `session_types.rs`, and whatever `crates/agent-access` does with those two
  fields — otherwise the recreated table is missing columns those queries
  reference and `list_session_documents`/`get_session_note` error at runtime.
- FE `deleted_at` filters to remove: only the ones touching `sessions` (per the
  original brief's grep). Every `session_documents`/`transcripts` `deleted_at`
  reference **stays** — do not run a blanket `grep -rn deleted_at apps/desktop/src`
  removal; that was the brief's original (now-superseded) instruction and would
  break the summary-soft-delete and transcript-supersede paths documented in §1.

## 4. Other findings for the implementer

- **fs-sync-core orphans** (zero remaining callers anywhere once `vault_export.rs` +
  `plugins/db/src/import/` + `plugins/db/tests/vault_export_round_trip.rs` are
  deleted — controller decision 7 says leave these alone in Task 13, sweep in
  Task 14): `render_session_meta`, `session_document_filename`,
  `render_session_document`, `render_transcripts` (crates/fs-sync-core's own
  version — `crates/agent-access` has an unrelated same-named *private* fn, not a
  real consumer), `render_human`, `render_organization`, `render_calendars`,
  `render_events`, `render_daily_notes`, `render_tasks`, `render_chat`,
  `render_settings`, and their backing structs (`SessionMeta`, `SessionParticipant`,
  `SessionKeyFacts`, `SessionDocument`, `Transcript`, `Human`, `Organization`,
  `Calendar`, `CalendarEvent`, `DailyNote`, `ActionItem`, `ChatGroup`,
  `ChatMessage`) in `crates/fs-sync-core/src/export.rs`. Kept alive today only by
  each other's own `#[cfg(test)]` unit tests in that same file — production-dead.
  `tmp_sibling_path`, `write_file_atomic`, `move_to_trash` in the same file are
  **not** orphaned (still used by `session_store` and `plugins/fs-sync`).
- **`sessions.deleted_at` was never re-set by anything current**: the old
  external-soft-hide path (`import_paths`/pre-Task-11 `vault_watch`) is what used
  to write it, and it's fully retired. `session_delete` (Task 8/9) hard-deletes the
  index rows and moves the folder to `.trash/` instead. Confirmed empirically that
  `DROP TABLE` auto-drops triggers defined on that table in SQLite (no explicit
  `DROP TRIGGER` needed before dropping `sessions`/`session_documents`/`transcripts`).
- **`crates/e2ee`** confirmed zero consumers (workspace member + root `Cargo.toml`
  `hypr-e2ee` alias only) — safe to delete as originally planned, independent of
  everything above.
- **`workspace_id`** still physically exists as a column on `sessions` in the
  current schema (never dropped by Task 3/4's migrations, which only dropped the
  `workspaces`/`workspace_memberships` *tables*) but has zero readers anywhere,
  not even `session_ops.rs`'s generic projection — cleanest possible drop.
