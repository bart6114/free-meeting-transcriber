import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createSession: vi.fn(),
  execute: vi.fn(),
  sessionUpdateMeta: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
}));

vi.mock("~/db", () => ({
  liveQueryClient: { execute: mocks.execute },
}));

vi.mock("~/session/queries", () => ({
  createSession: mocks.createSession,
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
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
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    removeItem: (key: string) => values.delete(key),
    setItem: (key: string, value: string) => values.set(key, value),
  });
});

it("reuses an existing onboarding welcome note and clears a stale meeting link through the store", async () => {
  mocks.execute.mockResolvedValueOnce([
    {
      id: "welcome-session",
      event_json: JSON.stringify({
        tracking_id: "fmtr-onboarding-demo-v1",
        meeting_link: "https://stale.example.com/meet",
      }),
    },
  ]);

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
  expect(mocks.createSession).not.toHaveBeenCalled();
  expect(mocks.execute).toHaveBeenCalledWith(expect.any(String), [
    "fmtr-onboarding-demo-v1",
  ]);
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
  mocks.execute.mockResolvedValueOnce([
    {
      id: "welcome-session",
      event_json: JSON.stringify({
        tracking_id: "fmtr-onboarding-demo-v1",
        meeting_link: "",
      }),
    },
  ]);

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
  expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
});

it("still returns the session when the stale-link clear fails", async () => {
  mocks.execute.mockResolvedValueOnce([
    {
      id: "welcome-session",
      event_json: JSON.stringify({
        tracking_id: "fmtr-onboarding-demo-v1",
        meeting_link: "https://stale.example.com/meet",
      }),
    },
  ]);
  mocks.sessionUpdateMeta.mockResolvedValueOnce({
    status: "error",
    error: "no _meta.json",
  });

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
});

it("creates a welcome note without a meeting link", async () => {
  mocks.execute.mockResolvedValueOnce([]);
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

it("guards empty event metadata before reading its tracking ID", async () => {
  mocks.execute.mockResolvedValueOnce([]);
  mocks.createSession.mockResolvedValueOnce("welcome-session");

  await getOrCreateWelcomeSession();

  const [query] = mocks.execute.mock.calls[0];
  expect(query).toMatch(
    /CASE\s+WHEN json_valid\(event_json\)\s+THEN json_extract\(event_json, '\$\.tracking_id'\)\s+END = \?/,
  );
});

it("carries the welcome note across a one-time onboarding relaunch", () => {
  setPendingWelcomeSession("welcome-session");

  expect(takePendingWelcomeSession()).toBe("welcome-session");
  expect(takePendingWelcomeSession()).toBeNull();
});
