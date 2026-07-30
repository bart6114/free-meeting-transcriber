import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { events, type IndexEntity } from "~/types/tauri.gen";

type IndexSubscription = {
  entities: readonly IndexEntity[];
  ids?: readonly string[];
  onChange: () => void;
};

const subscriptions = new Set<IndexSubscription>();
let listenerStarted = false;

// One tauri event listener per webview (started lazily, never unlistened -- module
// lifetime), fanning each coalesced `index-changed` event out to every mounted
// subscriber. Each window is its own JS context, so every webview gets its own
// singleton and the backend's emit-to-all reaches them all.
function ensureIndexChangedListener() {
  if (listenerStarted) {
    return;
  }
  listenerStarted = true;
  events.indexChanged
    .listen(({ payload }) => {
      for (const subscription of subscriptions) {
        if (!subscription.entities.includes(payload.entity)) {
          continue;
        }
        // Id-scoped skip only when both sides carry ids; an empty payload is
        // treated as "anything may have changed".
        if (
          subscription.ids &&
          payload.ids.length > 0 &&
          !payload.ids.some((id) => subscription.ids?.includes(id))
        ) {
          continue;
        }
        subscription.onChange();
      }
    })
    .catch((error) => {
      listenerStarted = false;
      console.error("[index-query] failed to listen for index changes", error);
    });
}

/**
 * Non-hook variant for imperative subscribers (meeting-float host, event listeners):
 * calls `onChange` whenever a matching `index-changed` event arrives. Returns an
 * unsubscribe function.
 */
export function subscribeIndexChanged(
  entity: IndexEntity | readonly IndexEntity[],
  onChange: () => void,
  ids?: readonly string[],
): () => void {
  ensureIndexChangedListener();
  const subscription: IndexSubscription = {
    entities: Array.isArray(entity) ? entity : [entity as IndexEntity],
    ids,
    onChange,
  };
  subscriptions.add(subscription);
  return () => {
    subscriptions.delete(subscription);
  };
}

/**
 * `useQuery` that re-fetches when the vault index reports a change to the given
 * entity (optionally scoped to specific event ids -- note that `docs` and
 * `transcripts` events carry *session* ids). Replaces the SQL live-query hooks:
 * consumers keep the same `{ data, isLoading, error }` contract.
 */
export function useIndexQuery<TData>({
  entity,
  ids,
  queryKey,
  queryFn,
  enabled = true,
}: {
  entity: IndexEntity | readonly IndexEntity[];
  ids?: readonly string[];
  queryKey: readonly unknown[];
  queryFn: () => Promise<TData>;
  enabled?: boolean;
}) {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: queryKey as unknown[],
    queryFn,
    enabled,
  });

  // The stringified key stands in for the (per-render) array/object identities.
  const subscriptionKey = JSON.stringify([entity, ids, queryKey]);
  useEffect(() => {
    if (!enabled) {
      return;
    }
    return subscribeIndexChanged(
      entity,
      () => {
        void queryClient.invalidateQueries({
          queryKey: queryKey as unknown[],
          exact: true,
        });
      },
      ids,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, queryClient, subscriptionKey]);

  return query;
}
