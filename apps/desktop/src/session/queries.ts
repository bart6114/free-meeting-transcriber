import { useCallback } from "react";

import { json2md, md2json } from "@hypr/editor/markdown";
import { commands as analyticsCommands } from "@hypr/plugin-analytics";
import { commands as fsSyncCommands } from "@hypr/plugin-fs-sync";

import { executeTransaction, liveQueryClient, useLiveQuery } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";
import { waitForPendingSoftDelete } from "~/session/pending-soft-deletes";
import { DEFAULT_USER_ID, id } from "~/shared/utils";
import type { DeletedSessionData } from "~/store/zustand/undo-delete";

type SessionIdentitySqlRow = { id: string };
type SessionDeleteSqlRow = { id: string; title: string };
type SessionEmptySqlRow = {
  title: string;
  event_json: string;
  note_body: string;
  note_body_format: string;
  transcript_count: number;
  enhanced_note_count: number;
  meeting_chat_count: number;
  tag_count: number;
};

type SessionSqlRow = {
  id: string;
  owner_user_id: string;
  created_at: string;
  folder_path: string;
  event_json: string;
  title: string;
  raw_body: string;
  raw_body_format: string;
};

type SessionSummarySqlRow = {
  id: string;
  title: string;
  created_at: string;
};

type SessionTranscriptStateSqlRow = {
  has_transcript: boolean | number;
};

type EnhancedNoteSqlRow = {
  id: string;
  session_id: string;
  title: string;
  body: string;
  body_format: string;
  template_id: string;
  sort_order: number;
};

export type SessionRecord = {
  id: string;
  user_id: string;
  created_at: string;
  folder_id: string;
  event_json: string;
  title: string;
  raw_md: string;
};

export type SessionChanges = Partial<
  Pick<
    SessionRecord,
    "created_at" | "event_json" | "folder_id" | "raw_md" | "title"
  >
>;

export type SessionSummaryRecord = {
  id: string;
  title: string;
  created_at: string;
};

export type EnhancedNoteRecord = {
  id: string;
  sessionId: string;
  title: string;
  content: string;
  templateId: string;
  position: number;
};

const EMPTY_ENHANCED_NOTES: EnhancedNoteRecord[] = [];
const EMPTY_SESSION_SUMMARIES: SessionSummaryRecord[] = [];

const SESSION_SELECT_SQL = `
  SELECT
    sessions.id,
    sessions.owner_user_id,
    sessions.created_at,
    sessions.folder_path,
    sessions.event_json,
    sessions.title,
    COALESCE(note.body, '') AS raw_body,
    COALESCE(note.body_format, 'prosemirror_json') AS raw_body_format
  FROM sessions
  LEFT JOIN session_documents AS note
    ON note.id = sessions.id
    AND note.kind = 'note'
    AND note.deleted_at IS NULL
  WHERE sessions.id = ? AND sessions.deleted_at IS NULL
  LIMIT 1
`;

export function useSession(sessionId: string): SessionRecord | null {
  const { data = null } = useLiveQuery<SessionSqlRow, SessionRecord | null>({
    sql: SESSION_SELECT_SQL,
    params: [sessionId],
    enabled: Boolean(sessionId),
    mapRows: (rows) => {
      const row = rows[0];
      return row ? mapSessionRow(row) : null;
    },
  });
  return sessionId ? data : null;
}

export function useSessionSummary(
  sessionId: string,
): SessionSummaryRecord | null {
  const { data = null } = useLiveQuery<
    SessionSummarySqlRow,
    SessionSummaryRecord | null
  >({
    sql: `
      SELECT id, title, created_at
      FROM sessions
      WHERE id = ? AND deleted_at IS NULL
      LIMIT 1
    `,
    params: [sessionId],
    enabled: Boolean(sessionId),
    mapRows: (rows) => rows[0] ?? null,
  });
  return sessionId ? data : null;
}

export function useSessionSummaries(): SessionSummaryRecord[] {
  const { data = EMPTY_SESSION_SUMMARIES } = useLiveQuery<
    SessionSummarySqlRow,
    SessionSummaryRecord[]
  >({
    sql: `
      SELECT id, title, created_at
      FROM sessions
      WHERE deleted_at IS NULL
      ORDER BY created_at DESC, id
    `,
  });
  return data;
}

export function useUpdateSession(sessionId: string) {
  return useCallback(
    (changes: SessionChanges) => updateSession(sessionId, changes),
    [sessionId],
  );
}

export function useSessionHasTranscript(sessionId: string): boolean {
  const { data = false } = useLiveQuery<SessionTranscriptStateSqlRow, boolean>({
    sql: `
      SELECT EXISTS (
        SELECT 1
        FROM transcripts
        WHERE session_id = ?
          AND deleted_at IS NULL
          AND CASE
            WHEN json_valid(words_json) THEN json_array_length(words_json)
            ELSE 0
          END > 0
      ) AS has_transcript
    `,
    params: [sessionId],
    enabled: Boolean(sessionId),
    mapRows: (rows) => Boolean(rows[0]?.has_transcript),
  });
  return sessionId ? data : false;
}

export function useEnhancedNoteRecords(
  sessionId: string,
): EnhancedNoteRecord[] {
  const { data = EMPTY_ENHANCED_NOTES } = useLiveQuery<
    EnhancedNoteSqlRow,
    EnhancedNoteRecord[]
  >({
    sql: `
      SELECT
        id,
        session_id,
        title,
        body,
        body_format,
        template_id,
        sort_order
      FROM session_documents
      WHERE session_id = ?
        AND kind IN ('summary', 'template_output')
        AND deleted_at IS NULL
      ORDER BY sort_order, id
    `,
    params: [sessionId],
    enabled: Boolean(sessionId),
    mapRows: (rows) => rows.map(mapEnhancedNoteRow),
  });
  return sessionId ? data : EMPTY_ENHANCED_NOTES;
}

export function useEnhancedNote(
  enhancedNoteId: string,
): EnhancedNoteRecord | null {
  const { data = null } = useLiveQuery<
    EnhancedNoteSqlRow,
    EnhancedNoteRecord | null
  >({
    sql: `
      SELECT
        id,
        session_id,
        title,
        body,
        body_format,
        template_id,
        sort_order
      FROM session_documents
      WHERE id = ?
        AND kind IN ('summary', 'template_output')
        AND deleted_at IS NULL
      LIMIT 1
    `,
    params: [enhancedNoteId],
    enabled: Boolean(enhancedNoteId),
    mapRows: (rows) => {
      const row = rows[0];
      return row ? mapEnhancedNoteRow(row) : null;
    },
  });
  return enhancedNoteId ? data : null;
}

export function useUpdateEnhancedNoteContent(
  enhancedNoteId: string,
  sessionId: string,
) {
  return useCallback(
    (content: string, sessionTitle?: string) =>
      updateEnhancedNoteContent(
        enhancedNoteId,
        sessionId,
        content,
        sessionTitle,
      ),
    [enhancedNoteId, sessionId],
  );
}

export function updateEnhancedNoteContent(
  enhancedNoteId: string,
  sessionId: string,
  content: string,
  sessionTitle?: string,
): Promise<void> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    const now = new Date().toISOString();
    const statements: Array<{ sql: string; params: unknown[] }> = [
      {
        sql: `
          UPDATE session_documents
          SET body = ?, body_format = 'prosemirror_json', updated_at = ?
          WHERE id = ?
            AND kind IN ('summary', 'template_output')
            AND deleted_at IS NULL
        `,
        params: [content, now, enhancedNoteId],
      },
    ];

    if (sessionTitle !== undefined) {
      statements.push({
        sql: `
          UPDATE sessions
          SET title = ?, updated_at = ?
          WHERE id = ? AND deleted_at IS NULL
        `,
        params: [sessionTitle, now, sessionId],
      });
    }

    await executeTransaction(statements);
  });
}

export function deleteEnhancedNote(enhancedNoteId: string): Promise<void> {
  return enqueueDatabaseWrite(`enhanced-note:${enhancedNoteId}`, async () => {
    const now = new Date().toISOString();
    await executeTransaction([
      {
        sql: `
          UPDATE session_documents
          SET deleted_at = ?, updated_at = ?
          WHERE id = ?
            AND kind IN ('summary', 'template_output')
            AND deleted_at IS NULL
        `,
        params: [now, now, enhancedNoteId],
      },
    ]);
  });
}

export function updateSession(
  sessionId: string,
  changes: SessionChanges,
): Promise<void> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    const now = new Date().toISOString();
    const assignments: string[] = [];
    const params: unknown[] = [];

    for (const [column, value] of [
      ["title", changes.title],
      ["created_at", changes.created_at],
      ["folder_path", changes.folder_id],
      ["event_json", changes.event_json],
    ] as const) {
      if (value === undefined) continue;
      assignments.push(`${column} = ?`);
      params.push(value);
    }

    const statements: Array<{ sql: string; params: unknown[] }> = [];
    if (assignments.length > 0) {
      statements.push({
        sql: `
          UPDATE sessions
          SET ${assignments.join(", ")}, updated_at = ?
          WHERE id = ? AND deleted_at IS NULL
        `,
        params: [...params, now, sessionId],
      });
    }

    if (changes.raw_md !== undefined) {
      statements.push({
        sql: `
          INSERT INTO session_documents (
            id, session_id, kind, body_format, body, created_by,
            updated_by, created_at, updated_at, deleted_at
          )
          SELECT ?, id, 'note', 'prosemirror_json', ?,
            owner_user_id, owner_user_id, ?, ?, NULL
          FROM sessions
          WHERE id = ? AND deleted_at IS NULL
          ON CONFLICT(id) DO UPDATE SET
            body_format = excluded.body_format,
            body = excluded.body,
            updated_by = excluded.updated_by,
            updated_at = excluded.updated_at,
            deleted_at = NULL
        `,
        params: [sessionId, changes.raw_md, now, now, sessionId],
      });
    }

    if (statements.length > 0) await executeTransaction(statements);
  });
}

export async function createSession(
  title = "",
  userId = DEFAULT_USER_ID,
  initial?: Pick<SessionChanges, "event_json" | "raw_md">,
): Promise<string> {
  const sessionId = id();
  const now = new Date().toISOString();

  await executeTransaction([
    {
      sql: `
        INSERT INTO sessions (
          id, owner_user_id, title, event_json, created_at,
          updated_at, deleted_at
        ) VALUES (
          ?, ?, ?, ?, ?, ?, NULL
        )
      `,
      params: [sessionId, userId, title, initial?.event_json ?? "", now, now],
    },
    createEmptyNoteStatement(sessionId, now, initial?.raw_md ?? ""),
  ]);

  trackNoteCreated();
  return sessionId;
}

export async function softDeleteSession(
  sessionId: string,
  tombstone = new Date().toISOString(),
): Promise<DeletedSessionData | null> {
  const [session] = await liveQueryClient.execute<SessionDeleteSqlRow>(
    `SELECT id, title FROM sessions WHERE id = ? AND deleted_at IS NULL LIMIT 1`,
    [sessionId],
  );
  if (!session) return null;

  const rowsAffected = await executeTransaction(
    buildSessionTombstoneStatements(sessionId, tombstone),
  );
  if (rowsAffected[rowsAffected.length - 1] !== 1) return null;

  return {
    session: { id: session.id, title: session.title },
    tombstone,
    deletedAt: Date.now(),
  };
}

export async function isSessionEmpty(sessionId: string): Promise<boolean> {
  const [row] = await liveQueryClient.execute<SessionEmptySqlRow>(
    `
      SELECT
        sessions.title,
        sessions.event_json,
        COALESCE(note.body, '') AS note_body,
        COALESCE(note.body_format, '') AS note_body_format,
        (
          SELECT COUNT(*)
          FROM transcripts
          WHERE session_id = sessions.id AND deleted_at IS NULL
        ) AS transcript_count,
        (
          SELECT COUNT(*)
          FROM session_documents
          WHERE session_id = sessions.id
            AND kind IN ('summary', 'template_output')
            AND deleted_at IS NULL
        ) AS enhanced_note_count,
        (
          SELECT COUNT(*)
          FROM session_documents
          WHERE session_id = sessions.id
            AND kind = 'meeting_chat'
            AND deleted_at IS NULL
        ) AS meeting_chat_count,
        (
          SELECT COUNT(*)
          FROM session_tags
          WHERE session_id = sessions.id AND deleted_at IS NULL
        ) AS tag_count
      FROM sessions
      LEFT JOIN session_documents AS note
        ON note.id = sessions.id
        AND note.kind = 'note'
        AND note.deleted_at IS NULL
      WHERE sessions.id = ? AND sessions.deleted_at IS NULL
      LIMIT 1
    `,
    [sessionId],
  );

  if (!row) return true;
  if (row.title.trim() && !row.event_json) return false;
  if (hasNoteContent(row.note_body, row.note_body_format)) return false;

  return (
    Number(row.transcript_count) === 0 &&
    Number(row.enhanced_note_count) === 0 &&
    Number(row.meeting_chat_count) === 0 &&
    Number(row.tag_count) === 0
  );
}

export async function restoreDeletedSession(
  data: DeletedSessionData,
): Promise<void> {
  // The undo toast shows before the soft-delete write commits. Wait for the
  // in-flight delete to settle first — an "alive" session during that window
  // is not restored, it just isn't tombstoned yet.
  await waitForPendingSoftDelete(data.session.id);
  for (let attempt = 0; attempt < 10; attempt += 1) {
    const rowsAffected = await executeTransaction(
      buildSessionTombstoneStatements(data.session.id, data.tombstone, true),
    );
    if (rowsAffected[rowsAffected.length - 1] === 1) return;

    const [alive] = await liveQueryClient.execute<SessionIdentitySqlRow>(
      `SELECT id FROM sessions WHERE id = ? AND deleted_at IS NULL LIMIT 1`,
      [data.session.id],
    );
    if (alive) return;

    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  throw new Error(`Session ${data.session.id} was never soft-deleted`);
}

export async function finalizeSessionDeletion(
  sessionId: string,
): Promise<void> {
  try {
    const result = await fsSyncCommands.deleteSessionFolder(sessionId);
    if (result.status !== "error") return;
    console.error("[delete-session] failed to delete session folder", {
      sessionId,
      error: result.error,
    });
  } catch (error) {
    console.error("[delete-session] failed to delete session folder", {
      sessionId,
      error,
    });
  }
}

export function buildSessionTombstoneStatements(
  sessionId: string,
  tombstone: string,
  restore = false,
) {
  const value = restore ? null : tombstone;
  const predicate = restore ? "deleted_at = ?" : "deleted_at IS NULL";
  const predicateParams = restore ? [tombstone] : [];
  const directTables = [
    "session_documents",
    "transcripts",
    "session_tags",
    "action_items",
  ];

  const statements = directTables.map((table) => ({
    sql: `
      UPDATE ${table}
      SET deleted_at = ?, updated_at = ?
      WHERE session_id = ? AND ${predicate}
    `,
    params: [value, tombstone, sessionId, ...predicateParams],
  }));

  statements.push({
    sql: `
      UPDATE entity_mentions
      SET deleted_at = ?, updated_at = ?
      WHERE (
        (source_type = 'session' AND source_id = ?)
        OR (target_type = 'session' AND target_id = ?)
      ) AND ${predicate}
    `,
    params: [value, tombstone, sessionId, sessionId, ...predicateParams],
  });
  statements.push({
    sql: `
      UPDATE sessions
      SET deleted_at = ?, updated_at = ?
      WHERE id = ? AND ${predicate}
    `,
    params: [value, tombstone, sessionId, ...predicateParams],
  });

  return statements;
}

function createEmptyNoteStatement(sessionId: string, now: string, body = "") {
  return {
    sql: `
      INSERT INTO session_documents (
        id, session_id, kind, body_format, body, created_by,
        updated_by, created_at, updated_at, deleted_at
      )
      SELECT ?, id, 'note', 'prosemirror_json', ?,
        owner_user_id, owner_user_id, ?, ?, NULL
      FROM sessions
      WHERE id = ? AND deleted_at IS NULL
    `,
    params: [sessionId, body, now, now, sessionId],
  };
}

function hasNoteContent(body: string, format: string): boolean {
  if (!body) return false;

  let markdown = body;
  if (format === "prosemirror_json") {
    try {
      markdown = json2md(JSON.parse(body));
    } catch {
      markdown = body;
    }
  }

  markdown = markdown.trim();
  return Boolean(markdown && markdown !== "&nbsp;");
}

function mapSessionRow(row: SessionSqlRow): SessionRecord {
  let rawMd = row.raw_body;
  if (rawMd && row.raw_body_format === "markdown") {
    try {
      rawMd = JSON.stringify(md2json(rawMd));
    } catch (error) {
      console.error("[session] failed to decode imported Markdown", error);
    }
  }

  return {
    id: row.id,
    user_id: row.owner_user_id,
    created_at: row.created_at,
    folder_id: row.folder_path,
    event_json: row.event_json,
    title: row.title,
    raw_md: rawMd,
  };
}

function mapEnhancedNoteRow(row: EnhancedNoteSqlRow): EnhancedNoteRecord {
  let content = row.body;
  if (content && row.body_format === "markdown") {
    try {
      content = JSON.stringify(md2json(content));
    } catch (error) {
      console.error("[session] failed to decode summary Markdown", error);
    }
  }

  return {
    id: row.id,
    sessionId: row.session_id,
    title: row.title,
    content,
    templateId: row.template_id,
    position: Number(row.sort_order),
  };
}

function trackNoteCreated(): void {
  void analyticsCommands
    .eventFireAndForget({
      event: "note_created",
      has_event_id: false,
    })
    .catch((error) => {
      console.error(
        "[session] failed to record note creation analytics",
        error,
      );
    });
}
