import { useMemo } from "react";

import { useIndexQuery } from "~/shared/index-query";
import type {
  TimelineSessionRow,
  TimelineSessionsTable,
} from "~/sidebar/timeline/utils";
import { useUndoDelete } from "~/store/zustand/undo-delete";
import { commands, type SessionListEntry } from "~/types/tauri.gen";

const EMPTY_SESSIONS: Record<string, TimelineSessionRow> = {};

export function useTimelineSessionsTable(): TimelineSessionsTable {
  const { data: timelineSessionsTable = EMPTY_SESSIONS } = useIndexQuery({
    entity: "sessions",
    queryKey: ["timeline-sessions"],
    queryFn: async () => {
      const result = await commands.sessionList();
      if (result.status === "error") {
        throw new Error(result.error);
      }
      return mapTimelineSessionEntries(result.data);
    },
  });
  const pendingDeletions = useUndoDelete((state) => state.pendingDeletions);

  // Sessions with a pending deletion are hidden optimistically, before the
  // delete write commits and the index re-emits.
  return useMemo(() => {
    const pendingIds = Object.keys(pendingDeletions).filter(
      (sessionId) => sessionId in timelineSessionsTable,
    );
    if (pendingIds.length === 0) return timelineSessionsTable;

    const filtered = { ...timelineSessionsTable };
    for (const sessionId of pendingIds) {
      delete filtered[sessionId];
    }
    return filtered;
  }, [timelineSessionsTable, pendingDeletions]);
}

// session_list is already (created_at, id) ASC -- the order the timeline expects.
function mapTimelineSessionEntries(
  entries: SessionListEntry[],
): Record<string, TimelineSessionRow> {
  return Object.fromEntries(
    entries.map(({ meta }) => [
      meta.id,
      {
        title: meta.title,
        created_at: meta.created_at,
        folder_id: meta.folder ?? "",
        tags: meta.tags,
      },
    ]),
  );
}
