-- Write-through DB-to-vault mirror (Task 13): a dirty-queue table drained by
-- the `vault_export` worker (apps/desktop/src-tauri/src/vault_export.rs),
-- modeled 1:1 on `search_index_dirty` (20260714120000_search_index_queue.sql
-- + its per-table trigger migrations). One row per vault-file-producing
-- entity; `generation` lets the worker detect a row changed again while it
-- was mid-export (same pattern as search_index's acknowledge-by-generation).
--
-- Entity granularity intentionally collapses several source tables onto one
-- vault file's entity id:
--   sessions / session_documents / transcripts / session_participants ->
--     ('session', session_id): all four live under sessions/<id>/*.
--   tags -> propagated to every session that references it via session_tags
--     (tags are embedded in each session's _meta.json, not their own file).
--   session_tags -> ('session', session_id) directly.
--   humans -> ('human', id); organizations -> ('organization', id).
--   chat_groups / chat_messages -> ('chat_group', chat_group_id): both live
--     under chats/<group>/messages.json.
--   calendars / events / daily_notes / action_items / app_settings -> fixed
--     singleton ids ('all' / 'legacy_settings_document') since each is one
--     shared JSON file re-rendered from the whole table on every change.
CREATE TABLE IF NOT EXISTS vault_export_dirty (
  entity_type  TEXT NOT NULL,
  entity_id    TEXT NOT NULL,
  generation   INTEGER NOT NULL DEFAULT 1,
  queued_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (entity_type, entity_id)
) STRICT;

CREATE TRIGGER IF NOT EXISTS vault_export_sessions_insert
AFTER INSERT ON sessions
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', NEW.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_sessions_update
AFTER UPDATE ON sessions
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', NEW.id
  WHERE NEW.id <> OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_sessions_delete
AFTER DELETE ON sessions
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_documents_insert
AFTER INSERT ON session_documents
WHEN NEW.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', NEW.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_documents_update
AFTER UPDATE ON session_documents
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', OLD.session_id
  WHERE OLD.session_id <> ''
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', NEW.session_id
  WHERE NEW.session_id <> '' AND NEW.session_id <> OLD.session_id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_documents_delete
AFTER DELETE ON session_documents
WHEN OLD.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', OLD.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_transcripts_insert
AFTER INSERT ON transcripts
WHEN NEW.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', NEW.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_transcripts_update
AFTER UPDATE ON transcripts
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', OLD.session_id
  WHERE OLD.session_id <> ''
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', NEW.session_id
  WHERE NEW.session_id <> '' AND NEW.session_id <> OLD.session_id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_transcripts_delete
AFTER DELETE ON transcripts
WHEN OLD.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', OLD.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_participants_insert
AFTER INSERT ON session_participants
WHEN NEW.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', NEW.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_participants_update
AFTER UPDATE ON session_participants
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', OLD.session_id
  WHERE OLD.session_id <> ''
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', NEW.session_id
  WHERE NEW.session_id <> '' AND NEW.session_id <> OLD.session_id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_participants_delete
AFTER DELETE ON session_participants
WHEN OLD.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', OLD.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_humans_insert
AFTER INSERT ON humans
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('human', NEW.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_humans_update
AFTER UPDATE ON humans
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('human', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'human', NEW.id
  WHERE NEW.id <> OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_humans_delete
AFTER DELETE ON humans
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('human', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_organizations_insert
AFTER INSERT ON organizations
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('organization', NEW.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_organizations_update
AFTER UPDATE ON organizations
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('organization', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'organization', NEW.id
  WHERE NEW.id <> OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_organizations_delete
AFTER DELETE ON organizations
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('organization', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_tags_insert
AFTER INSERT ON tags
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', session_id FROM session_tags WHERE tag_id = NEW.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_tags_update
AFTER UPDATE ON tags
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', session_id FROM session_tags WHERE tag_id = OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', session_id FROM session_tags
  WHERE tag_id = NEW.id AND NEW.id <> OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_tags_delete
AFTER DELETE ON tags
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', session_id FROM session_tags WHERE tag_id = OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_tags_insert
AFTER INSERT ON session_tags
WHEN NEW.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', NEW.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_tags_update
AFTER UPDATE ON session_tags
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', OLD.session_id
  WHERE OLD.session_id <> ''
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'session', NEW.session_id
  WHERE NEW.session_id <> '' AND NEW.session_id <> OLD.session_id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_session_tags_delete
AFTER DELETE ON session_tags
WHEN OLD.session_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('session', OLD.session_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_chat_groups_insert
AFTER INSERT ON chat_groups
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('chat_group', NEW.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_chat_groups_update
AFTER UPDATE ON chat_groups
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('chat_group', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'chat_group', NEW.id
  WHERE NEW.id <> OLD.id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_chat_groups_delete
AFTER DELETE ON chat_groups
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('chat_group', OLD.id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_chat_messages_insert
AFTER INSERT ON chat_messages
WHEN NEW.chat_group_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('chat_group', NEW.chat_group_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_chat_messages_update
AFTER UPDATE ON chat_messages
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'chat_group', OLD.chat_group_id
  WHERE OLD.chat_group_id <> ''
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

  INSERT INTO vault_export_dirty (entity_type, entity_id)
  SELECT 'chat_group', NEW.chat_group_id
  WHERE NEW.chat_group_id <> '' AND NEW.chat_group_id <> OLD.chat_group_id
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_chat_messages_delete
AFTER DELETE ON chat_messages
WHEN OLD.chat_group_id <> ''
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id)
  VALUES ('chat_group', OLD.chat_group_id)
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_calendars_insert
AFTER INSERT ON calendars
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('calendars_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_calendars_update
AFTER UPDATE ON calendars
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('calendars_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_calendars_delete
AFTER DELETE ON calendars
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('calendars_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_events_insert
AFTER INSERT ON events
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('events_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_events_update
AFTER UPDATE ON events
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('events_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_events_delete
AFTER DELETE ON events
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('events_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_daily_notes_insert
AFTER INSERT ON daily_notes
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('daily_notes_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_daily_notes_update
AFTER UPDATE ON daily_notes
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('daily_notes_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_daily_notes_delete
AFTER DELETE ON daily_notes
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('daily_notes_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_action_items_insert
AFTER INSERT ON action_items
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('tasks_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_action_items_update
AFTER UPDATE ON action_items
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('tasks_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_action_items_delete
AFTER DELETE ON action_items
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('tasks_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_app_settings_insert
AFTER INSERT ON app_settings
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('settings_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_app_settings_update
AFTER UPDATE ON app_settings
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('settings_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;

CREATE TRIGGER IF NOT EXISTS vault_export_app_settings_delete
AFTER DELETE ON app_settings
BEGIN
  INSERT INTO vault_export_dirty (entity_type, entity_id) VALUES ('settings_file', 'all')
  ON CONFLICT (entity_type, entity_id) DO UPDATE SET
    generation = vault_export_dirty.generation + 1,
    queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');
END;
