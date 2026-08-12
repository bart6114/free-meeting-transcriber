import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import { TRANSCRIPT_RENDER_CACHE_TIME_MS } from "../cache";
import { useTranscriptRenderData } from "../render-request-hooks";

import {
  getMaxSpeakerNumberForParticipants,
  type Segment,
} from "~/stt/live-segment";
import { useSessionTranscripts, useTranscript } from "~/stt/queries";
import {
  getRenderTranscriptRequestKey,
  renderRequestHasDiarizedChannel,
  renderTranscriptSegments,
} from "~/stt/render-transcript";

export function useRenderedTranscriptSegments(transcriptId: string): Segment[] {
  return useRenderedTranscriptData(transcriptId).segments;
}

export function useRenderedTranscriptData(transcriptId: string): {
  maxSpeakerNumber?: number;
  segments: Segment[];
} {
  const { request } = useTranscriptRenderData(transcriptId);
  const requestKey = useMemo(
    () => getRenderTranscriptRequestKey(request),
    [request],
  );

  // eslint-disable-next-line @tanstack/query/exhaustive-deps -- requestKey is the canonical hash of the complete render request.
  const { data = [] } = useQuery({
    queryKey: ["rendered-transcript-segments", transcriptId, requestKey],
    queryFn: async () => {
      if (!request) {
        return [];
      }

      return renderTranscriptSegments(request);
    },
    enabled: !!request,
    // Keep the previous segments on screen while a changed request re-renders,
    // so a speaker rename doesn't blank the transcript and remount every segment.
    placeholderData: keepPreviousData,
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: TRANSCRIPT_RENDER_CACHE_TIME_MS,
  });

  const maxSpeakerNumber = useMemo(
    () =>
      // Diarization can surface more speakers than the participant list;
      // capping would merge two distinct diarized speakers under one label.
      request && !renderRequestHasDiarizedChannel(request)
        ? getMaxSpeakerNumberForParticipants(
            request.participant_human_ids,
            request.self_human_id,
          )
        : undefined,
    [request],
  );

  return { maxSpeakerNumber, segments: data };
}

export function useTranscriptOffset(transcriptId: string): number {
  const transcript = useTranscript(transcriptId);
  const transcripts = useSessionTranscripts(transcript?.sessionId ?? "");

  return useMemo(() => {
    if (!transcript) {
      return 0;
    }

    const earliestStartedAt = Math.min(
      ...transcripts.map((current) => current.startedAt),
    );

    return Number.isFinite(earliestStartedAt)
      ? transcript.startedAt - earliestStartedAt
      : 0;
  }, [transcript, transcripts]);
}
