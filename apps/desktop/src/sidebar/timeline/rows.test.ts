import { describe, expect, it } from "vitest";

import {
  buildTimelineRows,
  getTimelineRowHeight,
  TIMELINE_ROW_HEIGHT,
} from "./rows";
import { makeTimelineItem, type TimelineBucket } from "./utils";

describe("timeline rows", () => {
  it("flattens thousands of sessions without losing the full logical list", () => {
    const items = Array.from({ length: 3_500 }, (_, index) =>
      makeTimelineItem(`session-${index}`, {
        title: `Note ${index}`,
        created_at: new Date(
          Date.UTC(2024, 0, 14) - index * 60_000,
        ).toISOString(),
      }),
    );
    const rows = buildTimelineRows({
      buckets: [bucket("Yesterday", items)],
      currentTimeMs: Date.UTC(2024, 0, 15),
      fallbackIndicatorIndex: -1,
      groupBy: "date",
      hasToday: false,
      suppressCurrentTimeIndicator: false,
    });

    expect(rows).toHaveLength(3_502);
    expect(rows[0]?.key).toBe("bucket:Yesterday");
    expect(rows[1]?.key).toBe("session:Yesterday:session-0");
    expect(rows[rows.length - 1]?.key).toBe("current-time:fallback");
  });

  it("gives repeated tag sessions unique row keys", () => {
    const shared = makeTimelineItem("shared", {
      title: "Shared note",
      created_at: "2024-01-14T12:00:00.000Z",
    });
    const rows = buildTimelineRows({
      buckets: [
        { ...bucket("work", [shared]), kind: "tag" },
        { ...bucket("personal", [shared]), kind: "tag" },
      ],
      currentTimeMs: Date.UTC(2024, 0, 15),
      fallbackIndicatorIndex: -1,
      groupBy: "tag",
      hasToday: false,
      suppressCurrentTimeIndicator: true,
    });

    expect(
      rows.filter((row) => row.kind === "session").map((row) => row.key),
    ).toEqual(["session:work:shared", "session:personal:shared"]);
  });

  it("places today's indicator between future and past sessions", () => {
    const rows = buildTimelineRows({
      buckets: [
        bucket("Today", [
          makeTimelineItem("future", {
            created_at: "2024-01-15T13:00:00.000Z",
          }),
          makeTimelineItem("past", {
            created_at: "2024-01-15T11:00:00.000Z",
          }),
        ]),
      ],
      currentTimeMs: Date.parse("2024-01-15T12:00:00.000Z"),
      fallbackIndicatorIndex: -1,
      groupBy: "date",
      hasToday: true,
      suppressCurrentTimeIndicator: false,
    });

    expect(rows.map((row) => row.key)).toEqual([
      "bucket:Today",
      "session:Today:future",
      "current-time:today",
      "session:Today:past",
    ]);
  });

  it("does not reserve indicator spacing when the fallback is suppressed", () => {
    const rows = buildTimelineRows({
      buckets: [bucket("Yesterday", [])],
      currentTimeMs: Date.UTC(2024, 0, 15),
      fallbackIndicatorIndex: 0,
      groupBy: "date",
      hasToday: false,
      suppressCurrentTimeIndicator: true,
    });
    const indicator = rows[0];

    expect(indicator).toMatchObject({ kind: "current-time", gap: "none" });
    expect(indicator && getTimelineRowHeight(indicator)).toBe(
      TIMELINE_ROW_HEIGHT.currentTime,
    );
  });
});

function bucket(
  label: string,
  items: ReturnType<typeof makeTimelineItem>[],
): TimelineBucket {
  return { id: label, label, items, precision: "time" };
}
