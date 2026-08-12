import { describe, expect, it, vi } from "vitest";

import { subscribeIndexChanged } from "./index-query";

const mocks = vi.hoisted(() => ({
  listeners: [] as Array<(event: { payload: unknown }) => void>,
}));

vi.mock("~/types/tauri.gen", () => ({
  events: {
    indexChanged: {
      listen: (callback: (event: { payload: unknown }) => void) => {
        mocks.listeners.push(callback);
        return Promise.resolve(() => {});
      },
    },
  },
}));

function emit(entity: string, ids: string[]) {
  for (const listener of mocks.listeners) {
    listener({ payload: { entity, ids } });
  }
}

describe("subscribeIndexChanged", () => {
  it("fires duplicate-key subscriptions once per event, distinct keys separately", async () => {
    // A large transcript mounts hundreds of identical useTranscript/usePeople
    // subscriptions (one per segment). Without dedupe, each event triggered as
    // many invalidateQueries calls, each canceling and restarting the same
    // refetch -- which could strand the query in a paused state with stale data.
    const sharedA = vi.fn();
    const sharedB = vi.fn();
    const distinct = vi.fn();
    const keyless = vi.fn();

    const unsubscribes = [
      subscribeIndexChanged("transcripts", sharedA, undefined, "shared-key"),
      subscribeIndexChanged("transcripts", sharedB, undefined, "shared-key"),
      subscribeIndexChanged("transcripts", distinct, undefined, "other-key"),
      subscribeIndexChanged("transcripts", keyless),
    ];
    await Promise.resolve();

    emit("transcripts", ["s1"]);

    expect(sharedA).toHaveBeenCalledTimes(1);
    expect(sharedB).not.toHaveBeenCalled();
    expect(distinct).toHaveBeenCalledTimes(1);
    expect(keyless).toHaveBeenCalledTimes(1);

    // Dedupe is per event delivery, not global: the next event fires again.
    emit("transcripts", ["s1"]);
    expect(sharedA).toHaveBeenCalledTimes(2);

    // After the first copy unsubscribes, the surviving duplicate takes over.
    unsubscribes[0]!();
    emit("transcripts", ["s1"]);
    expect(sharedA).toHaveBeenCalledTimes(2);
    expect(sharedB).toHaveBeenCalledTimes(1);

    for (const unsubscribe of unsubscribes.slice(1)) {
      unsubscribe();
    }
  });
});
