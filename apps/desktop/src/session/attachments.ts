import { commands as fsSyncCommands } from "@hypr/plugin-fs-sync";

import { enqueueSessionAudioOperation } from "./audio-operations";

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

// Graceful no-op: `session_attachments`/`attachment_local_state` were
// dropped in Task 4 (cloudsync/e2ee/workspaces/sharing removal), so there is
// no longer a way to detect a logically-deleted-but-locally-present
// attachment to retry cleanup on. Its only caller
// (`cleanupLogicallyDeletedAudio` in audio-retention.ts) is already a
// no-op pending Task 9 (Session store scaffold).
export async function cleanupDeletedSessionAudio(
  _inputSessionId: string,
  _canDelete: () => boolean,
): Promise<boolean> {
  return false;
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

// Graceful no-op: `attachment_local_state` was dropped in Task 4
// (cloudsync/e2ee/workspaces/sharing removal). Local-availability
// bookkeeping gets rewired to `sessions/<id>/audio/` on disk in Task 9
// (Session store scaffold) of the filesystem-first-sessions plan. The
// on-disk file deletion this accompanies (`deleteSessionAudioFile`) is
// unaffected — only this DB-side bookkeeping is skipped.
async function markSessionAudioAvailability(
  _sessionId: string,
  _availability: "present" | "absent",
): Promise<void> {}

// See markSessionAudioAvailability above — `session_attachments` was
// likewise dropped in Task 4; the on-disk file deletion this accompanies is
// unaffected.
async function tombstoneSessionAudioMetadata(
  _sessionId: string,
): Promise<void> {}

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
