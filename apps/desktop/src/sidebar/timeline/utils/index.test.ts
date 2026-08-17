import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import {
  buildTagTimelineBuckets,
  buildTimelineBuckets,
  calculateTodayIndicatorPlacement,
  deriveTimelineWindowData,
  filterTimelineTablesUpToTomorrow,
  getBucketInfo,
  hasFutureTimelineItems,
  hasTimelineItemsAfterTomorrow,
  isTimelineItemInFuture,
  type TimelineBucket,
  type TimelineSessionsTable,
} from ".";

process.env.TZ = "UTC";

const SYSTEM_TIME = new Date("2024-01-15T12:00:00.000Z");

describe("timeline utils", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(SYSTEM_TIME);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("getBucketInfo returns Today for current date", () => {
    const info = getBucketInfo(new Date("2024-01-15T05:00:00.000Z"));
    expect(info).toMatchObject({ label: "Today", precision: "time" });
  });

  test("getBucketInfo groups recent past days", () => {
    const info = getBucketInfo(new Date("2024-01-10T05:00:00.000Z"));
    expect(info).toMatchObject({ label: "5 days ago", precision: "time" });
  });

  test("getBucketInfo groups distant future months", () => {
    const info = getBucketInfo(new Date("2024-03-20T12:00:00.000Z"));
    expect(info).toMatchObject({ label: "in 2 months", precision: "date" });
  });

  test("buildTagTimelineBuckets groups by tag alphabetically with Untagged last", () => {
    const table: TimelineSessionsTable = {
      "session-1": {
        title: "Sprint planning",
        created_at: "2024-01-14T10:00:00.000Z",
        tags: ["project-x"],
      },
      "session-2": {
        title: "Interview",
        created_at: "2024-01-15T10:00:00.000Z",
        tags: ["hiring", "project-x"],
      },
      "session-3": {
        title: "Quick memo",
        created_at: "2024-01-13T10:00:00.000Z",
        tags: [],
      },
    };

    const buckets = buildTagTimelineBuckets({ timelineSessionsTable: table });

    expect(
      buckets.map((bucket) => ({
        label: bucket.label,
        kind: bucket.kind,
        ids: bucket.items.map((item) => item.id),
      })),
    ).toEqual([
      { label: "hiring", kind: "tag", ids: ["session-2"] },
      // Multi-tag sessions appear under every tag; newest-first within a bucket.
      { label: "project-x", kind: "tag", ids: ["session-2", "session-1"] },
      { label: "Untagged", kind: "tag", ids: ["session-3"] },
    ]);
    expect(buckets.every((bucket) => bucket.precision === "date")).toBe(true);
  });

  test("buildTagTimelineBuckets omits the Untagged bucket when every session is tagged", () => {
    const buckets = buildTagTimelineBuckets({
      timelineSessionsTable: {
        "session-1": {
          title: "Standup",
          created_at: "2024-01-15T10:00:00.000Z",
          tags: ["standup"],
        },
      },
    });

    expect(buckets.map((bucket) => bucket.label)).toEqual(["standup"]);
  });

  test("calculateTodayIndicatorPlacement places indicator inside an active timed session", () => {
    const placement = calculateTodayIndicatorPlacement(
      [
        {
          item: {
            type: "session",
            id: "session-1",
            data: {
              title: "test",
              created_at: "2024-01-15T11:30:00.000Z",
              event_json: JSON.stringify({
                started_at: "2024-01-15T11:30:00.000Z",
                ended_at: "2024-01-15T12:30:00.000Z",
              }),
            },
          },
          timestamp: new Date("2024-01-15T11:30:00.000Z"),
        },
      ],
      new Date("2024-01-15T12:00:00.000Z"),
    );

    expect(placement).toMatchObject({
      type: "inside",
      index: 0,
      progress: 0.5,
    });
  });

  test("calculateTodayIndicatorPlacement falls back to seam placement for future-only items", () => {
    const placement = calculateTodayIndicatorPlacement(
      [
        {
          item: {
            type: "session",
            id: "session-1",
            data: {
              title: "Future Session",
              created_at: "2024-01-10T12:00:00.000Z",
              event_json: JSON.stringify({
                started_at: "2024-01-15T13:00:00.000Z",
                ended_at: "2024-01-15T14:00:00.000Z",
              }),
            },
          },
          timestamp: new Date("2024-01-15T13:00:00.000Z"),
        },
      ],
      new Date("2024-01-15T12:00:00.000Z"),
    );

    expect(placement).toEqual({ type: "after" });
  });

  test("buildTimelineBuckets excludes Today bucket when empty", () => {
    const buckets = buildTimelineBuckets({
      timelineSessionsTable: null,
    });

    const todayBucket = buckets.find((bucket) => bucket.label === "Today");
    expect(todayBucket).toBeUndefined();
  });

  test("isTimelineItemInFuture only returns true for future-starting items", () => {
    expect(
      isTimelineItemInFuture({
        type: "session",
        id: "future-session",
        data: {
          title: "Future Session",
          created_at: "2024-01-10T12:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2024-01-16T11:00:00.000Z",
          }),
        },
      }),
    ).toBe(true);

    expect(
      isTimelineItemInFuture({
        type: "session",
        id: "past-session",
        data: {
          title: "Past Session",
          created_at: "2024-01-14T12:00:00.000Z",
        },
      }),
    ).toBe(false);
  });

  test("hasFutureTimelineItems detects a future-starting item", () => {
    expect(
      hasFutureTimelineItems(
        bucketsWith({
          title: "Future Session",
          created_at: "2024-01-14T12:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2024-01-16T11:00:00.000Z",
          }),
        }),
        SYSTEM_TIME.getTime(),
      ),
    ).toBe(true);
  });

  test("hasFutureTimelineItems counts an in-progress event as future-facing", () => {
    expect(
      hasFutureTimelineItems(
        bucketsWith({
          title: "Running Meeting",
          created_at: "2024-01-15T11:30:00.000Z",
          event_json: JSON.stringify({
            started_at: "2024-01-15T11:30:00.000Z",
            ended_at: "2024-01-15T12:30:00.000Z",
          }),
        }),
        SYSTEM_TIME.getTime(),
      ),
    ).toBe(true);
  });

  test("hasFutureTimelineItems returns false for past-only items", () => {
    expect(
      hasFutureTimelineItems(
        bucketsWith({
          title: "Past Session",
          created_at: "2024-01-14T12:00:00.000Z",
        }),
        SYSTEM_TIME.getTime(),
      ),
    ).toBe(false);
  });

  test("hasFutureTimelineItems ignores unparseable timestamps and empty buckets", () => {
    expect(hasFutureTimelineItems([], SYSTEM_TIME.getTime())).toBe(false);
    expect(
      hasFutureTimelineItems(
        bucketsWith({
          title: "Broken Session",
          created_at: "not-a-date",
        }),
        SYSTEM_TIME.getTime(),
      ),
    ).toBe(false);
  });

  test("filterTimelineTablesUpToTomorrow keeps tomorrow and removes later items", () => {
    const filtered = filterTimelineTablesUpToTomorrow({
      timelineSessionsTable: {
        tomorrow: {
          title: "Tomorrow Session",
          created_at: "2024-01-14T12:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2024-01-16T11:00:00.000Z",
          }),
        },
        later: {
          title: "Later Session",
          created_at: "2024-01-14T12:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2024-01-17T11:00:00.000Z",
          }),
        },
      },
    });

    expect(Object.keys(filtered.timelineSessionsTable ?? {})).toEqual([
      "tomorrow",
    ]);
  });

  test("hasTimelineItemsAfterTomorrow only returns true for items after tomorrow", () => {
    expect(
      hasTimelineItemsAfterTomorrow({
        timelineSessionsTable: {
          later: {
            title: "Later Session",
            created_at: "2024-01-14T12:00:00.000Z",
            event_json: JSON.stringify({
              started_at: "2024-01-17T11:00:00.000Z",
            }),
          },
        },
      }),
    ).toBe(true);

    expect(
      hasTimelineItemsAfterTomorrow({
        timelineSessionsTable: {
          tomorrow: {
            title: "Tomorrow Session",
            created_at: "2024-01-14T12:00:00.000Z",
            event_json: JSON.stringify({
              started_at: "2024-01-16T11:00:00.000Z",
            }),
          },
        },
      }),
    ).toBe(false);
  });

  test("deriveTimelineWindowData separates tomorrow-or-earlier sessions from later ones", () => {
    const derived = deriveTimelineWindowData({
      timelineSessionsTable: {
        tomorrow: {
          title: "Tomorrow Session",
          created_at: "2024-01-14T12:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2024-01-16T11:00:00.000Z",
          }),
        },
        later: {
          title: "Later Session",
          created_at: "2024-01-14T12:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2024-01-17T11:00:00.000Z",
          }),
        },
      },
    });

    expect(Object.keys(derived.timelineSessionsTable ?? {})).toEqual([
      "tomorrow",
    ]);
    expect(derived.hasMoreFutureItems).toBe(true);
  });

  test("buildTimelineBuckets sorts buckets by most recent first", () => {
    const timelineSessionsTable: TimelineSessionsTable = {
      "session-future": {
        title: "Future Session",
        created_at: "2024-01-10T12:00:00.000Z",
        event_json: JSON.stringify({ started_at: "2024-01-16T09:00:00.000Z" }),
      },
      "session-past": {
        title: "Past Session",
        created_at: "2024-01-14T09:00:00.000Z",
      },
    };

    const buckets = buildTimelineBuckets({
      timelineSessionsTable,
    });

    expect(buckets.map((bucket) => bucket.label)).toEqual([
      "Tomorrow",
      "Yesterday",
    ]);
  });

  test("getBucketInfo: future month bucket sorts after all week buckets", () => {
    // System time is 2024-01-15
    // Week buckets: absDays <= 27, Month buckets: absDays > 27
    // "in 4 weeks" = ~25-27 days, "next month" = 28+ days
    const in4Weeks = getBucketInfo(new Date("2024-02-11T12:00:00.000Z")); // 27 days out (last day of week bucket)
    const nextMonth = getBucketInfo(new Date("2024-02-13T12:00:00.000Z")); // 29 days out (first day of month bucket)

    expect(in4Weeks.label).toBe("in 4 weeks");
    expect(nextMonth.label).toBe("next month");
    expect(nextMonth.sortKey).toBeGreaterThan(in4Weeks.sortKey);
  });

  test("getBucketInfo: past month bucket sorts before all week buckets", () => {
    // Week buckets: absDays <= 27, Month buckets: absDays > 27
    // "4 weeks ago" = ~25-27 days ago, "a month ago" = 28+ days ago
    const weeksAgo4 = getBucketInfo(new Date("2023-12-19T12:00:00.000Z")); // 27 days ago (last day of week bucket)
    const monthAgo = getBucketInfo(new Date("2023-12-17T12:00:00.000Z")); // 29 days ago (first day of month bucket)

    expect(weeksAgo4.label).toBe("4 weeks ago");
    expect(monthAgo.label).toBe("a month ago");
    expect(monthAgo.sortKey).toBeLessThan(weeksAgo4.sortKey);
  });

  test("buildTimelineBuckets: future buckets sort correctly (weeks before months)", () => {
    const timelineSessionsTable: TimelineSessionsTable = {
      "session-2weeks": {
        title: "In 2 weeks",
        event_json: JSON.stringify({ started_at: "2024-01-29T09:00:00.000Z" }), // 14 days -> "in 2 weeks"
        created_at: "2024-01-10T12:00:00.000Z",
      },
      "session-4weeks": {
        title: "In 4 weeks",
        event_json: JSON.stringify({ started_at: "2024-02-11T09:00:00.000Z" }), // 27 days -> "in 4 weeks"
        created_at: "2024-01-10T12:00:00.000Z",
      },
      "session-nextmonth": {
        title: "Next month",
        event_json: JSON.stringify({ started_at: "2024-02-13T09:00:00.000Z" }), // 29 days -> "next month"
        created_at: "2024-01-10T12:00:00.000Z",
      },
    };

    const buckets = buildTimelineBuckets({
      timelineSessionsTable,
    });

    // Should be: next month, in 4 weeks, in 2 weeks (furthest future first)
    expect(buckets.map((b) => b.label)).toEqual([
      "next month",
      "in 4 weeks",
      "in 2 weeks",
    ]);
  });
});

function bucketsWith(data: {
  title: string;
  created_at: string;
  event_json?: string;
}): TimelineBucket[] {
  return [
    {
      label: "Today",
      precision: "time",
      items: [{ type: "session", id: "session-1", data }],
    },
  ];
}
