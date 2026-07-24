import { commands as fsSyncCommands } from "@hypr/plugin-fs-sync";

import { enqueueSessionAudioOperation } from "./audio-operations";

import { enqueueDatabaseWrite } from "~/db/write-queue";
import { commands } from "~/types/tauri.gen";

// Graceful no-op: `session_attachments`/`attachment_local_state` were
// dropped in Task 4 (cloudsync/e2ee/workspaces/sharing removal). Note
// attachments (as opposed to recording audio, see catalogLocalSessionAudio
// below) have no file-canonical home yet — deferred past Task 9. The
// uploaded file itself is already saved by the caller (fs-sync) before this
// is called — only the DB-side bookkeeping is skipped here.
export async function catalogLocalNoteAttachment(_input: {
  sessionId: string;
  attachmentId: string;
  filename: string;
  contentType: string;
  sizeBytes: number;
  sha256: string;
}): Promise<void> {}

// Moves a just-finished recording from wherever the capture backend wrote it
// into `sessions/<id>/audio/<filename>` via the session store.
export async function catalogLocalSessionAudio(
  sessionId: string,
  sourcePath: string,
): Promise<void> {
  const result = await commands.sessionStoreAudio(sessionId, sourcePath);
  if (result.status === "error") {
    throw new Error(result.error);
  }
}

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
  // Two locations can hold audio for a session: the legacy flat
  // `sessions/<id>/<file>` layout (uploads/imports, `fs-sync`-owned) and the
  // store-owned `sessions/<id>/audio/` folder that recordings land in as of
  // Task 9. Both are cleared so retention doesn't leave orphaned recordings
  // behind depending on which path produced them.
  const flatResult = await fsSyncCommands.audioDelete(sessionId);
  if (flatResult.status === "error") {
    throw new Error(flatResult.error);
  }
  const folderDeleted = await deleteSessionAudioFolder(sessionId);
  await markSessionAudioAvailability(sessionId, "absent");
  return flatResult.data || folderDeleted;
}

async function deleteSessionAudioFolder(sessionId: string): Promise<boolean> {
  const listResult = await commands.sessionListAudio(sessionId);
  if (listResult.status === "error") {
    throw new Error(listResult.error);
  }
  if (listResult.data.length === 0) {
    return false;
  }
  for (const filename of listResult.data) {
    const deleteResult = await commands.sessionDeleteAudio(sessionId, filename);
    if (deleteResult.status === "error") {
      throw new Error(deleteResult.error);
    }
  }
  return true;
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
