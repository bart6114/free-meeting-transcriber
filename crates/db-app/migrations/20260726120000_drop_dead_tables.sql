-- Drop the dead chat/daily-notes/mentions tables: zero production readers or
-- writers remain (the chat feature died in 93c3f5e). SQLite drops each table's
-- indexes with it, and no surviving trigger references these tables.
DROP TABLE IF EXISTS chat_groups;
DROP TABLE IF EXISTS chat_messages;
DROP TABLE IF EXISTS daily_notes;
DROP TABLE IF EXISTS entity_mentions;
