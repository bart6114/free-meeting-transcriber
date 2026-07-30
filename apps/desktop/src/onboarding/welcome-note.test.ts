import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createSession: vi.fn(),
  sessionFindByTrackingId: vi.fn(
    (): Promise<
      | { status: "ok"; data: { id: string; event?: unknown } | null }
      | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  sessionUpdateMeta: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
}));

vi.mock("~/session/queries", () => ({
  createSession: mocks.createSession,
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionFindByTrackingId: mocks.sessionFindByTrackingId,
    sessionUpdateMeta: mocks.sessionUpdateMeta,
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
  mocks.sessionUpdateMeta.mockResolvedValue({ status: "ok", data: null });
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    removeItem: (key: string) => values.delete(key),
    setItem: (key: string, value: string) => values.set(key, value),
  });
});

it("reuses an existing onboarding welcome note and clears a stale meeting link through the store", async () => {
  mocks.sessionFindByTrackingId.mockResolvedValueOnce({
    status: "ok",
    data: {
      id: "welcome-session",
      event: {
        tracking_id: "fmtr-onboarding-demo-v1",
        meeting_link: "https://stale.example.com/meet",
      },
    },
  });

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
  expect(mocks.createSession).not.toHaveBeenCalled();
  expect(mocks.sessionFindByTrackingId).toHaveBeenCalledWith(
    "fmtr-onboarding-demo-v1",
  );
  // The event envelope is `_meta.json`-canonical now: the clear is a read-modify-write
  // through the store command, never a raw SQL json_set.
  expect(mocks.sessionUpdateMeta).toHaveBeenCalledWith("welcome-session", {
    event: {
      tracking_id: "fmtr-onboarding-demo-v1",
      meeting_link: "",
    },
  });
});

it("leaves the event untouched when the meeting link is already empty", async () => {
  mocks.sessionFindByTrackingId.mockResolvedValueOnce({
    status: "ok",
    data: {
      id: "welcome-session",
      event: {
        tracking_id: "fmtr-onboarding-demo-v1",
        meeting_link: "",
      },
    },
  });

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
  expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
});

it("still returns the session when the stale-link clear fails", async () => {
  mocks.sessionFindByTrackingId.mockResolvedValueOnce({
    status: "ok",
    data: {
      id: "welcome-session",
      event: {
        tracking_id: "fmtr-onboarding-demo-v1",
        meeting_link: "https://stale.example.com/meet",
      },
    },
  });
  mocks.sessionUpdateMeta.mockResolvedValueOnce({
    status: "error",
    error: "no _meta.json",
  });

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
});

it("creates a welcome note without a meeting link", async () => {
  mocks.createSession.mockResolvedValueOnce("welcome-session");

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");

  const [title, , initial] = mocks.createSession.mock.calls[0];
  const event = JSON.parse(initial.event_json);
  expect(title).toBe("Welcome to Free Meeting Transcriber");
  expect(event.meeting_link).toBe("");
  expect(event.tracking_id).toBe("fmtr-onboarding-demo-v1");
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
