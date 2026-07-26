import { beforeEach, describe, expect, it, vi } from "vitest";

import type { TaskRecord, TaskSource } from "@hypr/editor/tasks";

import {
  createStoreBackedTaskStorage,
  type TaskItem,
  type TaskStorageDependencies,
} from "./task-storage";

const sessionSource: TaskSource = { type: "session_raw_note", id: "session-1" };
const task: TaskRecord = {
  taskId: "task-1",
  sourceId: "session-1",
  sourceType: "session_raw_note",
  sourceOrder: 0,
  status: "todo",
  textPreview: "Follow up",
  body: [
    {
      type: "paragraph",
      content: [{ type: "text", text: "Follow up" }],
    },
  ],
  dueDate: "2026-07-12",
};

function taskItem(overrides: Partial<TaskItem> = {}): TaskItem {
  return {
    id: "task-1",
    source_type: "session_raw_note",
    source_id: "session-1",
    source_order: 0,
    status: "todo",
    text: "Follow up",
    body: [
      { type: "paragraph", content: [{ type: "text", text: "Follow up" }] },
    ],
    due_at: "2026-07-12",
    assignee: "",
    created_at: "2026-07-10T10:00:00.000Z",
    updated_at: "2026-07-10T10:00:00.000Z",
    ...overrides,
  };
}

function createHarness(initialTasks: TaskItem[] = []) {
  const listTasks = vi.fn().mockResolvedValue(initialTasks);
  const replaceTasks = vi.fn().mockResolvedValue(undefined);
  const removeTasks = vi.fn().mockResolvedValue(undefined);
  const moveTasks = vi.fn().mockResolvedValue(undefined);
  const enqueueWrite = vi.fn(
    async (_key: string, write: () => Promise<void>) => {
      await write();
    },
  );
  const dependencies: TaskStorageDependencies = {
    listTasks,
    replaceTasks,
    removeTasks,
    moveTasks,
    enqueueWrite,
  };

  return {
    dependencies,
    enqueueWrite,
    listTasks,
    replaceTasks,
    removeTasks,
    moveTasks,
  };
}

describe("store-backed task storage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("publishes stable source snapshots from the initial command fetch", async () => {
    const harness = createHarness([taskItem()]);
    const storage = createStoreBackedTaskStorage(harness.dependencies);
    const listener = vi.fn();

    storage.subscribeSource(sessionSource, listener);
    expect(storage.getTasksForSource(sessionSource)).toEqual([]);
    await vi.waitFor(() => expect(listener).toHaveBeenCalledOnce());

    expect(harness.listTasks).toHaveBeenCalledWith(
      "session_raw_note",
      "session-1",
    );
    const firstSnapshot = storage.getTasksForSource(sessionSource);
    expect(firstSnapshot).toEqual([task]);
    expect(storage.getTask("task-1")).toBe(firstSnapshot[0]);
  });

  it("fetches once per source and keeps the snapshot stable across identical refetches", async () => {
    const harness = createHarness([taskItem()]);
    const storage = createStoreBackedTaskStorage(harness.dependencies);
    const listener = vi.fn();
    const otherListener = vi.fn();

    storage.subscribeSource(sessionSource, listener);
    storage.subscribeSource(sessionSource, otherListener);
    await vi.waitFor(() => expect(listener).toHaveBeenCalledOnce());
    expect(harness.listTasks).toHaveBeenCalledOnce();
    const firstSnapshot = storage.getTasksForSource(sessionSource);

    // an own write refetches; identical data must not re-notify or swap the snapshot
    storage.removeTasksForSource(sessionSource, ["task-ghost"]);
    await vi.waitFor(() => expect(harness.listTasks).toHaveBeenCalledTimes(2));
    expect(storage.getTasksForSource(sessionSource)).toBe(firstSnapshot);
    expect(listener).toHaveBeenCalledOnce();
  });

  it("refetches after an own write and notifies on changed data", async () => {
    const harness = createHarness([taskItem()]);
    const storage = createStoreBackedTaskStorage(harness.dependencies);
    const listener = vi.fn();

    storage.subscribeSource(sessionSource, listener);
    await vi.waitFor(() => expect(listener).toHaveBeenCalledOnce());

    harness.listTasks.mockResolvedValue([
      taskItem({ text: "Updated follow up" }),
    ]);
    storage.upsertTasksForSource(sessionSource, [
      { ...task, textPreview: "Updated follow up" },
    ]);

    await vi.waitFor(() => expect(listener).toHaveBeenCalledTimes(2));
    expect(harness.replaceTasks).toHaveBeenCalledOnce();
    expect(storage.getTask("task-1")?.textPreview).toBe("Updated follow up");
  });

  it("replaces the source's task list through the serialized write queue", async () => {
    const harness = createHarness();
    const storage = createStoreBackedTaskStorage(harness.dependencies);

    storage.upsertTasksForSource(sessionSource, [task]);

    await vi.waitFor(() => expect(harness.replaceTasks).toHaveBeenCalledOnce());
    expect(harness.enqueueWrite).toHaveBeenCalledWith(
      "tasks",
      expect.any(Function),
    );
    expect(harness.replaceTasks).toHaveBeenCalledWith(
      "session_raw_note",
      "session-1",
      [
        {
          id: "task-1",
          source_order: 0,
          status: "todo",
          text: "Follow up",
          body: [
            {
              type: "paragraph",
              content: [{ type: "text", text: "Follow up" }],
            },
          ],
          due_at: "2026-07-12",
        },
      ],
    );
  });

  it("skips writes when the committed source snapshot is unchanged", async () => {
    const harness = createHarness([taskItem()]);
    const storage = createStoreBackedTaskStorage(harness.dependencies);
    const listener = vi.fn();
    storage.subscribeSource(sessionSource, listener);
    await vi.waitFor(() => expect(listener).toHaveBeenCalledOnce());

    storage.upsertTasksForSource(sessionSource, [task]);

    expect(harness.enqueueWrite).not.toHaveBeenCalled();
    expect(harness.replaceTasks).not.toHaveBeenCalled();
  });

  it("scopes removals to the source and moves tasks in one batch", async () => {
    const harness = createHarness();
    const storage = createStoreBackedTaskStorage(harness.dependencies);

    storage.removeTasksForSource(sessionSource, ["task-1"]);
    storage.moveTasksToSource(
      ["task-1", "task-2"],
      { type: "enhanced_note", id: "note-1" },
      4,
    );

    await vi.waitFor(() => expect(harness.moveTasks).toHaveBeenCalledOnce());
    expect(harness.removeTasks).toHaveBeenCalledWith(
      "session_raw_note",
      "session-1",
      ["task-1"],
    );
    expect(harness.moveTasks).toHaveBeenCalledWith(
      ["task-1", "task-2"],
      "enhanced_note",
      "note-1",
      4,
    );
  });

  it("refetches the moved tasks' previous sources after a move", async () => {
    const harness = createHarness([taskItem()]);
    const storage = createStoreBackedTaskStorage(harness.dependencies);
    const listener = vi.fn();
    storage.subscribeSource(sessionSource, listener);
    await vi.waitFor(() => expect(listener).toHaveBeenCalledOnce());
    harness.listTasks.mockClear();

    storage.moveTasksToSource(
      ["task-1"],
      { type: "enhanced_note", id: "note-1" },
      0,
    );

    await vi.waitFor(() => expect(harness.listTasks).toHaveBeenCalledTimes(2));
    const fetched = harness.listTasks.mock.calls.map((call) => call.join(":"));
    expect(fetched).toContain("enhanced_note:note-1");
    expect(fetched).toContain("session_raw_note:session-1");
  });

  it("drops malformed items instead of exposing invalid task records", async () => {
    const harness = createHarness([taskItem({ id: "", status: "unknown" })]);
    const storage = createStoreBackedTaskStorage(harness.dependencies);
    const listener = vi.fn();
    storage.subscribeSource(sessionSource, listener);

    await vi.waitFor(() => expect(harness.listTasks).toHaveBeenCalledOnce());
    await Promise.resolve();
    expect(storage.getTasksForSource(sessionSource)).toEqual([]);
    expect(storage.getTask("task-1")).toBeNull();
    expect(listener).not.toHaveBeenCalled();
  });
});
