import { events } from "~/types/tauri.gen";

export const RECORDING_META_SETTLED_TIMEOUT_MS = 30_000;

type Waiter = { sessionId: string; settle: () => void };

const waiters = new Set<Waiter>();
let listenerStarted = false;

/**
 * Starts the module's persistent `recording-meta-settled` listener (one per
 * webview, module lifetime). Call it when capture listeners are installed --
 * before any stop can be requested -- so the registration round-trip is done
 * before the first settle event could ever be emitted.
 */
export function ensureRecordingMetaSettledListener() {
  if (listenerStarted) {
    return;
  }
  listenerStarted = true;
  events.recordingMetaSettled
    .listen(({ payload }) => {
      for (const waiter of [...waiters]) {
        if (waiter.sessionId === payload.sessionId) {
          waiters.delete(waiter);
          waiter.settle();
        }
      }
    })
    .catch((error: unknown) => {
      listenerStarted = false;
      console.error("[recording-meta-settled] failed to listen", error);
    });
}

/**
 * Resolves `true` when the next `recording-meta-settled` event for the session
 * arrives -- the signal that the end-of-recording meta stamp, and any provisional
 * directory rename it triggered, is no longer in flight. Callers wait on it
 * before resolving any absolute session path after a stop.
 *
 * The timeout is a crash/fault safeguard, not ordering logic: a normal stop
 * always completes by event, and on timeout the caller logs and resolves the
 * path afresh rather than wedging stop handling forever.
 */
export function waitForRecordingMetaSettled(
  sessionId: string,
  timeoutMs: number = RECORDING_META_SETTLED_TIMEOUT_MS,
): Promise<boolean> {
  ensureRecordingMetaSettledListener();
  return new Promise((resolve) => {
    const waiter: Waiter = {
      sessionId,
      settle: () => {
        clearTimeout(timer);
        resolve(true);
      },
    };
    const timer = setTimeout(() => {
      waiters.delete(waiter);
      console.warn(
        `[recording-meta-settled] no settle event within ${timeoutMs}ms for session ${sessionId}; resolving paths afresh`,
      );
      resolve(false);
    }, timeoutMs);
    waiters.add(waiter);
  });
}
