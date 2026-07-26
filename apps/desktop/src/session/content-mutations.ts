import { executeTransaction, liveQueryClient } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";
import { DEFAULT_USER_ID } from "~/shared/utils";
import { commands } from "~/types/tauri.gen";

export type SessionDocumentContentUpdate = {
  id: string;
  currentContent: string;
  currentContentFormat: string;
  nextContent: string;
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
    const statements: Array<{
      sql: string;
      params: unknown[];
      expectedRowsAffected: number;
    }> = [
      {
        sql: `
          UPDATE session_documents
          SET body = ?, body_format = 'prosemirror_json', updated_at = ?
          WHERE id = ?
            AND session_id = ?
            AND kind IN ('summary', 'template_output')
            AND body = ?
            AND body_format = ?
            AND deleted_at IS NULL
            AND EXISTS (
              SELECT 1 FROM sessions
              WHERE sessions.id = ?
            )
        `,
        params: [
          note.nextContent,
          now,
          note.id,
          sessionId,
          note.currentContent,
          note.currentContentFormat,
          sessionId,
        ],
        expectedRowsAffected: 1,
      },
    ];

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

    await executeTransaction(statements);

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
    const now = new Date().toISOString();

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

    const statements: Array<{
      sql: string;
      params: unknown[];
      expectedRowsAffected: number;
    }> = [];

    for (const document of documents) {
      statements.push({
        sql: `
          UPDATE session_documents
          SET body = ?, body_format = 'prosemirror_json', updated_at = ?
          WHERE id = ?
            AND session_id = ?
            AND kind IN ('summary', 'template_output')
            AND body = ?
            AND body_format = ?
            AND deleted_at IS NULL
        `,
        params: [
          document.nextContent,
          now,
          document.id,
          sessionId,
          document.currentContent,
          document.currentContentFormat,
        ],
        expectedRowsAffected: 1,
      });
    }

    // Documents first: a stale document guard throws here and the store-canonical title
    // write below never happens, matching the old transaction's all-or-nothing rollback.
    if (statements.length > 0) {
      await executeTransaction(statements);
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
