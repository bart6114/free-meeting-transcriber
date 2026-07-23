import { useMemo } from "react";

import type { EventParticipant } from "@hypr/store";

import { liveQueryClient, useLiveQuery } from "~/db";
import type {
  TimelineEventRow,
  TimelineEventsTable,
  TimelineSessionRow,
  TimelineSessionsTable,
} from "~/sidebar/timeline/utils";
import { useUndoDelete } from "~/store/zustand/undo-delete";

type TimelineEventSqlRow = Omit<
  TimelineEventRow,
  "has_recurrence_rules" | "is_all_day"
> & {
  id: string;
  has_recurrence_rules: boolean | number;
  is_all_day: boolean | number;
};

type TimelineSessionSqlRow = TimelineSessionRow & { id: string };

type EventParticipantsSqlRow = { participants_json: string };

type CalendarEventStartSqlRow = { started_at: string };

type NearbyCalendarEventSqlRow = {
  id: string;
  title: string;
  started_at: string;
  meeting_link: string;
  location: string;
  description: string;
  participants_json: string | null;
};

export type NearbyCalendarEvent = {
  id: string;
  title: string;
  meetingLink?: string;
  location?: string;
  description?: string;
  participantNames: string[];
};

const EMPTY_EVENTS: Record<string, TimelineEventRow> = {};
const EMPTY_SESSIONS: Record<string, TimelineSessionRow> = {};
const EMPTY_EVENT_PARTICIPANTS: EventParticipant[] = [];

export function useTimelineTables(): {
  timelineEventsTable: TimelineEventsTable;
  timelineSessionsTable: TimelineSessionsTable;
} {
  const timelineEventsTable = useTimelineEventsTable();
  const timelineSessionsTable = useTimelineSessionsTable();

  return { timelineEventsTable, timelineSessionsTable };
}

export function useTimelineEventsTable(): TimelineEventsTable {
  const { data: timelineEventsTable = EMPTY_EVENTS } = useLiveQuery<
    TimelineEventSqlRow,
    Record<string, TimelineEventRow>
  >({
    sql: `
      SELECT
        event.id,
        event.title,
        event.started_at,
        event.ended_at,
        event.calendar_id,
        event.tracking_id_event,
        event.has_recurrence_rules,
        event.recurrence_series_id,
        event.is_all_day,
        event.location,
        event.meeting_link,
        event.description,
        calendar.color AS calendar_color
      FROM events AS event
      LEFT JOIN calendars AS calendar
        ON calendar.id = event.calendar_id AND calendar.deleted_at IS NULL
      WHERE event.deleted_at IS NULL
      ORDER BY event.started_at, event.id
    `,
    mapRows: mapTimelineEventRows,
  });

  return timelineEventsTable;
}

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

export async function getCalendarEventStartedAt(
  eventId: string,
): Promise<string | null> {
  const rows = await liveQueryClient.execute<CalendarEventStartSqlRow>(
    `
      SELECT started_at
      FROM events
      WHERE id = ? AND deleted_at IS NULL
      LIMIT 1
    `,
    [eventId],
  );
  return rows[0]?.started_at || null;
}

export async function getNearbyCalendarEvents(
  nowMs: number,
  windowMs: number,
): Promise<NearbyCalendarEvent[]> {
  const rows = await liveQueryClient.execute<NearbyCalendarEventSqlRow>(
    `
      SELECT
        id,
        title,
        started_at,
        meeting_link,
        location,
        description,
        participants_json
      FROM events
      WHERE deleted_at IS NULL
        AND is_all_day = 0
        AND started_at <> ''
        AND abs(
          ((julianday(started_at) - 2440587.5) * 86400000) - ?
        ) <= ?
      ORDER BY abs(
        ((julianday(started_at) - 2440587.5) * 86400000) - ?
      ), julianday(started_at), id
    `,
    [nowMs, windowMs, nowMs],
  );

  return rows.map((row) => ({
    id: row.id,
    title: row.title || "Untitled Event",
    meetingLink: row.meeting_link || undefined,
    location: row.location || undefined,
    description: row.description || undefined,
    participantNames: [
      ...new Set(
        parseEventParticipants(row.participants_json ?? undefined)
          .filter((participant) => participant.is_current_user !== true)
          .map((participant) => participant.name?.trim() ?? "")
          .filter(Boolean),
      ),
    ],
  }));
}

export function useSessionEventParticipants(
  sessionId: string,
): EventParticipant[] {
  const { data = EMPTY_EVENT_PARTICIPANTS } = useLiveQuery<
    EventParticipantsSqlRow,
    EventParticipant[]
  >({
    sql: `
      SELECT event.participants_json
      FROM sessions AS session
      JOIN events AS event
        ON event.deleted_at IS NULL
        AND (
          event.id = session.event_id
          OR (
            event.tracking_id_event = CASE
              WHEN json_valid(session.event_json)
              THEN json_extract(session.event_json, '$.tracking_id')
              ELSE ''
            END
            AND event.calendar_id = CASE
              WHEN json_valid(session.event_json)
              THEN json_extract(session.event_json, '$.calendar_id')
              ELSE ''
            END
          )
        )
      WHERE session.id = ? AND session.deleted_at IS NULL
      ORDER BY event.started_at, event.id
      LIMIT 1
    `,
    params: [sessionId],
    enabled: Boolean(sessionId),
    mapRows: (rows) => parseEventParticipants(rows[0]?.participants_json),
  });
  return sessionId ? data : EMPTY_EVENT_PARTICIPANTS;
}

export function mapTimelineEventRows(
  rows: TimelineEventSqlRow[],
): Record<string, TimelineEventRow> {
  return Object.fromEntries(
    rows.map(({ id, ...row }) => [
      id,
      {
        ...row,
        has_recurrence_rules: Boolean(row.has_recurrence_rules),
        is_all_day: Boolean(row.is_all_day),
      },
    ]),
  );
}

export function mapTimelineSessionRows(
  rows: TimelineSessionSqlRow[],
): Record<string, TimelineSessionRow> {
  return Object.fromEntries(rows.map(({ id, ...row }) => [id, row]));
}

export function parseEventParticipants(
  value: string | undefined,
): EventParticipant[] {
  if (!value) return EMPTY_EVENT_PARTICIPANTS;

  try {
    const parsed = JSON.parse(value);
    return Array.isArray(parsed)
      ? (parsed as EventParticipant[])
      : EMPTY_EVENT_PARTICIPANTS;
  } catch {
    return EMPTY_EVENT_PARTICIPANTS;
  }
}
