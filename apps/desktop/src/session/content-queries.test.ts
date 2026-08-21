import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  sessionGet: vi.fn(
    (): Promise<
      | { status: "ok"; data: Record<string, unknown> | null }
      | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  sessionEnhancedDocs: vi.fn(
    (): Promise<
      | { status: "ok"; data: Array<Record<string, unknown>> }
      | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: [] }),
  ),
  sessionTranscripts: vi.fn(
    (): Promise<
      | { status: "ok"; data: Array<Record<string, unknown>> }
      | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: [] }),
  ),
  sessionIds: vi.fn(
    (): Promise<
      { status: "ok"; data: string[] } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: [] }),
  ),
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionGet: mocks.sessionGet,
    sessionEnhancedDocs: mocks.sessionEnhancedDocs,
    sessionTranscripts: mocks.sessionTranscripts,
    sessionIds: mocks.sessionIds,
  },
}));

import {
  loadActiveSessionIds,
  loadSessionContentSnapshot,
} from "./content-queries";

describe("session content snapshots", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.sessionGet.mockResolvedValue({ status: "ok", data: null });
    mocks.sessionEnhancedDocs.mockResolvedValue({ status: "ok", data: [] });
    mocks.sessionTranscripts.mockResolvedValue({ status: "ok", data: [] });
    mocks.sessionIds.mockResolvedValue({ status: "ok", data: [] });
  });

  it("maps one canonical session content snapshot from the store commands", async () => {
    mocks.sessionGet.mockResolvedValueOnce({
      status: "ok",
      data: {
        meta: {
          id: "session-1",
          title: "Planning",
          created_at: "2026-07-10T09:00:00.000Z",
          tags: [],
        },
        note_markdown: "Raw note",
      },
    });
    mocks.sessionEnhancedDocs.mockResolvedValueOnce({
      status: "ok",
      data: [
        {
          id: "summary-1",
          session_id: "session-1",
          kind: "template_output",
          title: "First",
          template_id: "template-1",
          sort_order: 1,
          markdown: "First summary",
        },
        {
          id: "summary-2",
          session_id: "session-1",
          kind: "template_output",
          title: "Second",
          template_id: "template-2",
          sort_order: 2,
          markdown: "Second summary",
        },
      ],
    });
    mocks.sessionTranscripts.mockResolvedValueOnce({
      status: "ok",
      data: [
        {
          id: "transcript-1",
          session_id: "session-1",
          started_at: 100,
          ended_at: 200,
          memo_md: "pre-meeting memo",
          words: [
            {
              id: "word-1",
              text: "Hello",
              start_ms: 0,
              end_ms: 100,
              channel: 0,
            },
          ],
          speaker_hints: [],
        },
      ],
    });

    const snapshot = await loadSessionContentSnapshot("session-1");

    expect(snapshot).toMatchObject({
      sessionId: "session-1",
      title: "Planning",
      createdAt: "2026-07-10T09:00:00.000Z",
      rawNoteId: "session-1:note",
      rawContentFormat: "md",
      rawMarkdown: "Raw note",
      enhancedNotes: [
        {
          id: "summary-1",
          markdown: "First summary",
          content: "First summary",
          contentFormat: "md",
          templateId: "template-1",
          position: 1,
        },
        { id: "summary-2", markdown: "Second summary", position: 2 },
      ],
      transcripts: [
        {
          id: "transcript-1",
          started_at: 100,
          ended_at: 200,
          memo: "pre-meeting memo",
          words: [expect.objectContaining({ id: "word-1", text: "Hello" })],
        },
      ],
    });
    expect(mocks.sessionGet).toHaveBeenCalledWith("session-1");
    expect(mocks.sessionEnhancedDocs).toHaveBeenCalledWith("session-1");
    expect(mocks.sessionTranscripts).toHaveBeenCalledWith("session-1");
  });

  it("returns null for an unknown session without loading docs or transcripts", async () => {
    await expect(loadSessionContentSnapshot("ghost")).resolves.toBeNull();
    expect(mocks.sessionEnhancedDocs).not.toHaveBeenCalled();
    expect(mocks.sessionTranscripts).not.toHaveBeenCalled();
  });

  it("marks a session without a note file as having no raw note", async () => {
    mocks.sessionGet.mockResolvedValueOnce({
      status: "ok",
      data: {
        meta: {
          id: "session-1",
          title: "",
          created_at: "2026-07-10T09:00:00.000Z",
          tags: [],
        },
        note_markdown: null,
      },
    });

    const snapshot = await loadSessionContentSnapshot("session-1");
    expect(snapshot).toMatchObject({
      rawNoteId: null,
      rawContent: "",
      rawMarkdown: "",
    });
  });

  it("lists active session ids via the store command", async () => {
    mocks.sessionIds.mockResolvedValueOnce({
      status: "ok",
      data: ["session-2", "session-1"],
    });

    await expect(loadActiveSessionIds()).resolves.toEqual([
      "session-2",
      "session-1",
    ]);
  });
});
