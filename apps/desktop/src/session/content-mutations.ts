import { executeTransaction, liveQueryClient } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";
import { DEFAULT_USER_ID } from "~/shared/utils";
import { commands } from "~/types/tauri.gen";

// Markdown-based since D-3: `enhanced/<doc-id>.md` is the doc's canonical home, so the
// compare-and-swap runs against the file's markdown body (the store rejects with a
// "conflict:" error when `currentMarkdown` is stale), not against the SQL row.
export type SessionDocumentContentUpdate = {
  id: string;
  currentMarkdown: string;
  nextMarkdown: string;
};

export function persistGeneratedEnhancedNote({
  sessionId,
  ownerUserId,
  note,
  tagNames,
}: {
  sessionId: string;
  ownerUserId: string;
  note: SessionDocumentContentUpdate;
  tagNames: string[];
}): Promise<void> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    const now = new Date().toISOString();
    const userId = ownerUserId.trim() || DEFAULT_USER_ID;
    const normalizedTagNames = [...new Set(tagNames)].filter(Boolean);

    // File-first with the same staleness contract the old guarded UPDATE had: a stale
    // `currentMarkdown` (reset/regenerate replaced the summary meanwhile) rejects and
    // nothing below runs. A missing doc file (session or doc deleted) rejects too,
    // replacing the old `expectedRowsAffected`/`EXISTS(sessions)` guards.
    const docWrite = await commands.sessionUpdateEnhancedDoc(
      sessionId,
      note.id,
      {
        markdown: note.nextMarkdown,
        expected_markdown: note.currentMarkdown,
      },
    );
    if (docWrite.status === "error") {
      throw new Error(
        `Failed to persist generated summary ${note.id}: ${docWrite.error}`,
      );
    }

    const statements: Array<{
      sql: string;
      params: unknown[];
      expectedRowsAffected: number;
    }> = [];

    for (const tagName of normalizedTagNames) {
      statements.push(
        {
          sql: `
            INSERT INTO tags (
              id, owner_user_id, name, created_at, updated_at, deleted_at
            ) VALUES (?, ?, ?, ?, ?, NULL)
            ON CONFLICT(id) DO UPDATE SET
              owner_user_id = excluded.owner_user_id,
              name = excluded.name,
              updated_at = excluded.updated_at,
              deleted_at = NULL
          `,
          params: [tagName, userId, tagName, now, now],
          expectedRowsAffected: 1,
        },
        {
          sql: `
            INSERT INTO session_tags (
              id, owner_user_id, session_id, tag_id,
              created_at, updated_at, deleted_at
            ) VALUES (?, ?, ?, ?, ?, ?, NULL)
            ON CONFLICT(id) DO UPDATE SET
              owner_user_id = excluded.owner_user_id,
              session_id = excluded.session_id,
              tag_id = excluded.tag_id,
              updated_at = excluded.updated_at,
              deleted_at = NULL
          `,
          params: [
            `${sessionId}:${tagName}`,
            userId,
            sessionId,
            tagName,
            now,
            now,
          ],
          expectedRowsAffected: 1,
        },
      );
    }

    if (statements.length > 0) {
      await executeTransaction(statements);
    }

    // Dual-write the session's full tag set into `_meta.json` (file-canonical). The
    // tag/session_tags upserts above are additive, so the resulting set is whatever SQL now
    // holds -- read it back (ordered, for stable file content) rather than guessing a merge.
    // Best-effort: `isSessionEmpty` and every tag reader stay on SQL until Phase E, so a
    // failed meta write must not fail the whole enhanced-note persist.
    if (normalizedTagNames.length > 0) {
      const tagRows = await liveQueryClient.execute<{ tag_id: string }>(
        `
          SELECT tag_id
          FROM session_tags
          WHERE session_id = ? AND deleted_at IS NULL
          ORDER BY tag_id
        `,
        [sessionId],
      );
      const result = await commands.sessionUpdateMeta(sessionId, {
        tags: tagRows.map((row) => row.tag_id),
      });
      if (result.status === "error") {
        console.error(
          "[content-mutations] failed to write tags into session meta",
          result.error,
        );
      }
    }
  });
}

// `documents` here is summary/template_output enhanced notes only -- the raw note is stamped
// separately, file-first, by `applyGeneratedNoteTitle` in title-success.ts (it reads/writes
// through session_read_note/session_write_note, never raw SQL, since the editor reads the file
// as of Task 9's file-canonical note-load path).
export function applyGeneratedSessionTitle({
  sessionId,
  currentTitle,
  nextTitle,
  documents,
}: {
  sessionId: string;
  currentTitle: string;
  nextTitle: string;
  documents: SessionDocumentContentUpdate[];
}): Promise<void> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    // Same compare-and-swap the old single-transaction title UPDATE gave us, kept honest by
    // the write queue: everything that mutates this session's title serializes through the
    // `session:<id>` queue key, so check-then-write can't interleave with a user edit. A
    // stale generation (user renamed meanwhile) must apply nothing at all.
    const [session] = await liveQueryClient.execute<{ title: string }>(
      `SELECT title FROM sessions WHERE id = ? LIMIT 1`,
      [sessionId],
    );
    if (!session || session.title !== currentTitle) {
      throw new Error(
        `[content-mutations] session title changed while generating; not applying "${nextTitle}"`,
      );
    }

    // Documents first: a stale document guard (the store's "conflict:" CAS rejection, the
    // file-era equivalent of the old expectedRowsAffected rollback) throws here and the
    // store-canonical title write below never happens.
    for (const document of documents) {
      const docWrite = await commands.sessionUpdateEnhancedDoc(
        sessionId,
        document.id,
        {
          markdown: document.nextMarkdown,
          expected_markdown: document.currentMarkdown,
        },
      );
      if (docWrite.status === "error") {
        throw new Error(
          `Failed to stamp title into summary ${document.id}: ${docWrite.error}`,
        );
      }
    }

    // Title last, through the store (file-first + SQL dual-write): `_meta.json` is canonical
    // for session meta, so this must never be a raw `UPDATE sessions`.
    const result = await commands.sessionUpdateMeta(sessionId, {
      title: nextTitle,
    });
    if (result.status === "error") {
      throw new Error(
        `Failed to update session ${sessionId} title: ${result.error}`,
      );
    }
  });
}
