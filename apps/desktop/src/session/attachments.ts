import { commands as fsSyncCommands } from "@hypr/plugin-fs-sync";

import { enqueueSessionAudioOperation } from "./audio-operations";

import { enqueueDatabaseWrite } from "~/shared/write-queue";
import { commands } from "~/types/tauri.gen";

// Settles a just-finished recording at the canonical `<session dir>/audio.<ext>` via the
// session store, and returns where it ended up — the store may relocate it, so callers
// holding the capture backend's path must re-point at this one.
export async function catalogLocalSessionAudio(
  sessionId: string,
  sourcePath: string,
): Promise<string> {
  const result = await commands.sessionStoreAudio(sessionId, sourcePath);
  if (result.status === "error") {
    throw new Error(result.error);
  }
  return result.data;
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
