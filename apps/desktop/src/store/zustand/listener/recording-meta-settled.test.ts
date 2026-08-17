import { afterEach, beforeEach, expect, it, vi } from "vitest";

const unlisten = vi.hoisted(() => vi.fn());
const listen = vi.hoisted(() => vi.fn());

vi.mock("~/types/tauri.gen", () => ({
  events: { recordingMetaSettled: { listen } },
}));

import { createRecordingMetaSettledWaiter } from "./recording-meta-settled";

type SettledHandler = (event: {
  payload: { sessionId: string; succeeded: boolean };
}) => void;

beforeEach(() => {
  vi.useFakeTimers();
  listen.mockReset();
  unlisten.mockReset();
  listen.mockImplementation(() => Promise.resolve(unlisten));
});

afterEach(() => {
  vi.useRealTimers();
});

function installedHandler(): SettledHandler {
  const calls = listen.mock.calls as unknown[][];
  return calls[calls.length - 1][0] as SettledHandler;
}

it("installs the listener immediately, before any stop request runs", () => {
  createRecordingMetaSettledWaiter("s1");
  expect(listen).toHaveBeenCalledTimes(1);
});

it("resolves when the matching session settles", async () => {
  const waiter = createRecordingMetaSettledWaiter("s1");
  const pending = waiter.wait(5_000);
  installedHandler()({ payload: { sessionId: "s1", succeeded: true } });

  await expect(pending).resolves.toBe(true);
});

it("ignores settle events for unrelated sessions", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const waiter = createRecordingMetaSettledWaiter("s1");
  const pending = waiter.wait(5_000);
  installedHandler()({ payload: { sessionId: "other", succeeded: true } });

  await vi.advanceTimersByTimeAsync(5_000);
  await expect(pending).resolves.toBe(false);
  warn.mockRestore();
});

it("times out with a warning instead of wedging stop handling", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const waiter = createRecordingMetaSettledWaiter("s1");
  const pending = waiter.wait(1_000);

  await vi.advanceTimersByTimeAsync(1_000);
  await expect(pending).resolves.toBe(false);
  expect(warn).toHaveBeenCalledOnce();
  warn.mockRestore();
});

it("a failed settle event still resolves the waiter", async () => {
  const waiter = createRecordingMetaSettledWaiter("s1");
  const pending = waiter.wait(5_000);
  installedHandler()({ payload: { sessionId: "s1", succeeded: false } });

  await expect(pending).resolves.toBe(true);
});
