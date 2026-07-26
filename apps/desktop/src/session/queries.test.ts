import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  analyticsEventFireAndForget: vi.fn(() => Promise.resolve()),
  execute: vi.fn(),
  executeTransaction: vi.fn(
    (_statements: Array<{ sql: string; params: unknown[] }>) =>
      Promise.resolve([1]),
  ),
  sessionWriteMeta: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  sessionUpdateMeta: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  sessionWriteNote: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  sessionDelete: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  sessionRestore: vi.fn(
    (): Promise<
      { status: "ok"; data: boolean } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: true }),
  ),
  waitForPendingSoftDelete: vi.fn(() => Promise.resolve()),
}));

vi.mock("@hypr/plugin-analytics", () => ({
  commands: { eventFireAndForget: mocks.analyticsEventFireAndForget },
}));

vi.mock("@hypr/plugin-fs-sync", () => ({
  commands: {
    deleteSessionFolder: vi.fn(() =>
      Promise.resolve({ status: "ok", data: null }),
    ),
  },
}));

vi.mock("~/db", () => ({
  executeTransaction: mocks.executeTransaction,
  liveQueryClient: { execute: mocks.execute },
}));

vi.mock("~/session/pending-soft-deletes", () => ({
  waitForPendingSoftDelete: mocks.waitForPendingSoftDelete,
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionWriteMeta: mocks.sessionWriteMeta,
    sessionUpdateMeta: mocks.sessionUpdateMeta,
    sessionWriteNote: mocks.sessionWriteNote,
    sessionDelete: mocks.sessionDelete,
    sessionRestore: mocks.sessionRestore,
  },
}));

import {
  createSession,
  deleteEnhancedNote,
  isSessionEmpty,
  restoreDeletedSession,
  softDeleteSession,
  updateEnhancedNoteContent,
  updateSession,
} from "./queries";

describe("session SQLite operations", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useRealTimers();
    mocks.sessionWriteMeta.mockResolvedValue({ status: "ok", data: null });
    mocks.sessionUpdateMeta.mockResolvedValue({ status: "ok", data: null });
    mocks.sessionWriteNote.mockResolvedValue({ status: "ok", data: null });
    mocks.sessionDelete.mockResolvedValue({ status: "ok", data: null });
    mocks.sessionRestore.mockResolvedValue({ status: "ok", data: true });
    mocks.waitForPendingSoftDelete.mockResolvedValue(undefined);
  });

  it("routes title changes through the store's meta patch, never raw SQL", async () => {
    await updateSession("session-1", { title: "Updated title" });

    expect(mocks.sessionUpdateMeta).toHaveBeenCalledWith("session-1", {
      title: "Updated title",
    });
    // No `UPDATE sessions` here: the store's dual-write owns the SQL mirror, and a raw
    // UPDATE would leave `_meta.json` stale (the next rebuild would revert the title).
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("maps folder/event changes onto the store patch shape", async () => {
    await updateSession("session-1", {
      folder_id: "work",
      event_json: '{"tracking_id":"evt-1"}',
    });

    expect(mocks.sessionUpdateMeta).toHaveBeenCalledWith("session-1", {
      folder: "work",
      event: { tracking_id: "evt-1" },
    });
  });

  it("throws when the store rejects the meta patch", async () => {
    mocks.sessionUpdateMeta.mockResolvedValueOnce({
      status: "error",
      error: "no _meta.json",
    });

    await expect(
      updateSession("session-1", { title: "Updated title" }),
    ).rejects.toThrow("no _meta.json");
  });

  it("is a no-op when there is nothing to change", async () => {
    await updateSession("session-1", {});
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
  });

  it("creates a session via the store, then seeds initial content as markdown", async () => {
    await createSession("Welcome", "user-1", {
      event_json: '{"tracking_id":"welcome"}',
      raw_md: JSON.stringify({
        type: "doc",
        content: [
          { type: "paragraph", content: [{ type: "text", text: "Hi" }] },
        ],
      }),
    });

    // The event rides the store write itself, parsed into the meta envelope -- never a
    // separate `UPDATE sessions SET event_json` statement.
    expect(mocks.sessionWriteMeta).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Welcome",
        event: { tracking_id: "welcome" },
      }),
    );

    // Bookkeeping placeholder (always-empty body) only.
    const statements = mocks.executeTransaction.mock.calls[0][0] as Array<{
      sql: string;
      params: unknown[];
    }>;
    expect(statements).toHaveLength(1);
    expect(statements[0].sql).toContain("session_documents");
    expect(statements[0].params).toContain("");

    // Real content lands in the file-canonical store as markdown, not SQL.
    expect(mocks.sessionWriteNote).toHaveBeenCalledWith(
      expect.any(String),
      "Hi",
    );
  });

  it("creates a session with no initial content and does not touch the note store", async () => {
    await createSession("Untitled");

    expect(mocks.sessionWriteMeta).toHaveBeenCalled();
    expect(mocks.sessionWriteNote).not.toHaveBeenCalled();
  });

  it("throws when the store fails to create the session", async () => {
    mocks.sessionWriteMeta.mockResolvedValueOnce({
      status: "error",
      error: "disk full",
    });

    await expect(createSession("Untitled")).rejects.toThrow("disk full");
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("commits enhanced note content via SQL and the derived session title via the store", async () => {
    mocks.executeTransaction.mockResolvedValueOnce([1]);

    await updateEnhancedNoteContent(
      "enhanced-note-1",
      "session-1",
      '{"type":"doc"}',
      "Edited title",
    );

    const statements = mocks.executeTransaction.mock.calls[0][0] as Array<{
      sql: string;
      params: unknown[];
    }>;
    expect(statements).toHaveLength(1);
    expect(statements[0].sql).toContain("UPDATE session_documents");
    expect(statements[0].params).toContain("enhanced-note-1");
    expect(statements[0].params).toContain('{"type":"doc"}');
    expect(mocks.sessionUpdateMeta).toHaveBeenCalledWith("session-1", {
      title: "Edited title",
    });
  });

  it("does not touch session meta when no derived title accompanies the note content", async () => {
    mocks.executeTransaction.mockResolvedValueOnce([1]);

    await updateEnhancedNoteContent(
      "enhanced-note-1",
      "session-1",
      '{"type":"doc"}',
    );

    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
  });

  it("soft-deletes an enhanced note instead of removing its data", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-10T12:00:00.000Z"));
    mocks.executeTransaction.mockResolvedValueOnce([1]);

    await deleteEnhancedNote("enhanced-note-1");

    const statements = mocks.executeTransaction.mock.calls[0][0] as Array<{
      sql: string;
      params: unknown[];
    }>;
    expect(statements).toHaveLength(1);
    expect(statements[0].sql).toContain("UPDATE session_documents");
    expect(statements[0].sql).toContain("deleted_at IS NULL");
    expect(statements[0].sql).not.toContain("DELETE FROM");
    expect(statements[0].params).toEqual([
      "2026-07-10T12:00:00.000Z",
      "2026-07-10T12:00:00.000Z",
      "enhanced-note-1",
    ]);
  });

  it("deletes the session through the store, capturing its title first for the undo toast", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-10T12:00:00.000Z"));
    mocks.execute.mockResolvedValueOnce([
      { id: "session-1", title: "Planning" },
    ]);

    const deleted = await softDeleteSession("session-1");

    expect(deleted).toEqual({
      session: { id: "session-1", title: "Planning" },
      tombstone: "2026-07-10T12:00:00.000Z",
      deletedAt: Date.parse("2026-07-10T12:00:00.000Z"),
    });
    expect(mocks.sessionDelete).toHaveBeenCalledWith("session-1");
  });

  it("does not call session_delete when the session no longer exists", async () => {
    mocks.execute.mockResolvedValueOnce([]);

    await expect(softDeleteSession("session-1")).resolves.toBeNull();
    expect(mocks.sessionDelete).not.toHaveBeenCalled();
  });

  it("throws (does not swallow) a genuine session_delete command failure", async () => {
    mocks.execute.mockResolvedValueOnce([
      { id: "session-1", title: "Planning" },
    ]);
    mocks.sessionDelete.mockResolvedValueOnce({
      status: "error",
      error: "boom",
    });

    // Must reject, not resolve to null -- useDeleteSession distinguishes "already deleted"
    // (benign null) from a real failure (must surface to its catch block's error toast) purely
    // by whether the promise rejects.
    await expect(softDeleteSession("session-1")).rejects.toThrow("boom");
  });

  it("recognizes a blank SQLite session", async () => {
    mocks.execute.mockResolvedValueOnce([
      {
        title: "",
        event_json: "",
        note_body: JSON.stringify({
          type: "doc",
          content: [{ type: "paragraph" }],
        }),
        note_body_format: "prosemirror_json",
        transcript_count: 0,
        enhanced_note_count: 0,
        tag_count: 0,
      },
    ]);

    await expect(isSessionEmpty("session-1")).resolves.toBe(true);
  });

  it.each([
    ["title", { title: "Named note", event_json: "" }],
    ["note body", { note_body: "Written content" }],
    ["transcript", { transcript_count: 1 }],
    ["enhanced note", { enhanced_note_count: 1 }],
    ["tag", { tag_count: 1 }],
  ])("keeps a session with %s data", async (_label, overrides) => {
    mocks.execute.mockResolvedValueOnce([
      {
        title: "",
        event_json: "event",
        note_body: "",
        note_body_format: "prosemirror_json",
        transcript_count: 0,
        enhanced_note_count: 0,
        tag_count: 0,
        ...overrides,
      },
    ]);

    await expect(isSessionEmpty("session-1")).resolves.toBe(false);
  });

  it("waits for a pending delete to settle, then restores through the store", async () => {
    await restoreDeletedSession({
      session: { id: "session-1", title: "Planning" },
      tombstone: "2026-07-10T12:00:00.000Z",
      deletedAt: 1,
    });

    expect(mocks.waitForPendingSoftDelete).toHaveBeenCalledWith("session-1");
    expect(mocks.sessionRestore).toHaveBeenCalledWith("session-1");
  });

  it("throws when nothing was trashed to restore", async () => {
    mocks.sessionRestore.mockResolvedValueOnce({ status: "ok", data: false });

    await expect(
      restoreDeletedSession({
        session: { id: "session-1", title: "Planning" },
        tombstone: "2026-07-10T12:00:00.000Z",
        deletedAt: 1,
      }),
    ).rejects.toThrow("was never soft-deleted");
  });

  it("throws when the store restore call fails", async () => {
    mocks.sessionRestore.mockResolvedValueOnce({
      status: "error",
      error: "boom",
    });

    await expect(
      restoreDeletedSession({
        session: { id: "session-1", title: "Planning" },
        tombstone: "2026-07-10T12:00:00.000Z",
        deletedAt: 1,
      }),
    ).rejects.toThrow("boom");
  });
});
