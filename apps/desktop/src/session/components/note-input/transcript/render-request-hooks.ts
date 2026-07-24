import { useMemo } from "react";

import type { RenderTranscriptRequest } from "@hypr/plugin-transcription";

import {
  type TranscriptRecord,
  useSessionTranscripts,
  useTranscript,
} from "~/stt/queries";
import {
  buildRenderTranscriptRequestFromRows,
  type TranscriptRow,
} from "~/stt/render-transcript";

export type TranscriptRowWithId = {
  transcriptId: string;
  row: TranscriptRow;
};

export function useTranscriptRenderData(transcriptId: string): {
  request: RenderTranscriptRequest | null;
  transcriptRows: TranscriptRowWithId[];
} {
  const transcript = useTranscript(transcriptId);
  const transcripts = useMemo(
    () => (transcript ? [transcript] : emptyTranscripts),
    [transcript],
  );

  return useRenderData(transcripts);
}

export function useSessionTranscriptRenderData(sessionId: string): {
  request: RenderTranscriptRequest | null;
  transcriptRows: TranscriptRowWithId[];
} {
  const transcripts = useSessionTranscripts(sessionId);

  return useRenderData(transcripts);
}

function useRenderData(transcripts: readonly TranscriptRecord[]): {
  request: RenderTranscriptRequest | null;
  transcriptRows: TranscriptRowWithId[];
} {
  const selfHumanId = transcripts[0]?.ownerUserId;

  const transcriptRows = useMemo(() => {
    return transcripts.map((transcript) => ({
      transcriptId: transcript.id,
      row: {
        started_at: transcript.startedAt,
        words: transcript.words,
        speaker_hints: transcript.speakerHints,
      },
    }));
  }, [transcripts]);

  const request = useMemo(
    () =>
      buildRenderTranscriptRequestFromRows(
        transcriptRows.map((transcriptRow) => transcriptRow.row),
        { humans: [], selfHumanId },
      ),
    [selfHumanId, transcriptRows],
  );

  return { request, transcriptRows };
}

const emptyTranscripts: TranscriptRecord[] = [];
