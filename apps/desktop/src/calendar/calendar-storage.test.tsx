import { beforeEach, describe, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execute: vi.fn(),
  executeTransaction: vi.fn(),
  liveRows: [] as Array<Record<string, unknown>>,
  liveQueryOptions: null as null | {
    sql: string;
    params?: unknown[];
    mapRows: (rows: Array<Record<string, unknown>>) => unknown;
  },
}));

vi.mock("~/db", () => ({
  executeTransaction: mocks.executeTransaction,
  liveQueryClient: { execute: mocks.execute },
  useLiveQuery: (options: {
    sql: string;
    params?: unknown[];
    mapRows: (rows: Array<Record<string, unknown>>) => unknown;
  }) => {
    mocks.liveQueryOptions = options;
    return { data: options.mapRows(mocks.liveRows) };
  },
}));

vi.mock("~/db/write-queue", () => ({
  enqueueDatabaseWrite: (
    _key: string,
    write: () => Promise<unknown>,
  ): Promise<unknown> => write(),
}));

import {
  getCalendarEventStartedAt,
  getNearbyCalendarEvents,
} from "./queries";

describe("calendar SQLite selection", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.liveRows = [];
    mocks.liveQueryOptions = null;
    mocks.execute.mockResolvedValue([]);
    mocks.executeTransaction.mockResolvedValue([]);
  });

  test("reads an event start time from SQLite", async () => {
    mocks.execute.mockResolvedValue([
      { started_at: "2026-07-10T09:00:00.000Z" },
    ]);

    await expect(getCalendarEventStartedAt("event-1")).resolves.toBe(
      "2026-07-10T09:00:00.000Z",
    );
    expect(mocks.execute.mock.calls[0][1]).toEqual(["event-1"]);
  });

  test("returns nearby event participant names without the current user", async () => {
    mocks.execute.mockResolvedValue([
      {
        id: "event-1",
        title: "Planning",
        started_at: "2026-07-10T09:00:00.000Z",
        meeting_link: "https://meet.example.com/planning",
        location: "Room 1",
        description: "Weekly plan",
        participants_json: JSON.stringify([
          { name: "Alice", is_current_user: false },
          { name: "John", is_current_user: true },
          { name: "Alice", is_current_user: false },
        ]),
      },
    ]);

    await expect(getNearbyCalendarEvents(1000, 500)).resolves.toEqual([
      {
        id: "event-1",
        title: "Planning",
        meetingLink: "https://meet.example.com/planning",
        location: "Room 1",
        description: "Weekly plan",
        participantNames: ["Alice"],
      },
    ]);
    expect(mocks.execute.mock.calls[0][1]).toEqual([1000, 500, 1000]);
  });
});
