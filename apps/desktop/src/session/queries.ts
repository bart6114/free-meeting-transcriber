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
import { commands, type SessionMetaPatch } from "~/types/tauri.gen";

type SessionDeleteSqlRow = { id: string; title: string };
type SessionEmptySqlRow = {
  title: string;
  event_json: string;
  note_body: string;
  note_body_format: string;
  transcript_count: number;
  enhanced_note_count: number;
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
  WHERE sessions.id = ?
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
      WHERE id = ?
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
    // The file home is markdown-canonical; the editor hands us prosemirror JSON. Content
    // that doesn't parse is already markdown (defensive -- the enhanced editor always
    // serializes JSON today).
    let markdown = content;
    try {
      markdown = json2md(JSON.parse(content));
    } catch {
      // keep `content` as-is
    }

    // File-first: `enhanced/<doc-id>.md` is canonical, and the store's dual-write keeps
    // the `session_documents` row (still read by Phase-E-pending live queries and search)
    // in sync -- a raw UPDATE here would leave the file stale for the next rebuild.
    const docWrite = await commands.sessionUpdateEnhancedDoc(
      sessionId,
      enhancedNoteId,
      { markdown },
    );
    if (docWrite.status === "error") {
      throw new Error(
        `Failed to update summary ${enhancedNoteId}: ${docWrite.error}`,
      );
    }

    // Session title is store-canonical (`_meta.json`), so it rides its own store call --
    // the store's dual-write updates the sessions row itself.
    if (sessionTitle !== undefined) {
      const result = await commands.sessionUpdateMeta(sessionId, {
        title: sessionTitle,
      });
      if (result.status === "error") {
        throw new Error(
          `Failed to update session ${sessionId} title: ${result.error}`,
        );
      }
    }
  });
}

export function deleteEnhancedNote(
  enhancedNoteId: string,
  sessionId: string,
): Promise<void> {
  return enqueueDatabaseWrite(`enhanced-note:${enhancedNoteId}`, async () => {
    // The store moves `enhanced/<doc-id>.md` to `.trash/` (hand-recoverable) and
    // hard-deletes the index row -- no tombstone, since no undo path exists for enhanced
    // notes and rebuild prunes file-less rows anyway.
    const result = await commands.sessionDeleteEnhancedDoc(
      sessionId,
      enhancedNoteId,
    );
    if (result.status === "error") {
      throw new Error(
        `Failed to delete summary ${enhancedNoteId}: ${result.error}`,
      );
    }
  });
}

// File-first: `_meta.json` is canonical for session meta, so every change goes through the
// store's `session_update_meta` (read-modify-write of the file, then the SQL dual-write) --
// a raw `UPDATE sessions` here would leave the file stale and the next rebuild would revert
// the change to the old file value.
export function updateSession(
  sessionId: string,
  changes: SessionChanges,
): Promise<void> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    const patch: SessionMetaPatch = {};
    if (changes.title !== undefined) patch.title = changes.title;
    if (changes.created_at !== undefined) patch.created_at = changes.created_at;
    if (changes.folder_id !== undefined) patch.folder = changes.folder_id;
    if (changes.event_json) patch.event = JSON.parse(changes.event_json);

    if (Object.keys(patch).length === 0) return;

    const result = await commands.sessionUpdateMeta(sessionId, patch);
    if (result.status === "error") {
      throw new Error(`Failed to update session ${sessionId}: ${result.error}`);
    }
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
    // The event rides the store write itself (never a separate SQL UPDATE): `_meta.json` is
    // canonical, and the store's dual-write seeds the sessions row's event_json.
    event: initial?.event_json ? JSON.parse(initial.event_json) : null,
    folder: null,
  });
  if (metaWrite.status === "error") {
    throw new Error(
      `Failed to create session ${sessionId}: ${metaWrite.error}`,
    );
  }

  // Bookkeeping placeholder only: always an empty body. Canonical-id-vs-fallback reads
  // (SESSION_SELECT_SQL above, useKeywords.ts, isSessionEmpty below) COALESCE onto a
  // store-written "<id>:note" row when this bare-id row is empty, so this just keeps a
  // row present for the join -- real note content lives only in the file-canonical store
  // from here on (sessionWriteNote below), never in this row's body.
  await executeTransaction([createEmptyNoteStatement(sessionId, now, "")]);

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
    `SELECT id, title FROM sessions WHERE id = ? LIMIT 1`,
    [sessionId],
  );
  if (!session) return null;

  const result = await commands.sessionDelete(sessionId);
  if (result.status === "error") {
    // Distinct from "already deleted" (the pre-check SELECT above returning no rows, which is
    // benign and returns null): the session existed and the store call itself failed. Throw so
    // useDeleteSession's existing catch rolls back the optimistic UI and shows an error toast --
    // a genuine command error must never look identical to a no-op idempotent delete.
    throw new Error(`session_delete failed: ${result.error}`);
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
      WHERE sessions.id = ?
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
        updated_by, updated_at, deleted_at
      )
      SELECT ?, id, 'note', 'prosemirror_json', ?,
        owner_user_id, owner_user_id, ?, NULL
      FROM sessions
      WHERE id = ?
    `,
    params: [sessionId, body, now, sessionId],
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
