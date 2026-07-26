import { beforeEach, describe, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  cleanupDeletedSessionAudio: vi.fn(),
  deleteLocalSessionAudio: vi.fn(),
  sessionList: vi.fn(),
  sessionHasTranscript: vi.fn(),
  getSessionMode: vi.fn(),
  live: { loading: false, sessionId: null as string | null },
}));

vi.mock("~/session/attachments", () => ({
  cleanupDeletedSessionAudio: mocks.cleanupDeletedSessionAudio,
  deleteLocalSessionAudio: mocks.deleteLocalSessionAudio,
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionList: mocks.sessionList,
    sessionHasTranscript: mocks.sessionHasTranscript,
  },
}));

vi.mock("~/store/zustand/listener/instance", () => ({
  listenerStore: {
    getState: () => ({
      getSessionMode: mocks.getSessionMode,
      live: mocks.live,
    }),
  },
}));

import {
  cleanupExpiredAudio,
  deleteProcessedAudioForRetention,
  normalizeAudioRetention,
  sessionAudioExpired,
} from "./audio-retention";

function mockCleanupRows(
  sessions: Array<{ id: string; created_at: string; has_words: number }>,
) {
  mocks.sessionList.mockResolvedValue({
    status: "ok",
    data: sessions.map((session) => ({
      meta: { id: session.id, created_at: session.created_at },
      has_transcript_words: session.has_words === 1,
    })),
  });
}

describe("audio retention", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.cleanupDeletedSessionAudio.mockResolvedValue(true);
    mocks.deleteLocalSessionAudio.mockResolvedValue(true);
    mocks.getSessionMode.mockReturnValue("inactive");
    mocks.sessionHasTranscript.mockResolvedValue({ status: "ok", data: false });
    mocks.live.loading = false;
    mocks.live.sessionId = null;
    mockCleanupRows([]);
  });

  test("normalizes current and legacy values", () => {
    expect(normalizeAudioRetention("none")).toBe("none");
    expect(normalizeAudioRetention("oneWeek")).toBe("oneWeek");
    expect(normalizeAudioRetention("forever")).toBe("forever");
    expect(normalizeAudioRetention(false)).toBe("none");
    expect(normalizeAudioRetention(true)).toBe("forever");
    expect(normalizeAudioRetention("invalid")).toBe("forever");
    expect(normalizeAudioRetention("invalid", undefined)).toBeUndefined();
  });

  test("applies each retention window", () => {
    const now = Date.parse("2026-05-13T00:00:00.000Z");

    expect(sessionAudioExpired("not-a-date", "none", now)).toBe(true);
    expect(
      sessionAudioExpired("2026-01-01T00:00:00.000Z", "forever", now),
    ).toBe(false);
    expect(sessionAudioExpired("2026-05-11T23:59:59.999Z", "oneDay", now)).toBe(
      true,
    );
    expect(sessionAudioExpired("2026-05-12T00:00:00.001Z", "oneDay", now)).toBe(
      false,
    );
    expect(sessionAudioExpired("not-a-date", "oneDay", now)).toBe(false);
  });

  test("deletes only expired inactive SQLite sessions", async () => {
    mockCleanupRows([
      {
        id: "expired",
        created_at: "2026-05-11T23:59:59.999Z",
        has_words: 1,
      },
      {
        id: "fresh",
        created_at: "2026-05-12T00:00:00.001Z",
        has_words: 1,
      },
      {
        id: "active",
        created_at: "2026-05-11T23:59:59.999Z",
        has_words: 1,
      },
    ]);
    mocks.getSessionMode.mockImplementation((sessionId) =>
      sessionId === "active" ? "active" : "inactive",
    );

    const deleted = await cleanupExpiredAudio(
      "oneDay",
      Date.parse("2026-05-13T00:00:00.000Z"),
    );

    expect(mocks.deleteLocalSessionAudio).toHaveBeenCalledTimes(1);
    expect(mocks.deleteLocalSessionAudio).toHaveBeenCalledWith(
      "expired",
      expect.any(Function),
    );
    expect(deleted).toEqual(["expired"]);
  });

  test("retention none keeps audio until transcript words exist", async () => {
    mockCleanupRows([
      {
        id: "unprocessed",
        created_at: "2026-05-13T00:00:00.000Z",
        has_words: 0,
      },
      {
        id: "processed",
        created_at: "2026-05-13T00:00:00.000Z",
        has_words: 1,
      },
    ]);

    await expect(
      cleanupExpiredAudio("none", Date.parse("2026-05-13T00:00:00.000Z")),
    ).resolves.toEqual(["processed"]);
    expect(mocks.deleteLocalSessionAudio).toHaveBeenCalledWith(
      "processed",
      expect.any(Function),
    );
  });

  test("deletes processed audio immediately when retention is none", async () => {
    mocks.sessionHasTranscript.mockResolvedValueOnce({
      status: "ok",
      data: true,
    });

    await expect(
      deleteProcessedAudioForRetention("none", "processed"),
    ).resolves.toBe(true);
    expect(mocks.deleteLocalSessionAudio).toHaveBeenCalledWith(
      "processed",
      expect.any(Function),
    );
  });

  test("keeps unprocessed audio when retention is none", async () => {
    mocks.sessionHasTranscript.mockResolvedValueOnce({
      status: "ok",
      data: false,
    });

    await expect(
      deleteProcessedAudioForRetention("none", "unprocessed"),
    ).resolves.toBe(false);
    expect(mocks.deleteLocalSessionAudio).not.toHaveBeenCalled();
  });

  test("skips immediate deletion for retained audio", async () => {
    await expect(
      deleteProcessedAudioForRetention("oneDay", "processed"),
    ).resolves.toBe(false);
    expect(mocks.sessionHasTranscript).not.toHaveBeenCalled();
    expect(mocks.deleteLocalSessionAudio).not.toHaveBeenCalled();
  });

  // Logically-deleted-audio retry (session_attachments/attachment_local_state)
  // is a graceful no-op pending Task 9 (Session store scaffold) — see
  // cleanupLogicallyDeletedAudio in audio-retention.ts.
  test("does not query or delete anything when retention is forever", async () => {
    await expect(cleanupExpiredAudio("forever")).resolves.toEqual([]);
    expect(mocks.sessionList).not.toHaveBeenCalled();
    expect(mocks.deleteLocalSessionAudio).not.toHaveBeenCalled();
  });

  test("does not report failed audio deletions as deleted", async () => {
    mockCleanupRows([
      {
        id: "expired",
        created_at: "2026-05-01T00:00:00.000Z",
        has_words: 1,
      },
    ]);
    mocks.deleteLocalSessionAudio.mockRejectedValueOnce(
      new Error("disk failure"),
    );
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});

    await expect(
      cleanupExpiredAudio("oneDay", Date.parse("2026-05-13T00:00:00.000Z")),
    ).resolves.toEqual([]);
    expect(consoleError).toHaveBeenCalled();
    consoleError.mockRestore();
  });

  test("does not report a device without local audio as deleted", async () => {
    mockCleanupRows([
      {
        id: "remote-only",
        created_at: "2026-05-01T00:00:00.000Z",
        has_words: 1,
      },
    ]);
    mocks.deleteLocalSessionAudio.mockResolvedValueOnce(false);

    await expect(
      cleanupExpiredAudio("oneDay", Date.parse("2026-05-13T00:00:00.000Z")),
    ).resolves.toEqual([]);
  });
});
