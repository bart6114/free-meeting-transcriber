import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createSession: vi.fn(),
  sessionFindByTrackingId: vi.fn(
    (): Promise<
      | { status: "ok"; data: { id: string } | null }
      | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
}));

vi.mock("~/session/queries", () => ({
  createSession: mocks.createSession,
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionFindByTrackingId: mocks.sessionFindByTrackingId,
  },
}));

import {
  getOrCreateWelcomeSession,
  setPendingWelcomeSession,
  takePendingWelcomeSession,
} from "./welcome-note";

beforeEach(() => {
  vi.clearAllMocks();
  mocks.sessionFindByTrackingId.mockResolvedValue({ status: "ok", data: null });
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    removeItem: (key: string) => values.delete(key),
    setItem: (key: string, value: string) => values.set(key, value),
  });
});

it("reuses an existing onboarding welcome note", async () => {
  mocks.sessionFindByTrackingId.mockResolvedValueOnce({
    status: "ok",
    data: { id: "welcome-session" },
  });

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
  expect(mocks.createSession).not.toHaveBeenCalled();
  expect(mocks.sessionFindByTrackingId).toHaveBeenCalledWith(
    "fmtr-onboarding-demo-v1",
  );
});

it("creates a welcome note carrying the tracking marker", async () => {
  mocks.createSession.mockResolvedValueOnce("welcome-session");

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");

  const [title, , initial] = mocks.createSession.mock.calls[0];
  expect(title).toBe("Welcome to Loofah");
  expect(initial.tracking_id).toBe("fmtr-onboarding-demo-v1");
  expect(initial.raw_md).toContain(
    "transcribe the conversation on your machine",
  );
  expect(initial.raw_md).toContain("Record");

  const note = JSON.parse(initial.raw_md);
  expect(note.content).toHaveLength(7);
  expect(note.content[1]).toEqual({ type: "paragraph" });
  expect(note.content[3]).toEqual({ type: "paragraph" });
  expect(note.content[5]).toEqual({ type: "paragraph" });
});

it("does not create a session when the tracking-id lookup fails", async () => {
  mocks.sessionFindByTrackingId.mockResolvedValueOnce({
    status: "error",
    error: "index unavailable",
  });

  await expect(getOrCreateWelcomeSession()).rejects.toThrow(
    "index unavailable",
  );
  expect(mocks.createSession).not.toHaveBeenCalled();
});

it("carries the welcome note across a one-time onboarding relaunch", () => {
  setPendingWelcomeSession("welcome-session");

  expect(takePendingWelcomeSession()).toBe("welcome-session");
  expect(takePendingWelcomeSession()).toBeNull();
});
