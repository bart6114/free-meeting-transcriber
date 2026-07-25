import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { TranscriptWithData } from "~/types/tauri.gen";

const mocks = vi.hoisted(() => ({
  execute: vi.fn(),
  executeTransaction: vi.fn(
    (_statements: Array<{ sql: string; params: unknown[] }>) =>
      Promise.resolve([1]),
  ),
  sessionWriteTranscript: vi.fn(
    (
      _sessionId: string,
      _transcript: TranscriptWithData,
    ): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  queryOptions: [] as Array<{
    sql: string;
    params?: unknown[];
    enabled?: boolean;
  }>,
  transcriptRows: [] as Array<Record<string, unknown>>,
}));

vi.mock("~/db", () => ({
  executeTransaction: mocks.executeTransaction,
  liveQueryClient: { execute: mocks.execute },
  useLiveQuery: (options: {
    sql: string;
    params?: unknown[];
    enabled?: boolean;
    mapRows?: (rows: Array<Record<string, unknown>>) => unknown;
  }) => {
    mocks.queryOptions.push(options);

    return {
      data:
        options.enabled === false
          ? undefined
          : options.mapRows
            ? options.mapRows(mocks.transcriptRows)
            : mocks.transcriptRows,
    };
  },
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionWriteTranscript: mocks.sessionWriteTranscript,
  },
}));

import {
  appendTranscriptWordsAndHints,
  assignTranscriptSpeaker,
  createTranscript,
  softDeleteTranscript,
  useSessionTranscripts,
  useTranscript,
  useTranscriptLabelContext,
} from "./queries";

describe("transcript queries", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.queryOptions = [];
    mocks.transcriptRows = [];
    mocks.sessionWriteTranscript.mockResolvedValue({
      status: "ok",
      data: null,
    });
  });

  it("maps canonical transcript JSON into renderer records", () => {
    mocks.transcriptRows = [
      {
        id: "transcript-1",
        owner_user_id: "user-1",
        session_id: "session-1",
        started_at_ms: 1000,
        ended_at_ms: 2000,
        words_json: JSON.stringify([
          {
            id: "word-1",
            text: "Hello",
            start_ms: 0,
            end_ms: 500,
            channel: 0,
          },
        ]),
        speaker_hints_json: JSON.stringify([
          { word_id: "word-1", type: "provider_speaker_index", value: 0 },
        ]),
      },
    ];

    const { result } = renderHook(() => useSessionTranscripts("session-1"));

    expect(result.current).toEqual([
      expect.objectContaining({
        id: "transcript-1",
        ownerUserId: "user-1",
        sessionId: "session-1",
        startedAt: 1000,
        endedAt: 2000,
        words: [expect.objectContaining({ id: "word-1" })],
        speakerHints: [expect.objectContaining({ word_id: "word-1" })],
      }),
    ]);
    expect(mocks.queryOptions[0]?.sql).toContain("ORDER BY started_at_ms, id");
  });

  it("treats non-array transcript payloads as empty without hiding the row", () => {
    mocks.transcriptRows = [
      {
        id: "transcript-1",
        owner_user_id: "user-1",
        session_id: "session-1",
        started_at_ms: 1000,
        ended_at_ms: null,
        words_json: "{}",
        speaker_hints_json: "null",
      },
    ];

    const { result } = renderHook(() => useTranscript("transcript-1"));

    expect(result.current).toEqual(
      expect.objectContaining({
        id: "transcript-1",
        endedAt: undefined,
        words: [],
        speakerHints: [],
      }),
    );
  });

  it("resolves speaker labels straight from assigned hint values, not a lookup", () => {
    mocks.transcriptRows = [
      {
        id: "transcript-1",
        owner_user_id: "self",
        session_id: "session-1",
        started_at_ms: 1000,
        ended_at_ms: null,
        words_json: "[]",
        speaker_hints_json: JSON.stringify([
          {
            id: "hint-1",
            word_id: "word-1",
            type: "speaker_label",
            value: "Alice",
          },
        ]),
      },
    ];

    const { result } = renderHook(() =>
      useTranscriptLabelContext("transcript-1"),
    );

    expect(result.current?.getSelfHumanId()).toBe("self");
    expect(result.current?.getHumanName("Alice")).toBe("Alice");
    expect(result.current?.getHumanName("self")).toBeUndefined();
    expect(result.current?.getParticipantHumanIds?.()).toEqual([]);
  });

  it("writes a new transcript through the store", async () => {
    await createTranscript({
      id: "transcript-1",
      sessionId: "session-1",
      ownerUserId: "user-1",
      createdAt: "2026-07-10T12:00:00.000Z",
      startedAt: 1000,
      words: [
        { id: "word-1", text: "Hello", start_ms: 0, end_ms: 500, channel: 0 },
      ],
      speakerHints: [],
    });

    expect(mocks.sessionWriteTranscript).toHaveBeenCalledWith(
      "session-1",
      expect.objectContaining({
        id: "transcript-1",
        session_id: "session-1",
        started_at: 1000,
        words: [expect.objectContaining({ id: "word-1", text: "Hello" })],
      }),
    );
  });

  it("tombstones old session transcripts via the index before writing the replacement", async () => {
    await createTranscript({
      id: "transcript-new",
      sessionId: "session-1",
      ownerUserId: "user-1",
      createdAt: "2026-07-10T12:00:00.000Z",
      startedAt: 1000,
      replaceSession: true,
    });

    const statements = mocks.executeTransaction.mock.calls[0]?.[0] as Array<{
      sql: string;
      params: unknown[];
    }>;
    expect(statements).toHaveLength(1);
    expect(statements[0]?.sql).toContain("UPDATE transcripts");
    expect(statements[0]?.sql).toContain("deleted_at IS NULL");
    expect(mocks.sessionWriteTranscript).toHaveBeenCalledWith(
      "session-1",
      expect.objectContaining({ id: "transcript-new" }),
    );
  });

  it("appends words onto the current transcript and writes the merged result", async () => {
    mocks.execute.mockResolvedValueOnce([
      {
        session_id: "session-1",
        started_at_ms: 1000,
        words_json: JSON.stringify([
          { id: "word-1", text: "Hello", start_ms: 0, end_ms: 500, channel: 0 },
        ]),
        speaker_hints_json: "[]",
      },
    ]);

    await appendTranscriptWordsAndHints(
      "transcript-1",
      [{ id: "word-2", text: "world", start_ms: 500, end_ms: 900, channel: 0 }],
      [],
    );

    const call = mocks.sessionWriteTranscript.mock.calls[0];
    expect(call?.[0]).toBe("session-1");
    expect(call?.[1]?.id).toBe("transcript-1");
    expect(call?.[1]?.started_at).toBe(1000);
    expect(call?.[1]?.words).toEqual([
      expect.objectContaining({ id: "word-1", text: "Hello" }),
      expect.objectContaining({ id: "word-2", text: "world" }),
    ]);
  });

  it("refuses to overwrite malformed transcript JSON", async () => {
    mocks.execute.mockResolvedValueOnce([
      {
        session_id: "session-1",
        started_at_ms: 1000,
        words_json: "not-json",
        speaker_hints_json: "[]",
      },
    ]);

    await expect(
      appendTranscriptWordsAndHints("transcript-1", [], []),
    ).rejects.toThrow("invalid words data");
    expect(mocks.sessionWriteTranscript).not.toHaveBeenCalled();
  });

  it("persists a plain-string speaker label through the store", async () => {
    mocks.execute.mockResolvedValueOnce([
      {
        session_id: "session-1",
        started_at_ms: 1000,
        words_json: JSON.stringify([
          { id: "word-1", text: "Hello", start_ms: 0, end_ms: 500, channel: 1 },
        ]),
        speaker_hints_json: "[]",
      },
    ]);

    await assignTranscriptSpeaker({
      transcriptId: "transcript-1",
      segmentKey: {
        channel: "RemoteParty",
        speaker_index: 0,
        speaker_human_id: null,
      },
      speakerLabel: "Alice",
      anchorWordId: "word-1",
    });

    const call = mocks.sessionWriteTranscript.mock.calls[0];
    expect(call?.[1]?.speaker_hints).toEqual([
      expect.objectContaining({
        word_id: "word-1",
        type: "speaker_label",
        value: "Alice",
      }),
    ]);
  });

  it("zeroes a transcript's content instead of deleting the row", async () => {
    await softDeleteTranscript("session-1", "transcript-1");

    expect(mocks.sessionWriteTranscript).toHaveBeenCalledWith("session-1", {
      id: "transcript-1",
      session_id: "session-1",
      words: [],
      speaker_hints: [],
    });
  });
});
