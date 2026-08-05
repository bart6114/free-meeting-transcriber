import {
  differenceInCalendarDays,
  differenceInCalendarMonths,
  safeParseDate,
  startOfDay,
  startOfMonth,
  TZDate,
} from "@hypr/utils";

import { getSessionEvent } from "~/session/utils";

function toTZ(date: Date, timezone?: string): Date {
  return timezone ? new TZDate(date, timezone) : date;
}

export type TimelineSessionRow = {
  title?: string | null;
  created_at?: string | null;
  event_json?: string | null;
  folder_id?: string | null;
};

export type TimelineSessionsTable =
  | Record<string, TimelineSessionRow>
  | null
  | undefined;

export type TimelineItem = {
  type: "session";
  id: string;
  data: TimelineSessionRow;
};

export type TimelinePrecision = "time" | "date";

export type TimelineBucket = {
  label: string;
  precision: TimelinePrecision;
  items: TimelineItem[];
};

export type TimelineWindowData = {
  timelineSessionsTable: TimelineSessionsTable;
  hasMoreFutureItems: boolean;
};

export type TimelineIndicatorPlacement =
  | { type: "inside"; index: number; progress: number }
  | { type: "before"; index: number }
  | { type: "after" };

export function getBucketInfo(
  date: Date,
  timezone?: string,
): {
  label: string;
  sortKey: number;
  precision: TimelinePrecision;
} {
  const now = new Date();
  const tzDate = toTZ(date, timezone);
  const tzNow = toTZ(now, timezone);
  const daysDiff = differenceInCalendarDays(tzDate, tzNow);
  const sortKey = startOfDay(tzDate).getTime();
  const absDays = Math.abs(daysDiff);

  if (daysDiff === 0) {
    return { label: "Today", sortKey, precision: "time" };
  }

  if (daysDiff === -1) {
    return { label: "Yesterday", sortKey, precision: "time" };
  }

  if (daysDiff === 1) {
    return { label: "Tomorrow", sortKey, precision: "time" };
  }

  if (daysDiff < 0) {
    if (absDays <= 6) {
      return { label: `${absDays} days ago`, sortKey, precision: "time" };
    }

    if (absDays <= 27) {
      const weeks = Math.max(1, Math.round(absDays / 7));
      const weekRangeEndDay = Math.max(7, weeks * 7 - 3);
      const weekRangeEnd = new Date(
        now.getTime() - weekRangeEndDay * 24 * 60 * 60 * 1000,
      );
      const weekSortKey = startOfDay(toTZ(weekRangeEnd, timezone)).getTime();

      return {
        label: weeks === 1 ? "a week ago" : `${weeks} weeks ago`,
        sortKey: weekSortKey,
        precision: "date",
      };
    }

    let months = Math.abs(differenceInCalendarMonths(tzDate, tzNow));
    if (months === 0) {
      months = 1;
    }
    const monthStartKey = startOfMonth(tzDate).getTime();
    const lastDayInMonthBucket = new Date(
      now.getTime() - 28 * 24 * 60 * 60 * 1000,
    );
    const lastDayKey = startOfDay(
      toTZ(lastDayInMonthBucket, timezone),
    ).getTime();
    const monthSortKey = Math.min(monthStartKey, lastDayKey);
    return {
      label: months === 1 ? "a month ago" : `${months} months ago`,
      sortKey: monthSortKey,
      precision: "date",
    };
  }

  if (absDays <= 6) {
    return { label: `in ${absDays} days`, sortKey, precision: "time" };
  }

  if (absDays <= 27) {
    const weeks = Math.max(1, Math.round(absDays / 7));
    const weekRangeStartDay = Math.max(7, weeks * 7 - 3);
    const weekRangeStart = new Date(
      now.getTime() + weekRangeStartDay * 24 * 60 * 60 * 1000,
    );
    const weekSortKey = startOfDay(toTZ(weekRangeStart, timezone)).getTime();

    return {
      label: weeks === 1 ? "next week" : `in ${weeks} weeks`,
      sortKey: weekSortKey,
      precision: "date",
    };
  }

  let months = differenceInCalendarMonths(tzDate, tzNow);
  if (months === 0) {
    months = 1;
  }
  const monthStartKey = startOfMonth(tzDate).getTime();
  const firstDayInMonthBucket = new Date(
    now.getTime() + 28 * 24 * 60 * 60 * 1000,
  );
  const firstDayKey = startOfDay(
    toTZ(firstDayInMonthBucket, timezone),
  ).getTime();
  const monthSortKey = Math.max(monthStartKey, firstDayKey);
  return {
    label: months === 1 ? "next month" : `in ${months} months`,
    sortKey: monthSortKey,
    precision: "date",
  };
}

export function calculateIndicatorIndex(
  entries: Array<{ timestamp: Date | null }>,
  current: Date,
): number {
  const index = entries.findIndex(({ timestamp }) => {
    if (!timestamp) {
      return true;
    }

    return timestamp.getTime() < current.getTime();
  });

  if (index === -1) {
    return entries.length;
  }

  return index;
}

export function getItemTimeRange(item: TimelineItem): {
  start: Date | null;
  end: Date | null;
} {
  const sessionEvent = getSessionEvent(item.data);
  return {
    start: safeParseDate(sessionEvent?.started_at ?? item.data.created_at),
    end: safeParseDate(sessionEvent?.ended_at),
  };
}

export function calculateTodayIndicatorPlacement(
  entries: Array<{ item: TimelineItem; timestamp: Date | null }>,
  current: Date,
): TimelineIndicatorPlacement {
  const currentMs = current.getTime();

  const insideIndex = entries.findIndex(({ item }) => {
    const { start, end } = getItemTimeRange(item);
    if (!start || !end) {
      return false;
    }

    const startMs = start.getTime();
    const endMs = end.getTime();

    return startMs <= currentMs && currentMs <= endMs && endMs > startMs;
  });

  if (insideIndex !== -1) {
    const { start, end } = getItemTimeRange(entries[insideIndex].item);
    const startMs = start!.getTime();
    const endMs = end!.getTime();
    const progress = (currentMs - startMs) / (endMs - startMs);

    return {
      type: "inside",
      index: insideIndex,
      progress: Math.min(Math.max(progress, 0), 1),
    };
  }

  const indicatorIndex = calculateIndicatorIndex(entries, current);
  if (indicatorIndex === entries.length) {
    return { type: "after" };
  }

  return { type: "before", index: indicatorIndex };
}

export function getItemTimestamp(item: TimelineItem): Date | null {
  return getItemTimeRange(item).start;
}

export function isTimelineItemInFuture(item: TimelineItem): boolean {
  const timestamp = getItemTimestamp(item);
  if (!timestamp) {
    return false;
  }

  return timestamp.getTime() > Date.now();
}

export function hasFutureTimelineItems(
  buckets: TimelineBucket[],
  nowMs: number,
): boolean {
  return buckets.some((bucket) =>
    bucket.items.some((item) => {
      const { start, end } = getItemTimeRange(item);
      if (!start) {
        return false;
      }

      if (start.getTime() > nowMs) {
        return true;
      }

      return Boolean(end && end.getTime() > nowMs);
    }),
  );
}

function getTomorrowUpperBound(timezone?: string): number {
  const dayAfterTomorrow = new Date(Date.now() + 2 * 24 * 60 * 60 * 1000);
  return startOfDay(toTZ(dayAfterTomorrow, timezone)).getTime();
}

function isAtOrBeforeTomorrow(date: Date | null, timezone?: string): boolean {
  if (!date) {
    return true;
  }

  return date.getTime() < getTomorrowUpperBound(timezone);
}

function isAfterTomorrow(date: Date | null, timezone?: string): boolean {
  if (!date) {
    return false;
  }

  return date.getTime() >= getTomorrowUpperBound(timezone);
}

export function filterTimelineTablesUpToTomorrow({
  timelineSessionsTable,
  timezone,
}: {
  timelineSessionsTable: TimelineSessionsTable;
  timezone?: string;
}): {
  timelineSessionsTable: TimelineSessionsTable;
} {
  return {
    timelineSessionsTable: timelineSessionsTable
      ? Object.fromEntries(
          Object.entries(timelineSessionsTable).filter(([, row]) =>
            isAtOrBeforeTomorrow(
              safeParseDate(getSessionEvent(row)?.started_at ?? row.created_at),
              timezone,
            ),
          ),
        )
      : timelineSessionsTable,
  };
}

export function deriveTimelineWindowData({
  timelineSessionsTable,
  timezone,
}: {
  timelineSessionsTable: TimelineSessionsTable;
  timezone?: string;
}): TimelineWindowData {
  const filteredSessionsTable = timelineSessionsTable
    ? ({} as Record<string, TimelineSessionRow>)
    : timelineSessionsTable;
  let hasMoreFutureItems = false;

  if (timelineSessionsTable && filteredSessionsTable) {
    for (const [sessionId, row] of Object.entries(timelineSessionsTable)) {
      const date = safeParseDate(
        getSessionEvent(row)?.started_at ?? row.created_at,
      );

      if (isAfterTomorrow(date, timezone)) {
        hasMoreFutureItems = true;
        continue;
      }

      if (isAtOrBeforeTomorrow(date, timezone)) {
        filteredSessionsTable[sessionId] = row;
      }
    }
  }

  return {
    timelineSessionsTable: filteredSessionsTable,
    hasMoreFutureItems,
  };
}

export function hasTimelineItemsAfterTomorrow({
  timelineSessionsTable,
  timezone,
}: {
  timelineSessionsTable: TimelineSessionsTable;
  timezone?: string;
}): boolean {
  return Boolean(
    timelineSessionsTable &&
    Object.values(timelineSessionsTable).some((row) =>
      isAfterTomorrow(
        safeParseDate(getSessionEvent(row)?.started_at ?? row.created_at),
        timezone,
      ),
    ),
  );
}

export function buildTimelineBuckets({
  timelineSessionsTable,
  timezone,
}: {
  timelineSessionsTable: TimelineSessionsTable;
  timezone?: string;
}): TimelineBucket[] {
  const items: TimelineItem[] = [];

  if (timelineSessionsTable) {
    Object.entries(timelineSessionsTable).forEach(([sessionId, row]) => {
      const sessionEvent = getSessionEvent(row);
      const startTime = safeParseDate(
        sessionEvent?.started_at ?? row.created_at,
      );

      if (!startTime) {
        return;
      }

      items.push({
        type: "session",
        id: sessionId,
        data: row,
      });
    });
  }

  items.sort((a, b) => {
    const dateA = getItemTimestamp(a);
    const dateB = getItemTimestamp(b);
    const timeAValue = dateA?.getTime() ?? 0;
    const timeBValue = dateB?.getTime() ?? 0;
    if (timeBValue == timeAValue) {
      return (a.data.title ?? "Untitled") > (b.data.title ?? "Untitled")
        ? 1
        : (a.data.title ?? "Untitled") < (b.data.title ?? "Untitled")
          ? -1
          : 0;
    }
    return timeBValue - timeAValue;
  });

  const bucketMap = new Map<
    string,
    { sortKey: number; precision: TimelinePrecision; items: TimelineItem[] }
  >();

  items.forEach((item) => {
    const bucket = getBucketInfo(
      getItemTimestamp(item) ?? new Date(0),
      timezone,
    );

    if (!bucketMap.has(bucket.label)) {
      bucketMap.set(bucket.label, {
        sortKey: bucket.sortKey,
        precision: bucket.precision,
        items: [],
      });
    }
    bucketMap.get(bucket.label)!.items.push(item);
  });

  return Array.from(bucketMap.entries())
    .sort((a, b) => b[1].sortKey - a[1].sortKey)
    .map(
      ([label, value]) =>
        ({
          label,
          items: value.items,
          precision: value.precision,
        }) satisfies TimelineBucket,
    );
}
