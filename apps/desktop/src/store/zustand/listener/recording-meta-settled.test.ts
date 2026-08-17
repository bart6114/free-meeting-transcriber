import { afterEach, beforeEach, expect, it, vi } from "vitest";

const listen = vi.hoisted(() => vi.fn());

vi.mock("~/types/tauri.gen", () => ({
  events: { recordingMetaSettled: { listen } },
}));

import {
  ensureRecordingMetaSettledListener,
  waitForRecordingMetaSettled,
} from "./recording-meta-settled";

type SettledHandler = (event: {
  payload: { sessionId: string; succeeded: boolean };
}) => void;

beforeEach(() => {
  vi.useFakeTimers();
  listen.mockImplementation(() => Promise.resolve(() => {}));
});

afterEach(() => {
  vi.useRealTimers();
});

function installedHandler(): SettledHandler {
  const calls = listen.mock.calls as unknown[][];
  return calls[calls.length - 1][0] as SettledHandler;
}

it("registers the persistent listener once, before any stop request runs", () => {
  const before = listen.mock.calls.length;
  ensureRecordingMetaSettledListener();
  ensureRecordingMetaSettledListener();
  expect(listen.mock.calls.length).toBeLessThanOrEqual(before + 1);
});

it("resolves when the matching session settles", async () => {
  const pending = waitForRecordingMetaSettled("s1", 5_000);
  installedHandler()({ payload: { sessionId: "s1", succeeded: true } });

  await expect(pending).resolves.toBe(true);
});

it("ignores settle events for unrelated sessions", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const pending = waitForRecordingMetaSettled("s1", 5_000);
  installedHandler()({ payload: { sessionId: "other", succeeded: true } });

  await vi.advanceTimersByTimeAsync(5_000);
  await expect(pending).resolves.toBe(false);
  warn.mockRestore();
});

it("times out with a warning instead of wedging stop handling", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const pending = waitForRecordingMetaSettled("s1", 1_000);

  await vi.advanceTimersByTimeAsync(1_000);
  await expect(pending).resolves.toBe(false);
  expect(warn).toHaveBeenCalledOnce();
  warn.mockRestore();
});

it("a failed settle event still resolves the waiter", async () => {
  const pending = waitForRecordingMetaSettled("s1", 5_000);
  installedHandler()({ payload: { sessionId: "s1", succeeded: false } });

  await expect(pending).resolves.toBe(true);
});

it("multiple waiters for the same session all settle on one event", async () => {
  const a = waitForRecordingMetaSettled("s1", 5_000);
  const b = waitForRecordingMetaSettled("s1", 5_000);
  installedHandler()({ payload: { sessionId: "s1", succeeded: true } });

  await expect(a).resolves.toBe(true);
  await expect(b).resolves.toBe(true);
});
