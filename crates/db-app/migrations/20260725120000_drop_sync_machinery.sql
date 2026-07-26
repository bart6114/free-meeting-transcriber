-- Superset schema (Task 13): recreate sessions/session_documents/transcripts
-- without the sync-era columns, then drop the import/export bookkeeping tables.
-- SQLite drops a table's triggers and indexes with it, so the search-index
-- triggers and the plain read-performance indexes are recreated at the bottom
-- (trigger bodies verbatim from 20260714120100/120200/120300). No foreign-key
-- pragma juggling is needed: the canonical schema declares no FK constraints,
-- same as the other drop migrations (20260724100000/20260724110000).

-- sessions: drops workspace_id/event_id/external_event_id/series_id (zero
-- readers anywhere) and deleted_at (the old import/watch soft-hide is retired;
-- session_delete hard-deletes the row and moves the folder to .trash/).
-- Soft-hidden rows are not carried forward so they don't un-hide themselves.
CREATE TABLE sessions_new (
  id                 TEXT PRIMARY KEY NOT NULL,
  owner_user_id      TEXT NOT NULL DEFAULT '',
  title              TEXT NOT NULL DEFAULT '',
  kind               TEXT NOT NULL DEFAULT 'meeting',
  status             TEXT NOT NULL DEFAULT 'active',
  created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  started_at         TEXT NOT NULL DEFAULT '',
  ended_at           TEXT NOT NULL DEFAULT '',
  timezone           TEXT NOT NULL DEFAULT '',
  language           TEXT NOT NULL DEFAULT '',
  external_provider  TEXT NOT NULL DEFAULT '',
  source_apps_json   TEXT NOT NULL DEFAULT '[]',
  event_json         TEXT NOT NULL DEFAULT '',
  folder_path        TEXT NOT NULL DEFAULT '',
  slug               TEXT NOT NULL DEFAULT '',
  metadata_json      TEXT NOT NULL DEFAULT '{}'
) STRICT;

INSERT INTO sessions_new (
  id, owner_user_id, title, kind, status, created_at, updated_at,
  started_at, ended_at, timezone, language, external_provider,
  source_apps_json, event_json, folder_path, slug, metadata_json
)
SELECT
  id, owner_user_id, title, kind, status, created_at, updated_at,
  started_at, ended_at, timezone, language, external_provider,
  source_apps_json, event_json, folder_path, slug, metadata_json
FROM sessions
WHERE deleted_at IS NULL;

DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;

-- session_documents: drops workspace_id/source_hash/generation_metadata_json/
-- created_at (only the removed key-facts/meeting-chat writers used the hash and
-- generation metadata). deleted_at stays: it's the live tombstone for the
-- summary soft-delete path. key_facts/meeting_chat rows die with their feature.
CREATE TABLE session_documents_new (
  id           TEXT PRIMARY KEY NOT NULL,
  session_id   TEXT NOT NULL DEFAULT '',
  kind         TEXT NOT NULL DEFAULT 'note',
  template_id  TEXT NOT NULL DEFAULT '',
  title        TEXT NOT NULL DEFAULT '',
  body_format  TEXT NOT NULL DEFAULT 'prosemirror_json',
  body         TEXT NOT NULL DEFAULT '',
  sort_order   INTEGER NOT NULL DEFAULT 0,
  created_by   TEXT NOT NULL DEFAULT '',
  updated_by   TEXT NOT NULL DEFAULT '',
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  deleted_at   TEXT
) STRICT;

INSERT INTO session_documents_new (
  id, session_id, kind, template_id, title, body_format, body,
  sort_order, created_by, updated_by, updated_at, deleted_at
)
SELECT
  id, session_id, kind, template_id, title, body_format, body,
  sort_order, created_by, updated_by, updated_at, deleted_at
FROM session_documents
WHERE kind NOT IN ('key_facts', 'meeting_chat');

DROP TABLE session_documents;
ALTER TABLE session_documents_new RENAME TO session_documents;

-- transcripts: drops workspace_id/source/provider/model/language/
-- audio_attachment_id/metadata_json/created_at (never written; only the generic
-- MCP projection selected them). deleted_at stays: it's the live tombstone for
-- the supersede-on-batch-rerun path. All rows are carried forward.
CREATE TABLE transcripts_new (
  id                  TEXT PRIMARY KEY NOT NULL,
  owner_user_id       TEXT NOT NULL DEFAULT '',
  session_id          TEXT NOT NULL DEFAULT '',
  started_at_ms       INTEGER NOT NULL DEFAULT 0,
  ended_at_ms         INTEGER,
  memo                TEXT NOT NULL DEFAULT '',
  words_json          TEXT NOT NULL DEFAULT '[]',
  speaker_hints_json  TEXT NOT NULL DEFAULT '[]',
  updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  deleted_at          TEXT
) STRICT;

INSERT INTO transcripts_new (
  id, owner_user_id, session_id, started_at_ms, ended_at_ms, memo,
  words_json, speaker_hints_json, updated_at, deleted_at
)
SELECT
  id, owner_user_id, session_id, started_at_ms, ended_at_ms, memo,
  words_json, speaker_hints_json, updated_at, deleted_at
FROM transcripts;

DROP TABLE transcripts;
ALTER TABLE transcripts_new RENAME TO transcripts;

DROP TABLE IF EXISTS vault_export_dirty;
DROP TABLE IF EXISTS migration_import_runs;
DROP TABLE IF EXISTS migration_import_items;
DROP TABLE IF EXISTS migration_import_targets;
DROP TABLE IF EXISTS storage_migration_state;

-- 20260723150000_vault_export_dirty also hung vault_export_* triggers on tables
-- that survive this migration. Those triggers still INSERT INTO the
-- vault_export_dirty table just dropped, so every write to these tables would
-- fail if they were left behind. (The ones on sessions/session_documents/
-- transcripts died with the DROP TABLEs above; the ones on humans/organizations/
-- calendars/events/session_participants died in 20260724100000/20260724110000.)
DROP TRIGGER IF EXISTS vault_export_tags_insert;
DROP TRIGGER IF EXISTS vault_export_tags_update;
DROP TRIGGER IF EXISTS vault_export_tags_delete;
DROP TRIGGER IF EXISTS vault_export_session_tags_insert;
DROP TRIGGER IF EXISTS vault_export_session_tags_update;
DROP TRIGGER IF EXISTS vault_export_session_tags_delete;
DROP TRIGGER IF EXISTS vault_export_chat_groups_insert;
DROP TRIGGER IF EXISTS vault_export_chat_groups_update;
DROP TRIGGER IF EXISTS vault_export_chat_groups_delete;
DROP TRIGGER IF EXISTS vault_export_chat_messages_insert;
DROP TRIGGER IF EXISTS vault_export_chat_messages_update;
DROP TRIGGER IF EXISTS vault_export_chat_messages_delete;
DROP TRIGGER IF EXISTS vault_export_daily_notes_insert;
DROP TRIGGER IF EXISTS vault_export_daily_notes_update;
DROP TRIGGER IF EXISTS vault_export_daily_notes_delete;
DROP TRIGGER IF EXISTS vault_export_action_items_insert;
DROP TRIGGER IF EXISTS vault_export_action_items_update;
DROP TRIGGER IF EXISTS vault_export_action_items_delete;
DROP TRIGGER IF EXISTS vault_export_app_settings_insert;
DROP TRIGGER IF EXISTS vault_export_app_settings_update;
DROP TRIGGER IF EXISTS vault_export_app_settings_delete;

CREATE TRIGGER IF NOT EXISTS search_index_sessions_insert
AFTER INSERT ON sessions
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  VALUES ('session', NEW.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_sessions_update
AFTER UPDATE ON sessions
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  VALUES ('session', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO search_index_dirty (entity_type, entity_id)
  SELECT 'session', NEW.id
  WHERE NEW.id <> OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_sessions_delete
AFTER DELETE ON sessions
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  VALUES ('session', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_session_documents_insert
AFTER INSERT ON session_documents
WHEN NEW.session_id <> ''
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  VALUES ('session', NEW.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_session_documents_update
AFTER UPDATE ON session_documents
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  SELECT 'session', OLD.session_id
  WHERE OLD.session_id <> ''
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO search_index_dirty (entity_type, entity_id)
  SELECT 'session', NEW.session_id
  WHERE NEW.session_id <> '' AND NEW.session_id <> OLD.session_id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_session_documents_delete
AFTER DELETE ON session_documents
WHEN OLD.session_id <> ''
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  VALUES ('session', OLD.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_transcripts_insert
AFTER INSERT ON transcripts
WHEN NEW.session_id <> ''
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  VALUES ('session', NEW.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_transcripts_update
AFTER UPDATE ON transcripts
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  SELECT 'session', OLD.session_id
  WHERE OLD.session_id <> ''
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO search_index_dirty (entity_type, entity_id)
  SELECT 'session', NEW.session_id
  WHERE NEW.session_id <> '' AND NEW.session_id <> OLD.session_id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS search_index_transcripts_delete
AFTER DELETE ON transcripts
WHEN OLD.session_id <> ''
BEGIN
  INSERT INTO search_index_dirty (entity_type, entity_id)
  VALUES ('session', OLD.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = search_index_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

-- Plain read-performance indexes from the canonical schema, minus
-- idx_sessions_event_id (its column is gone).
CREATE INDEX IF NOT EXISTS idx_sessions_created_at ON sessions(created_at);
CREATE INDEX IF NOT EXISTS idx_sessions_folder_path ON sessions(folder_path);
CREATE INDEX IF NOT EXISTS idx_session_documents_session_id ON session_documents(session_id);
CREATE INDEX IF NOT EXISTS idx_session_documents_kind ON session_documents(kind);
CREATE INDEX IF NOT EXISTS idx_transcripts_session_id ON transcripts(session_id);
