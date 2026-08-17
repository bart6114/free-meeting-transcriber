import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { sessionAttachmentPathsQueryKey } from "~/session/hooks/useAttachmentResolver";
import { subscribeIndexChanged } from "~/shared/index-query";

/**
 * The one invalidation path for caches holding absolute session paths (audio
 * url/peaks/existence, attachment paths). The backend emits a `locations` index
 * event whenever a session's physical directory changes -- first-title rename,
 * migration, delete/restore, fs-sync move, or an external Finder rename caught
 * by a rebuild -- so no title-mutation site needs to know about path caches.
 *
 * Mounted per window (not main-window-gated): the standalone session window
 * caches these paths too.
 */
export function LocationInvalidationSync() {
  const queryClient = useQueryClient();

  useEffect(() => {
    return subscribeIndexChanged("locations", (ids) => {
      for (const sessionId of ids) {
        void queryClient.invalidateQueries({ queryKey: ["audio", sessionId] });
        void queryClient.invalidateQueries({
          queryKey: sessionAttachmentPathsQueryKey(sessionId),
        });
      }
    });
  }, [queryClient]);

  return null;
}
