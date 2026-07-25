import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  transcript: {
    id: "transcript-1",
    ownerUserId: "user-1",
    sessionId: "session-1",
    startedAt: 1000,
    endedAt: 2000,
    words: [
      {
        id: "word-1",
        text: "Hello",
        start_ms: 0,
        end_ms: 500,
        channel: 0,
      },
    ],
    speakerHints: [],
  },
}));

vi.mock("~/stt/queries", () => ({
  useSessionTranscripts: () => [mocks.transcript],
  useTranscript: () => mocks.transcript,
}));

import {
  useSessionTranscriptRenderData,
  useTranscriptRenderData,
} from "./render-request-hooks";

describe("SQLite transcript render data", () => {
  beforeEach(() => {});

  it("builds a renderer request from one canonical transcript", () => {
    const { result } = renderHook(() =>
      useTranscriptRenderData("transcript-1"),
    );

    expect(result.current.transcriptRows).toEqual([
      {
        transcriptId: "transcript-1",
        row: {
          started_at: 1000,
          words: mocks.transcript.words,
          speaker_hints: [],
        },
      },
    ]);
    expect(result.current.request).toEqual(
      expect.objectContaining({
        self_human_id: "user-1",
        participant_human_ids: [],
        humans: [],
      }),
    );
    expect(result.current.request?.transcripts[0]?.words[0]?.id).toBe("word-1");
  });

  it("uses the same canonical rows for session-wide export rendering", () => {
    const { result } = renderHook(() =>
      useSessionTranscriptRenderData("session-1"),
    );

    expect(
      result.current.transcriptRows.map((row) => row.transcriptId),
    ).toEqual(["transcript-1"]);
    expect(result.current.request?.transcripts).toHaveLength(1);
  });
});
