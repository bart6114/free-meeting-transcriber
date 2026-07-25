import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  execute: vi.fn(),
  subscribe: vi.fn(),
}));

vi.mock("~/db", () => ({
  liveQueryClient: {
    execute: mocks.execute,
    subscribe: mocks.subscribe,
  },
}));

import {
  createMeetingFloatLabelContext,
  loadMeetingFloatData,
  subscribeMeetingFloatData,
} from "./hooks";

const rows = [
  {
    session_id: "session-1",
    title: "Planning",
    owner_user_id: "human-self",
  },
] as const;

describe("meeting float SQLite data", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads titles and speaker identity from one canonical snapshot", async () => {
    mocks.execute.mockResolvedValue(rows);

    const data = await loadMeetingFloatData();
    const labels = createMeetingFloatLabelContext(data, "session-1");

    expect(data.sessions["session-1"]).toEqual({
      title: "Planning",
      ownerUserId: "human-self",
    });
    expect(labels.getSelfHumanId()).toBe("human-self");
    expect(labels.getParticipantHumanIds?.()).toEqual([]);
    expect(labels.getHumanName("Remote speaker")).toBe("Remote speaker");
    expect(mocks.execute.mock.calls[0][0]).not.toContain(
      "session_participants",
    );
    expect(mocks.execute.mock.calls[0][0]).not.toContain("humans");
  });

  it("maps live query updates through the same snapshot shape", async () => {
    const unsubscribe = vi.fn().mockResolvedValue(undefined);
    mocks.subscribe.mockImplementation(async (_sql, _params, handlers) => {
      handlers.onData(rows);
      return unsubscribe;
    });
    const onData = vi.fn();

    await expect(subscribeMeetingFloatData(onData, vi.fn())).resolves.toBe(
      unsubscribe,
    );
    expect(onData).toHaveBeenCalledWith(
      expect.objectContaining({
        sessions: expect.objectContaining({
          "session-1": expect.objectContaining({ title: "Planning" }),
        }),
      }),
    );
  });
});
