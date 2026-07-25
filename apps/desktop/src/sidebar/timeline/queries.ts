import { useMemo } from "react";

import { useLiveQuery } from "~/db";
import type {
  TimelineSessionRow,
  TimelineSessionsTable,
} from "~/sidebar/timeline/utils";
import { useUndoDelete } from "~/store/zustand/undo-delete";

type TimelineSessionSqlRow = TimelineSessionRow & { id: string };

const EMPTY_SESSIONS: Record<string, TimelineSessionRow> = {};

export function useTimelineSessionsTable(): TimelineSessionsTable {
  const { data: timelineSessionsTable = EMPTY_SESSIONS } = useLiveQuery<
    TimelineSessionSqlRow,
    Record<string, TimelineSessionRow>
  >({
    sql: `
      SELECT
        id,
        title,
        created_at,
        event_json,
        folder_path AS folder_id
      FROM sessions
      WHERE deleted_at IS NULL
      ORDER BY created_at, id
    `,
    mapRows: mapTimelineSessionRows,
  });
  const pendingDeletions = useUndoDelete((state) => state.pendingDeletions);

  // Sessions with a pending deletion are hidden optimistically, before the
  // soft-delete write commits and the live query re-emits.
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

function mapTimelineSessionRows(
  rows: TimelineSessionSqlRow[],
): Record<string, TimelineSessionRow> {
  return Object.fromEntries(rows.map(({ id, ...row }) => [id, row]));
}
