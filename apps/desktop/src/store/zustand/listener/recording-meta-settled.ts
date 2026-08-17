import { events } from "~/types/tauri.gen";

export const RECORDING_META_SETTLED_TIMEOUT_MS = 30_000;

/**
 * One-shot waiter for the backend's `recording-meta-settled` event: the signal
 * that the end-of-recording meta stamp -- and any provisional directory rename
 * it triggered -- is no longer in flight. Install BEFORE requesting stop, so an
 * instant finalization cannot slip past the listener; then `wait()` before
 * resolving `resource_dir` for the post-stop hook.
 *
 * The timeout is a crash/fault safeguard, not ordering logic: a normal stop
 * always completes by event, and on timeout the caller logs and resolves the
 * directory afresh rather than wedging stop handling forever.
 */
export function createRecordingMetaSettledWaiter(sessionId: string) {
  let resolveSettled: ((arrived: boolean) => void) | undefined;
  const settled = new Promise<boolean>((resolve) => {
    resolveSettled = resolve;
  });
  const unlistenPromise = events.recordingMetaSettled
    .listen(({ payload }) => {
      if (payload.sessionId === sessionId) {
        resolveSettled?.(true);
      }
    })
    .catch((error: unknown) => {
      console.error("[recording-meta-settled] failed to listen", error);
      resolveSettled?.(false);
      return undefined;
    });

  const dispose = () => {
    void unlistenPromise.then((unlisten) => unlisten?.());
  };

  return {
    wait: async (
      timeoutMs: number = RECORDING_META_SETTLED_TIMEOUT_MS,
    ): Promise<boolean> => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const timeout = new Promise<boolean>((resolve) => {
        timer = setTimeout(() => resolve(false), timeoutMs);
      });
      const arrived = await Promise.race([settled, timeout]);
      clearTimeout(timer);
      dispose();
      if (!arrived) {
        console.warn(
          `[recording-meta-settled] no settle event within ${timeoutMs}ms for session ${sessionId}; resolving the directory afresh`,
        );
      }
      return arrived;
    },
    dispose,
  };
}
