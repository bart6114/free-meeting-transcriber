import { useCallback, useRef } from "react";

import { commands as analyticsCommands } from "@hypr/plugin-analytics";
import { commands as detectCommands } from "@hypr/plugin-detect";
import { sonnerToast } from "@hypr/ui/components/ui/toast";

import { useListener } from "./contexts";
import { getSessionKeywords } from "./useKeywords";
import {
  canRunBatchTranscription,
  isStoppedTranscriptionError,
  useRunBatch,
} from "./useRunBatch";
import { useSTTConnection } from "./useSTTConnection";

import { requestMainAutoEnhance } from "~/ai/task-window-sync";
import { useShell } from "~/contexts/shell";
import {
  deleteProcessedAudioForRetention,
  normalizeAudioRetention,
} from "~/services/audio-retention";
import { getEnhancerService } from "~/services/enhancer";
import { catalogLocalSessionAudio } from "~/session/attachments";
import { enqueueSessionAudioOperation } from "~/session/audio-operations";
import { useSession, useSessionHasTranscript } from "~/session/queries";
import { useConfigValue } from "~/shared/config";
import { id } from "~/shared/utils";
import type {
  LiveTranscriptPersistCallback,
  OnStoppedCallback,
} from "~/store/zustand/listener/transcript";
import {
  getLiveTranscriptionConfig,
  getTranscriptionLanguages,
} from "~/stt/capabilities";
import { softDeleteTranscript } from "~/stt/queries";
import { commands } from "~/types/tauri.gen";

export const MEETING_DISCLOSURE_MESSAGE =
  "I'm using Free Meeting Transcriber to record and transcribe this meeting.";

const MEETING_DISCLOSURE_MAX_ATTEMPTS = 30;
const MEETING_DISCLOSURE_RETRY_INTERVAL_MS = 1_000;
const SLACK_BUNDLE_IDS = new Set([
  "com.slack.Slack",
  "com.tinyspeck.slackmacgap",
]);

type MeetingDisclosureOutcome =
  | { status: "sent" }
  | { status: "notSent"; reason: string }
  | { status: "cancelled" };

type MeetingDisclosureAttemptOutcome =
  | { status: "sent" }
  | { status: "notSent"; reason: unknown }
  | { status: "cancelled" };

type MeetingDisclosureTask = {
  cancelled: boolean;
  restartWhenSettled?: () => boolean;
  status: "sending" | "sent";
};

const meetingDisclosureTasks = new Map<string, MeetingDisclosureTask>();

function meetingDisclosureFailure(reason: unknown): MeetingDisclosureOutcome {
  const detail = reason instanceof Error ? reason.message : String(reason);
  console.warn("[listener] meeting disclosure was not sent", reason);
  sonnerToast.warning(
    "Recording started, but Free Meeting Transcriber could not post the meeting chat disclosure.",
    { id: "meeting-disclosure-send-failed" },
  );
  return { status: "notSent", reason: detail };
}

async function attemptMeetingRecordingDisclosure(
  isCancelled: () => boolean,
): Promise<MeetingDisclosureAttemptOutcome> {
  if (isCancelled()) {
    return { status: "cancelled" };
  }

  let micAppsResult: Awaited<
    ReturnType<typeof detectCommands.listMicUsingApplications>
  >;

  try {
    micAppsResult = await detectCommands.listMicUsingApplications();
  } catch (error) {
    return isCancelled()
      ? { status: "cancelled" }
      : { status: "notSent", reason: error };
  }

  if (isCancelled()) {
    return { status: "cancelled" };
  }

  if (micAppsResult.status === "error") {
    return { status: "notSent", reason: micAppsResult.error };
  }

  const micActiveBundleIds = [
    ...new Set(micAppsResult.data.map((app) => app.id.trim()).filter(Boolean)),
  ];
  if (!micActiveBundleIds.some((bundleId) => SLACK_BUNDLE_IDS.has(bundleId))) {
    return {
      status: "notSent",
      reason: "no mic-active Slack app was found",
    };
  }

  if (isCancelled()) {
    return { status: "cancelled" };
  }

  let result: Awaited<ReturnType<typeof detectCommands.sendMeetingChatMessage>>;

  try {
    result = await detectCommands.sendMeetingChatMessage(
      MEETING_DISCLOSURE_MESSAGE,
      micActiveBundleIds,
    );
  } catch (error) {
    return isCancelled()
      ? { status: "cancelled" }
      : { status: "notSent", reason: error };
  }

  if (result.status === "error") {
    return isCancelled()
      ? { status: "cancelled" }
      : { status: "notSent", reason: result.error };
  }

  if (result.data.sent) {
    return { status: "sent" };
  }

  if (isCancelled()) {
    return { status: "cancelled" };
  }

  return {
    status: "notSent",
    reason:
      result.data.warnings.join("; ") || "meeting chat mutation was rejected",
  };
}

export async function sendMeetingRecordingDisclosure({
  isCancelled = () => false,
  maxAttempts = MEETING_DISCLOSURE_MAX_ATTEMPTS,
  retryIntervalMs = MEETING_DISCLOSURE_RETRY_INTERVAL_MS,
}: {
  isCancelled?: () => boolean;
  maxAttempts?: number;
  retryIntervalMs?: number;
} = {}): Promise<MeetingDisclosureOutcome> {
  let lastFailureReason: unknown = "meeting chat disclosure was not sent";

  for (let attempt = 0; attempt < Math.max(1, maxAttempts); attempt += 1) {
    const outcome = await attemptMeetingRecordingDisclosure(isCancelled);
    if (outcome.status !== "notSent") {
      return outcome;
    }

    lastFailureReason = outcome.reason;
    if (attempt + 1 < Math.max(1, maxAttempts)) {
      await new Promise<void>((resolve) => {
        setTimeout(resolve, retryIntervalMs);
      });
      if (isCancelled()) {
        return { status: "cancelled" };
      }
    }
  }

  return meetingDisclosureFailure(lastFailureReason);
}

function startMeetingRecordingDisclosure(
  sessionId: string,
  isListening: () => boolean,
) {
  const existingTask = meetingDisclosureTasks.get(sessionId);
  if (existingTask) {
    if (existingTask.status === "sending" && existingTask.cancelled) {
      existingTask.restartWhenSettled = isListening;
    }
    return;
  }

  const task: MeetingDisclosureTask = {
    cancelled: false,
    status: "sending",
  };
  meetingDisclosureTasks.set(sessionId, task);

  void sendMeetingRecordingDisclosure({
    isCancelled: () => task.cancelled || !isListening(),
  }).then((outcome) => {
    if (meetingDisclosureTasks.get(sessionId) !== task) {
      return;
    }

    if (outcome.status === "sent") {
      task.status = "sent";
    } else {
      const restartWhenSettled = task.restartWhenSettled;
      meetingDisclosureTasks.delete(sessionId);
      if (restartWhenSettled?.()) {
        startMeetingRecordingDisclosure(sessionId, restartWhenSettled);
      }
    }
  });
}

function cancelMeetingRecordingDisclosure(sessionId: string) {
  const task = meetingDisclosureTasks.get(sessionId);
  if (!task || task.status === "sent") {
    return;
  }

  task.cancelled = true;
}

export function getPostCaptureAction(
  details: {
    audioPath: string | null;
    liveTranscriptionActive: boolean;
    needsBatchRepair: boolean;
  },
  canRunBatch: boolean,
) {
  if (details.liveTranscriptionActive && !details.needsBatchRepair) {
    return "enhance_only" as const;
  }

  if (!!details.audioPath && canRunBatch) {
    return "batch_then_enhance" as const;
  }

  return "none" as const;
}

export function useStartListening(sessionId: string) {
  const session = useSession(sessionId);
  const hadTranscriptBeforeStart = useSessionHasTranscript(sessionId);
  const getSessionMode = useListener((state) => state.getSessionMode);

  const aiLanguage = useConfigValue("ai_language");
  const spokenLanguages = useConfigValue("spoken_languages");
  const dictionaryTerms = useConfigValue("personalization_dictionary_terms");
  const audioRetention = normalizeAudioRetention(
    useConfigValue("audio_retention"),
  );
  const meetingDisclosureAutoSendChat = useConfigValue(
    "consent_auto_send_chat",
  );

  const start = useListener((state) => state.start);
  const { conn } = useSTTConnection();
  const runBatch = useRunBatch(sessionId);
  const { leftsidebar } = useShell();
  const setLeftSidebarExpanded = leftsidebar.setExpanded;

  const runBatchRef = useRef(runBatch);
  const canRunBatchRef = useRef(canRunBatchTranscription(conn));
  runBatchRef.current = runBatch;
  canRunBatchRef.current = canRunBatchTranscription(conn);

  const startListening = useCallback(async () => {
    let transcriptId: string | null = null;
    const startedAt = Date.now();
    let lastTranscriptWrite = Promise.resolve();
    let transcriptWriteError: unknown;
    const reportTranscriptWriteError = (error: unknown) => {
      transcriptWriteError = error;
      console.error("[listener] failed to persist transcript", error);
      sonnerToast.error(`Transcript is NOT being saved: ${error}`, {
        id: "live-transcript-persist-failed",
        duration: Infinity,
      });
    };
    const trackTranscriptWrite = (write: Promise<void>) => {
      lastTranscriptWrite = write.catch(reportTranscriptWriteError);
    };
    const keywords = await getSessionKeywords({
      sessionId,
      dictionaryTerms,
    });

    let audioCatalogFailed = false;
    const onStopped: OnStoppedCallback = async (_sessionId, details) => {
      cancelMeetingRecordingDisclosure(sessionId);
      if (details.audioPath) {
        const audioPath = details.audioPath;
        try {
          await enqueueSessionAudioOperation(sessionId, () =>
            catalogLocalSessionAudio(sessionId, audioPath),
          );
        } catch (error) {
          audioCatalogFailed = true;
          console.error("[listener] failed to catalog recorded audio", error);
          sonnerToast.error(
            "Recording audio could not be moved into the session folder — it remains at its original location",
            { id: "audio-catalog-failed" },
          );
        }
      }
      await lastTranscriptWrite;
      if (transcriptId) {
        try {
          const result = await commands.sessionFlushTranscript(sessionId);
          if (result.status === "error") throw new Error(result.error);
        } catch (error) {
          reportTranscriptWriteError(error);
        }
      }

      const postCaptureAction = getPostCaptureAction(
        details,
        canRunBatchRef.current,
      );

      let batchCompleted = false;
      if (postCaptureAction === "batch_then_enhance") {
        try {
          await runBatchRef.current(details.audioPath!);
          batchCompleted = true;
        } catch (error) {
          if (isStoppedTranscriptionError(error)) {
            return;
          }
          console.error(
            "[listener] failed to run post-capture transcription",
            error,
          );
          sonnerToast.error(
            "Post-meeting transcription failed. Summarizing the live transcript instead.",
            { id: "post-capture-batch-failed" },
          );
        }
      }

      const hasTranscriptEvidence =
        hadTranscriptBeforeStart || transcriptId !== null || batchCompleted;
      if (postCaptureAction !== "none" || hasTranscriptEvidence) {
        const shouldRegenerateExistingSummary =
          hadTranscriptBeforeStart && (transcriptId !== null || batchCompleted);
        const service = getEnhancerService();
        if (!service) {
          await requestMainAutoEnhance(
            sessionId,
            shouldRegenerateExistingSummary ? "regenerate" : "if_empty",
          );
        } else if (shouldRegenerateExistingSummary) {
          await service.resetEnhanceTasks(sessionId);
          service.queueAutoEnhance(sessionId);
        } else {
          await service.queueAutoEnhanceIfSummaryEmpty(sessionId);
        }
      }

      // A failed batch repair, a live transcript that never fully persisted, or an audio file
      // that never made it into the session folder all keep the recording around as the only
      // (or only correctly-located) source for a later repair, regardless of retention policy.
      if (
        (postCaptureAction !== "batch_then_enhance" || batchCompleted) &&
        !transcriptWriteError &&
        !audioCatalogFailed
      ) {
        await deleteProcessedAudioForRetention(audioRetention, sessionId);
      }
    };

    const handlePersist: LiveTranscriptPersistCallback = (delta) => {
      if (delta.new_words.length === 0 && delta.replaced_ids.length === 0) {
        return;
      }
      if (!transcriptId) transcriptId = id();

      trackTranscriptWrite(
        commands
          .sessionAppendTranscript(sessionId, {
            transcript_id: transcriptId,
            new_words: delta.new_words,
            replaced_ids: delta.replaced_ids,
            // Live deltas from the transcription plugin carry no speaker-hint data (that's
            // produced by the separate batch/assignment paths) -- nothing to forward here.
            new_hints: [],
            started_at_ms: startedAt,
          })
          .then((result) => {
            if (result.status === "error") throw new Error(result.error);
          }),
      );
    };

    const languages = getTranscriptionLanguages(aiLanguage, spokenLanguages);
    const liveTranscriptionConfig = await getLiveTranscriptionConfig({
      provider: conn?.provider,
      model: conn?.model,
      languages,
    });

    const started = await start(
      {
        session_id: sessionId,
        languages: liveTranscriptionConfig.languages,
        onboarding: false,
        model: conn?.model ?? "",
        base_url: conn?.baseUrl ?? "",
        api_key: conn?.apiKey ?? "",
        keywords,
        transcription_mode: liveTranscriptionConfig.transcriptionMode,
        participant_human_ids: [],
        self_human_id: session?.user_id || null,
      },
      {
        handlePersist,
        onStopped,
      },
    );

    if (!started) {
      await lastTranscriptWrite;
      if (transcriptId) {
        await softDeleteTranscript(sessionId, transcriptId);
      }
      return;
    }

    setLeftSidebarExpanded(false);

    if (meetingDisclosureAutoSendChat) {
      startMeetingRecordingDisclosure(
        sessionId,
        () => getSessionMode(sessionId) === "active",
      );
    }

    void analyticsCommands.event({
      event: "session_started",
      ...(conn
        ? {
            stt_provider: conn.provider,
            stt_model: conn.model,
          }
        : {}),
    });
  }, [
    aiLanguage,
    audioRetention,
    conn,
    dictionaryTerms,
    getSessionMode,
    hadTranscriptBeforeStart,
    session,
    sessionId,
    setLeftSidebarExpanded,
    meetingDisclosureAutoSendChat,
    spokenLanguages,
    start,
  ]);

  return startListening;
}
