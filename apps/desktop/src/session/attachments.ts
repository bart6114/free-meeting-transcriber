import { commands as fsSyncCommands } from "@hypr/plugin-fs-sync";

import { enqueueSessionAudioOperation } from "./audio-operations";

import { executeTransaction, liveQueryClient } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";

// Graceful no-op: `session_attachments`/`attachment_local_state` were
// dropped in Task 4 (cloudsync/e2ee/workspaces/sharing removal). Attachment
// cataloging gets rewired to `sessions/<id>/audio/` and
// `sessions/<id>/attachments/` on disk in Task 9 (Session store scaffold) of
// the filesystem-first-sessions plan. The uploaded/recorded file itself is
// already saved by the caller (fs-sync) before this is called — only the
// DB-side bookkeeping is skipped here.
export async function catalogLocalNoteAttachment(_input: {
  sessionId: string;
  attachmentId: string;
  filename: string;
  contentType: string;
  sizeBytes: number;
  sha256: string;
}): Promise<void> {}

// See catalogLocalNoteAttachment above.
export async function catalogLocalSessionAudio(
  _inputSessionId: string,
): Promise<void> {}

export async function deleteLocalSessionAudio(
  inputSessionId: string,
  canDelete: () => boolean,
): Promise<boolean> {
  return deleteSessionAudioWithMode(inputSessionId, false, canDelete);
}

export async function deleteSessionAudio(
  inputSessionId: string,
  canDelete: () => boolean,
): Promise<boolean> {
  return deleteSessionAudioWithMode(inputSessionId, true, canDelete);
}

export async function cleanupDeletedSessionAudio(
  inputSessionId: string,
  canDelete: () => boolean,
): Promise<boolean> {
  const sessionId = requireText(inputSessionId, "session ID", 512);
  return enqueueSessionAudioOperation(sessionId, () =>
    enqueueDatabaseWrite(`session:${sessionId}`, async () => {
      if (!canDelete()) {
        return false;
      }

      const rows = await liveQueryClient.execute<{ is_deleted: number }>(
        `
          SELECT EXISTS(
            SELECT 1
            FROM session_attachments
            WHERE id = ?
              AND session_id = ?
              AND deleted_at IS NOT NULL
              AND NOT EXISTS (
                SELECT 1
                FROM attachment_local_state AS local
                WHERE local.attachment_id = session_attachments.id
                  AND local.availability = 'absent'
              )
          ) AS is_deleted
        `,
        [`session-audio:${sessionId}`, sessionId],
      );
      if (rows[0]?.is_deleted !== 1) {
        return false;
      }

      return deleteSessionAudioFile(sessionId);
    }),
  );
}

async function deleteSessionAudioWithMode(
  inputSessionId: string,
  deleteMetadata: boolean,
  canDelete: () => boolean,
): Promise<boolean> {
  const sessionId = requireText(inputSessionId, "session ID", 512);
  return enqueueSessionAudioOperation(sessionId, () =>
    enqueueDatabaseWrite(`session:${sessionId}`, async () => {
      if (!canDelete()) {
        return false;
      }
      if (deleteMetadata) {
        await tombstoneSessionAudioMetadata(sessionId);
      }
      const deletedLocalFile = await deleteSessionAudioFile(sessionId);
      return deleteMetadata || deletedLocalFile;
    }),
  );
}

async function deleteSessionAudioFile(sessionId: string): Promise<boolean> {
  const result = await fsSyncCommands.audioDelete(sessionId);
  if (result.status === "error") {
    throw new Error(result.error);
  }
  await markSessionAudioAvailability(sessionId, "absent");
  return result.data;
}

async function markSessionAudioAvailability(
  sessionId: string,
  availability: "present" | "absent",
): Promise<void> {
  await executeTransaction([
    {
      sql: `
        INSERT INTO attachment_local_state (
          attachment_id,
          session_id,
          relative_path,
          availability,
          updated_at
        ) VALUES (?, ?, '', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        ON CONFLICT(attachment_id) DO UPDATE SET
          session_id = excluded.session_id,
          availability = excluded.availability,
          updated_at = excluded.updated_at
      `,
      params: [`session-audio:${sessionId}`, sessionId, availability],
    },
  ]);
}

async function tombstoneSessionAudioMetadata(sessionId: string): Promise<void> {
  await executeTransaction([
    {
      sql: `
        UPDATE session_attachments
        SET
          updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
          deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        WHERE id = ?
          AND session_id = ?
          AND deleted_at IS NULL
      `,
      params: [`session-audio:${sessionId}`, sessionId],
    },
  ]);
}

export async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

function requireText(
  value: unknown,
  label: string,
  maxLength: number,
  allowEmpty = false,
) {
  if (
    typeof value !== "string" ||
    (!allowEmpty && value.length === 0) ||
    value.length > maxLength ||
    value.trim() !== value ||
    /[\u0000-\u001f\u007f]/.test(value)
  ) {
    throw new Error(`invalid ${label}`);
  }
  return value;
}
