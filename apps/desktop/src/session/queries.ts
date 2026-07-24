import { useQuery } from "@tanstack/react-query";
import { useCallback } from "react";

import { json2md, md2json } from "@hypr/editor/markdown";
import { commands as analyticsCommands } from "@hypr/plugin-analytics";
import { commands as fsSyncCommands } from "@hypr/plugin-fs-sync";

import { executeTransaction, liveQueryClient, useLiveQuery } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";
import { waitForPendingSoftDelete } from "~/session/pending-soft-deletes";
import { DEFAULT_USER_ID, id } from "~/shared/utils";
import type { DeletedSessionData } from "~/store/zustand/undo-delete";
import { commands } from "~/types/tauri.gen";

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

// Note content ("raw_md") is intentionally excluded: it's written exclusively via
// `sessionWriteNote` now (see raw.tsx's persistChange), never through this SQL path.
export type SessionChanges = Partial<
  Pick<SessionRecord, "created_at" | "event_json" | "folder_id" | "title">
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

// The store (Tasks 5-8) writes the note row under id "<sessionId>:note", not "<sessionId>"
// (the pre-store convention). The store-written row must win when both exist: `createSession`
// now seeds the legacy "<sessionId>" row as a permanently-empty placeholder, so once the note
// editor saves exclusively through the store, that row never changes again -- preferring it
// would freeze every read here at "empty" forever instead of showing live content.
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
    ON note.id = COALESCE(
      (
        SELECT store_note.id
        FROM session_documents AS store_note
        WHERE store_note.id = sessions.id || ':note'
          AND store_note.session_id = sessions.id
          AND store_note.kind = 'note'
          AND store_note.deleted_at IS NULL
        LIMIT 1
      ),
      (
        SELECT legacy_note.id
        FROM session_documents AS legacy_note
        WHERE legacy_note.id = sessions.id
          AND legacy_note.session_id = sessions.id
          AND legacy_note.kind = 'note'
          AND legacy_note.deleted_at IS NULL
        LIMIT 1
      )
    )
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

/**
 * File-canonical note content for the editor: prefers `sessions/<id>/_memo.md` (read via
 * `session_read_note`) and falls back to the index's `raw_md` only while the file read hasn't
 * resolved yet, or when the file genuinely doesn't exist (e.g. a session never touched by the
 * store). Returns `null` while the index row itself hasn't loaded -- callers should keep
 * showing a loading state for that, same as `useSession` returning `null`.
 *
 * `staleTime: Infinity` is deliberate: this is a one-shot "seed the editor's initial content"
 * load, not a live subscription -- once mounted, the editor tracks further edits itself
 * (`persistChange`), and NoteEditor only re-syncs its content from a changed `rawMd` when the
 * editor isn't focused (see `shouldReplaceEditorContent` in `@hypr/editor/note`), so refetching
 * this on every render/focus would risk clobbering in-progress typing for no benefit.
 */
export function useSessionRawMd(sessionId: string): string | null {
  const indexSession = useSession(sessionId);
  const noteFile = useQuery({
    queryKey: ["session-note-file", sessionId],
    queryFn: async () => {
      const result = await commands.sessionReadNote(sessionId);
      if (result.status === "error") {
        throw new Error(result.error);
      }
      return result.data;
    },
    enabled: Boolean(sessionId),
    staleTime: Infinity,
  });

  if (!indexSession) return null;

  const fileMarkdown = noteFile.data;
  if (fileMarkdown !== null && fileMarkdown !== undefined) {
    return JSON.stringify(md2json(fileMarkdown));
  }
  return indexSession.raw_md;
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

    if (statements.length > 0) await executeTransaction(statements);
  });
}

export async function createSession(
  title = "",
  _userId = DEFAULT_USER_ID,
  // Not `Pick<SessionChanges, ...>`: `raw_md` was removed from `SessionChanges` (note content
  // is store-only now, see the comment on that type), but a session can still be *created*
  // with initial content (e.g. the onboarding welcome note) -- that seeds the file-canonical
  // store directly below, never `SessionChanges`/`updateSession`'s SQL path.
  initial?: { event_json?: string; raw_md?: string },
): Promise<string> {
  const sessionId = id();
  const now = new Date().toISOString();

  const metaWrite = await commands.sessionWriteMeta({
    id: sessionId,
    title,
    started_at: null,
    ended_at: null,
    created_at: now,
    tags: [],
  });
  if (metaWrite.status === "error") {
    throw new Error(
      `Failed to create session ${sessionId}: ${metaWrite.error}`,
    );
  }

  const statements: Array<{ sql: string; params: unknown[] }> = [
    // Bookkeeping placeholder only: always an empty body. Canonical-id-vs-fallback reads
    // (SESSION_SELECT_SQL above, useKeywords.ts, isSessionEmpty below) COALESCE onto a
    // store-written "<id>:note" row when this bare-id row is empty, so this just keeps a
    // row present for the join -- real note content lives only in the file-canonical store
    // from here on (sessionWriteNote below), never in this row's body.
    createEmptyNoteStatement(sessionId, now, ""),
  ];
  if (initial?.event_json) {
    statements.push({
      sql: `UPDATE sessions SET event_json = ? WHERE id = ?`,
      params: [initial.event_json, sessionId],
    });
  }
  await executeTransaction(statements);

  if (initial?.raw_md) {
    let markdown = "";
    try {
      markdown = json2md(JSON.parse(initial.raw_md));
    } catch (error) {
      console.error(
        "[session] failed to convert initial note content to markdown",
        error,
      );
    }
    if (markdown) {
      const noteWrite = await commands.sessionWriteNote(sessionId, markdown);
      if (noteWrite.status === "error") {
        console.error(
          "[session] failed to seed session note file",
          noteWrite.error,
        );
      }
    }
  }

  trackNoteCreated();
  return sessionId;
}

export async function softDeleteSession(
  sessionId: string,
  tombstone = new Date().toISOString(),
): Promise<DeletedSessionData | null> {
  // Read title before deleting: session_delete removes the index row outright (no
  // tombstone column left to read back from), so anything the undo toast needs to
  // display has to be captured up front.
  const [session] = await liveQueryClient.execute<SessionDeleteSqlRow>(
    `SELECT id, title FROM sessions WHERE id = ? AND deleted_at IS NULL LIMIT 1`,
    [sessionId],
  );
  if (!session) return null;

  const result = await commands.sessionDelete(sessionId);
  if (result.status === "error") {
    console.error("[delete-session] session_delete failed", result.error);
    return null;
  }

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
        ON note.id = COALESCE(
          (
            SELECT store_note.id
            FROM session_documents AS store_note
            WHERE store_note.id = sessions.id || ':note'
              AND store_note.session_id = sessions.id
              AND store_note.kind = 'note'
              AND store_note.deleted_at IS NULL
            LIMIT 1
          ),
          (
            SELECT legacy_note.id
            FROM session_documents AS legacy_note
            WHERE legacy_note.id = sessions.id
              AND legacy_note.session_id = sessions.id
              AND legacy_note.kind = 'note'
              AND legacy_note.deleted_at IS NULL
            LIMIT 1
          )
        )
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
  // The undo toast shows before the delete write commits. Wait for the in-flight
  // delete to settle first -- session_restore looks for a folder under *today's*
  // trash dir, which only exists once session_delete has actually moved it there.
  await waitForPendingSoftDelete(data.session.id);

  const result = await commands.sessionRestore(data.session.id);
  if (result.status === "error") {
    throw new Error(
      `Failed to restore session ${data.session.id}: ${result.error}`,
    );
  }
  if (!result.data) {
    throw new Error(`Session ${data.session.id} was never soft-deleted`);
  }
}

/**
 * @deprecated `session_delete` (the store command `softDeleteSession` now calls)
 * already moves the session folder to `.trash/` atomically, so there is nothing left
 * for this to finalize. Kept only for the rare cross-window path in
 * `useDeleteSession.ts` where a background window's delete commits but the main
 * window never learns about it in time to skip its own finalize call -- calling this
 * on an already-trashed folder is a harmless no-op.
 */
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
  // "markdown" is the legacy-import sentinel; "md" is what `session_write_note` (the store,
  // Tasks 5-8) writes. Both mean "this body is plain markdown, not prosemirror JSON yet".
  if (
    rawMd &&
    (row.raw_body_format === "markdown" || row.raw_body_format === "md")
  ) {
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
  // See mapSessionRow's comment: "md" is `session_write_document`'s format sentinel.
  if (content && (row.body_format === "markdown" || row.body_format === "md")) {
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
