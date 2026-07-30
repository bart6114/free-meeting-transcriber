import {
  AUDIO_RETENTION_DURATION_MS,
  type AudioRetentionPolicy,
} from "./audio-retention-policy";

import { deleteLocalSessionAudio } from "~/session/attachments";
import { listenerStore } from "~/store/zustand/listener/instance";
import { commands } from "~/types/tauri.gen";

export const AUDIO_RETENTION_TASK_ID = "audio-retention-cleanup";
export const AUDIO_RETENTION_INTERVAL = 60 * 1000;

export {
  normalizeAudioRetention,
  type AudioRetentionPolicy,
} from "./audio-retention-policy";

export function sessionAudioExpired(
  createdAt: unknown,
  policy: AudioRetentionPolicy,
  nowMs = Date.now(),
) {
  if (policy === "forever") {
    return false;
  }

  if (policy === "none") {
    return true;
  }

  if (typeof createdAt !== "string") {
    return false;
  }

  const createdAtMs = Date.parse(createdAt);
  if (!Number.isFinite(createdAtMs)) {
    return false;
  }

  return nowMs >= createdAtMs + AUDIO_RETENTION_DURATION_MS[policy];
}

export function isSessionAudioIdle(sessionId: string) {
  const state = listenerStore.getState();
  return (
    state.getSessionMode(sessionId) === "inactive" &&
    !(state.live.sessionId === sessionId && state.live.loading)
  );
}

async function sessionHasTranscriptWords(sessionId: string): Promise<boolean> {
  const result = await commands.sessionHasTranscript(sessionId);
  if (result.status === "error") {
    throw new Error(result.error);
  }
  return result.data;
}

export async function deleteProcessedAudioForRetention(
  policy: AudioRetentionPolicy,
  sessionId: string,
) {
  if (policy !== "none") {
    return false;
  }

  if (!isSessionAudioIdle(sessionId)) {
    return false;
  }

  if (!(await sessionHasTranscriptWords(sessionId))) {
    return false;
  }

  try {
    return await deleteLocalSessionAudio(sessionId, () =>
      isSessionAudioIdle(sessionId),
    );
  } catch (error) {
    console.error("[audio-retention] failed to delete audio", {
      sessionId,
      error,
    });
    return false;
  }
}

export async function cleanupExpiredAudio(
  policy: AudioRetentionPolicy,
  nowMs = Date.now(),
) {
  const deletedSessionIds = await cleanupLogicallyDeletedAudio();
  if (policy === "forever") {
    return deletedSessionIds;
  }

  const deletes: Promise<void>[] = [];
  const result = await commands.sessionList();
  if (result.status === "error") {
    throw new Error(result.error);
  }

  for (const entry of result.data) {
    const sessionId = entry.meta.id;
    if (!isSessionAudioIdle(sessionId)) {
      continue;
    }

    if (policy === "none" && !entry.has_transcript_words) {
      continue;
    }

    if (!sessionAudioExpired(entry.meta.created_at, policy, nowMs)) {
      continue;
    }

    deletes.push(
      deleteLocalSessionAudio(sessionId, () => isSessionAudioIdle(sessionId))
        .then((deleted) => {
          if (deleted) {
            deletedSessionIds.push(sessionId);
          }
        })
        .catch((error) => {
          console.error("[audio-retention] failed to delete audio", {
            sessionId,
            error,
          });
        }),
    );
  }

  await Promise.all(deletes);

  return deletedSessionIds;
}

// Graceful no-op: `session_attachments`/`attachment_local_state` were
// dropped in Task 4 (cloudsync/e2ee/workspaces/sharing removal). Audio
// retention gets rewired to scan `sessions/<id>/audio/` directly in Task 9
// (Session store scaffold) of the filesystem-first-sessions plan.
async function cleanupLogicallyDeletedAudio(): Promise<string[]> {
  return [];
}
