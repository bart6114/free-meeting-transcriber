import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useTranscriptRenderData } from "./render-request-hooks";

const mocks = vi.hoisted(() => ({
  useTranscript: vi.fn(),
  usePeople: vi.fn(() => [{ id: "bob_peters", name: "Bob Peters" }]),
}));

vi.mock("~/stt/queries", () => ({
  useTranscript: mocks.useTranscript,
  useSessionTranscripts: vi.fn(() => []),
}));

vi.mock("~/people/queries", () => ({
  usePeople: mocks.usePeople,
}));

describe("useTranscriptRenderData", () => {
  it("passes the people registry into the render request as humans", () => {
    mocks.useTranscript.mockReturnValue({
      id: "transcript-1",
      ownerUserId: "self",
      sessionId: "session-1",
      startedAt: 1000,
      words: [
        { id: "word-1", text: "hello", start_ms: 0, end_ms: 100, channel: 0 },
      ],
      speakerHints: [],
    });

    const { result } = renderHook(() =>
      useTranscriptRenderData("transcript-1"),
    );

    expect(result.current.request?.humans).toEqual([
      { human_id: "bob_peters", name: "Bob Peters" },
    ]);
    expect(result.current.request?.self_human_id).toBe("self");
  });
});
