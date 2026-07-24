import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createSession: vi.fn(),
  execute: vi.fn(),
}));

vi.mock("~/db", () => ({
  liveQueryClient: { execute: mocks.execute },
}));

vi.mock("~/session/queries", () => ({
  createSession: mocks.createSession,
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

it("reuses an existing onboarding welcome note and clears a stale meeting link", async () => {
  mocks.execute.mockResolvedValueOnce([{ id: "welcome-session" }]);

  await expect(getOrCreateWelcomeSession()).resolves.toBe("welcome-session");
  expect(mocks.createSession).not.toHaveBeenCalled();
  expect(mocks.execute).toHaveBeenCalledWith(expect.any(String), [
    "fmtr-onboarding-demo-v1",
  ]);
  expect(mocks.execute).toHaveBeenCalledWith(
    expect.stringContaining("json_set(event_json, '$.meeting_link', '')"),
    ["welcome-session"],
  );
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
