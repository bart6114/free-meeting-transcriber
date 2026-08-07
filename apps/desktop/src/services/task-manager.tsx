import { useQueryClient } from "@tanstack/react-query";
import { useSetTask, useScheduleTaskRun } from "tinytick/ui-react";

import {
  AUDIO_RETENTION_INTERVAL,
  AUDIO_RETENTION_TASK_ID,
  cleanupExpiredAudio,
  normalizeAudioRetention,
} from "./audio-retention";

import { useConfigValue } from "~/shared/config";

// tinytick aborts runs after a default maxDuration of 1s and never re-schedules
// the repeatDelay of a timed-out run, which permanently kills the repeating
// task. Long-running tasks must set an explicit maxDuration, and retries keep
// the repeat loop alive if a run still exceeds it.
const AUDIO_RETENTION_MAX_DURATION = 10 * 60 * 1000; // 10 min
const REPEATING_TASK_MAX_RETRIES = 3;

export function TaskManager() {
  const queryClient = useQueryClient();

  const audioRetention = normalizeAudioRetention(
    useConfigValue("audio_retention"),
  );

  useSetTask(
    AUDIO_RETENTION_TASK_ID,
    async () => {
      const deletedSessionIds = await cleanupExpiredAudio(audioRetention);
      for (const sessionId of deletedSessionIds) {
        void queryClient.invalidateQueries({
          queryKey: ["audio", sessionId],
        });
      }
    },
    [audioRetention, queryClient],
    undefined,
    {
      maxDuration: AUDIO_RETENTION_MAX_DURATION,
      maxRetries: REPEATING_TASK_MAX_RETRIES,
      retryDelay: AUDIO_RETENTION_INTERVAL,
    },
  );

  useScheduleTaskRun(AUDIO_RETENTION_TASK_ID, undefined, 0, {
    repeatDelay: AUDIO_RETENTION_INTERVAL,
  });

  return null;
}
