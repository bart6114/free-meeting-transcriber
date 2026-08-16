import { beforeEach, describe, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  sessionDir: vi.fn(),
}));

vi.mock("@tauri-apps/api/path", () => ({
  sep: vi.fn().mockReturnValue("/"),
}));

vi.mock("@hypr/plugin-fs-sync", () => ({
  commands: {
    sessionDir: mocks.sessionDir,
  },
}));

import { getSessionResourcePath } from "./resource-path";

describe("getSessionResourcePath", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test("returns the backend-resolved session directory", async () => {
    mocks.sessionDir.mockResolvedValue({
      status: "ok",
      data: "/data/fmtr/sessions/2026-03-20 — Planning — 6ba7b8",
    });

    await expect(
      getSessionResourcePath("/data/fmtr", "session-123"),
    ).resolves.toBe("/data/fmtr/sessions/2026-03-20 — Planning — 6ba7b8");
    expect(mocks.sessionDir).toHaveBeenCalledWith("session-123");
  });

  test("falls back to the legacy layout when the command errors", async () => {
    mocks.sessionDir.mockResolvedValue({
      status: "error",
      error: "not found",
    });

    await expect(
      getSessionResourcePath("/data/fmtr", "session-123"),
    ).resolves.toBe("/data/fmtr/sessions/session-123");
  });

  test("falls back to the legacy layout when the command rejects", async () => {
    mocks.sessionDir.mockRejectedValue(new Error("ipc failure"));

    await expect(
      getSessionResourcePath("/data/fmtr", "session-123"),
    ).resolves.toBe("/data/fmtr/sessions/session-123");
  });
});
