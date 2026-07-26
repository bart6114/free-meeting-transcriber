import { sql } from "drizzle-orm";
import { index, integer, sqliteTable, text } from "drizzle-orm/sqlite-core";

const currentTimestamp = sql`(strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))`;

export const templates = sqliteTable("templates", {
  id: text("id").primaryKey(),
  title: text("title").notNull().default(""),
  description: text("description").notNull().default(""),
  pinned: integer("pinned", { mode: "boolean" }).notNull().default(false),
  pinOrder: integer("pin_order"),
  category: text("category"),
  iconJson: text("icon_json", { mode: "json" })
    .notNull()
    .default('{"type":"icon","value":"notebook-tabs","color":"#9ca3af"}'),
  targetsJson: text("targets_json", { mode: "json" }),
  sectionsJson: text("sections_json", { mode: "json" }).notNull().default("[]"),
  createdAt: text("created_at").notNull(),
  updatedAt: text("updated_at").notNull(),
});

export const sessions = sqliteTable(
  "sessions",
  {
    id: text("id").primaryKey().notNull(),
    ownerUserId: text("owner_user_id").notNull().default(""),
    title: text("title").notNull().default(""),
    kind: text("kind").notNull().default("meeting"),
    status: text("status").notNull().default("active"),
    createdAt: text("created_at").notNull().default(currentTimestamp),
    updatedAt: text("updated_at").notNull().default(currentTimestamp),
    startedAt: text("started_at").notNull().default(""),
    endedAt: text("ended_at").notNull().default(""),
    timezone: text("timezone").notNull().default(""),
    language: text("language").notNull().default(""),
    externalProvider: text("external_provider").notNull().default(""),
    sourceAppsJson: text("source_apps_json").notNull().default("[]"),
    eventJson: text("event_json").notNull().default(""),
    folderPath: text("folder_path").notNull().default(""),
    slug: text("slug").notNull().default(""),
    metadataJson: text("metadata_json").notNull().default("{}"),
  },
  (table) => [
    index("idx_sessions_created_at").on(table.createdAt),
    index("idx_sessions_folder_path").on(table.folderPath),
  ],
);

export const sessionDocuments = sqliteTable(
  "session_documents",
  {
    id: text("id").primaryKey().notNull(),
    sessionId: text("session_id").notNull().default(""),
    kind: text("kind").notNull().default("note"),
    templateId: text("template_id").notNull().default(""),
    title: text("title").notNull().default(""),
    bodyFormat: text("body_format").notNull().default("prosemirror_json"),
    body: text("body").notNull().default(""),
    sortOrder: integer("sort_order").notNull().default(0),
    createdBy: text("created_by").notNull().default(""),
    updatedBy: text("updated_by").notNull().default(""),
    updatedAt: text("updated_at").notNull().default(currentTimestamp),
    deletedAt: text("deleted_at"),
  },
  (table) => [
    index("idx_session_documents_session_id").on(table.sessionId),
    index("idx_session_documents_kind").on(table.kind),
  ],
);

export const transcripts = sqliteTable(
  "transcripts",
  {
    id: text("id").primaryKey().notNull(),
    ownerUserId: text("owner_user_id").notNull().default(""),
    sessionId: text("session_id").notNull().default(""),
    startedAtMs: integer("started_at_ms").notNull().default(0),
    endedAtMs: integer("ended_at_ms"),
    memo: text("memo").notNull().default(""),
    wordsJson: text("words_json").notNull().default("[]"),
    speakerHintsJson: text("speaker_hints_json").notNull().default("[]"),
    updatedAt: text("updated_at").notNull().default(currentTimestamp),
    deletedAt: text("deleted_at"),
  },
  (table) => [index("idx_transcripts_session_id").on(table.sessionId)],
);

export const actionItems = sqliteTable(
  "action_items",
  {
    id: text("id").primaryKey().notNull(),
    workspaceId: text("workspace_id").notNull().default(""),
    sessionId: text("session_id").notNull().default(""),
    sourceType: text("source_type").notNull().default(""),
    sourceId: text("source_id").notNull().default(""),
    sourceOrder: integer("source_order").notNull().default(0),
    assigneeHumanId: text("assignee_human_id").notNull().default(""),
    status: text("status").notNull().default("todo"),
    text: text("text").notNull().default(""),
    bodyJson: text("body_json").notNull().default("{}"),
    dueAt: text("due_at").notNull().default(""),
    completedAt: text("completed_at"),
    createdBy: text("created_by").notNull().default(""),
    updatedBy: text("updated_by").notNull().default(""),
    metadataJson: text("metadata_json").notNull().default("{}"),
    createdAt: text("created_at").notNull().default(currentTimestamp),
    updatedAt: text("updated_at").notNull().default(currentTimestamp),
    deletedAt: text("deleted_at"),
  },
  (table) => [
    index("idx_action_items_session_id").on(table.sessionId),
    index("idx_action_items_source").on(table.sourceType, table.sourceId),
  ],
);

export const tags = sqliteTable(
  "tags",
  {
    id: text("id").primaryKey().notNull(),
    workspaceId: text("workspace_id").notNull().default(""),
    ownerUserId: text("owner_user_id").notNull().default(""),
    name: text("name").notNull().default(""),
    createdAt: text("created_at").notNull().default(currentTimestamp),
    updatedAt: text("updated_at").notNull().default(currentTimestamp),
    deletedAt: text("deleted_at"),
  },
  (table) => [index("idx_tags_name").on(table.name)],
);

export const sessionTags = sqliteTable(
  "session_tags",
  {
    id: text("id").primaryKey().notNull(),
    workspaceId: text("workspace_id").notNull().default(""),
    ownerUserId: text("owner_user_id").notNull().default(""),
    sessionId: text("session_id").notNull().default(""),
    tagId: text("tag_id").notNull().default(""),
    createdAt: text("created_at").notNull().default(currentTimestamp),
    updatedAt: text("updated_at").notNull().default(currentTimestamp),
    deletedAt: text("deleted_at"),
  },
  (table) => [
    index("idx_session_tags_session_id").on(table.sessionId),
    index("idx_session_tags_tag_id").on(table.tagId),
  ],
);

export const appSettings = sqliteTable("app_settings", {
  id: text("id").primaryKey().notNull(),
  valueJson: text("value_json").notNull().default("null"),
  updatedAt: text("updated_at").notNull().default(currentTimestamp),
});
