import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  executeTransaction: vi.fn(
    (
      _statements: Array<{
        sql: string;
        params: unknown[];
        expectedRowsAffected: number;
      }>,
    ) => Promise.resolve([1, 1]),
  ),
  execute: vi.fn((_sql: string, _params: unknown[]) =>
    Promise.resolve([] as unknown[]),
  ),
  sessionUpdateMeta: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
}));

vi.mock("~/db", () => ({
  executeTransaction: mocks.executeTransaction,
  liveQueryClient: { execute: mocks.execute },
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionUpdateMeta: mocks.sessionUpdateMeta,
  },
}));

import {
  applyGeneratedSessionTitle,
  persistGeneratedEnhancedNote,
} from "./content-mutations";

describe("session content SQLite corrections", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.sessionUpdateMeta.mockResolvedValue({ status: "ok", data: null });
  });

  it("saves generated content and deterministic tag rows atomically", async () => {
    mocks.execute.mockResolvedValueOnce([
      { tag_id: "launch" },
      { tag_id: "prep" },
    ]);

    await persistGeneratedEnhancedNote({
      sessionId: "session-1",
      ownerUserId: "user-1",
      note: {
        id: "summary-1",
        currentContent: "old summary",
        currentContentFormat: "markdown",
        nextContent: '{"type":"doc"}',
      },
      tagNames: ["launch", "launch", "prep"],
    });

    const statements = mocks.executeTransaction.mock.calls[0][0];
    expect(statements).toHaveLength(5);
    expect(statements[0]).toMatchObject({ expectedRowsAffected: 1 });
    expect(statements[0].sql).toContain("AND body = ?");
    expect(statements[0].sql).toContain("EXISTS");
    expect(statements[1].sql).toContain("INSERT INTO tags");
    expect(statements[1].params[0]).toBe("launch");
    expect(statements[2].sql).toContain("INSERT INTO session_tags");
    expect(statements[2].params[0]).toBe("session-1:launch");
    expect(
      statements.every((statement) => statement.expectedRowsAffected === 1),
    ).toBe(true);
  });

  it("dual-writes the full SQL tag set into session meta after the tag upserts", async () => {
    // The read-back happens after the transaction, so it reflects pre-existing tags plus
    // the newly upserted ones -- the meta patch must carry the full set, not just the delta.
    mocks.execute.mockResolvedValueOnce([
      { tag_id: "existing" },
      { tag_id: "launch" },
      { tag_id: "prep" },
    ]);

    await persistGeneratedEnhancedNote({
      sessionId: "session-1",
      ownerUserId: "user-1",
      note: {
        id: "summary-1",
        currentContent: "old summary",
        currentContentFormat: "markdown",
        nextContent: '{"type":"doc"}',
      },
      tagNames: ["launch", "prep"],
    });

    expect(mocks.execute).toHaveBeenCalledWith(
      expect.stringContaining("FROM session_tags"),
      ["session-1"],
    );
    expect(mocks.sessionUpdateMeta).toHaveBeenCalledWith("session-1", {
      tags: ["existing", "launch", "prep"],
    });
  });

  it("does not fail the enhanced-note persist when the meta tag write fails", async () => {
    mocks.execute.mockResolvedValueOnce([{ tag_id: "launch" }]);
    mocks.sessionUpdateMeta.mockResolvedValueOnce({
      status: "error",
      error: "no _meta.json",
    });

    await expect(
      persistGeneratedEnhancedNote({
        sessionId: "session-1",
        ownerUserId: "user-1",
        note: {
          id: "summary-1",
          currentContent: "old summary",
          currentContentFormat: "markdown",
          nextContent: '{"type":"doc"}',
        },
        tagNames: ["launch"],
      }),
    ).resolves.toBeUndefined();
  });

  it("skips the meta tag write entirely when generation produced no tags", async () => {
    await persistGeneratedEnhancedNote({
      sessionId: "session-1",
      ownerUserId: "user-1",
      note: {
        id: "summary-1",
        currentContent: "old summary",
        currentContentFormat: "markdown",
        nextContent: '{"type":"doc"}',
      },
      tagNames: [],
    });

    expect(mocks.execute).not.toHaveBeenCalled();
    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
  });

  it("applies a generated title through the store after the document guards pass", async () => {
    mocks.execute.mockResolvedValueOnce([{ title: "" }]);

    await applyGeneratedSessionTitle({
      sessionId: "session-1",
      currentTitle: "",
      nextTitle: "Planning",
      documents: [
        {
          id: "summary-1",
          currentContent: "old summary",
          currentContentFormat: "markdown",
          nextContent: '{"type":"doc"}',
        },
      ],
    });

    const statements = mocks.executeTransaction.mock.calls[0][0];
    expect(statements).toHaveLength(1);
    expect(statements[0].sql).toContain("AND body = ?");
    expect(statements[0]).toMatchObject({ expectedRowsAffected: 1 });
    // The raw note is stamped separately, file-first (title-success.ts's
    // applyGeneratedNoteTitle) -- this SQL path must never target it.
    expect(statements[0].sql).not.toContain("'note'");
    expect(statements[0].sql).toContain(
      "kind IN ('summary', 'template_output')",
    );
    // The title itself is store-canonical, never a raw `UPDATE sessions`.
    expect(mocks.sessionUpdateMeta).toHaveBeenCalledWith("session-1", {
      title: "Planning",
    });
  });

  it("applies nothing when the session title changed while generating", async () => {
    mocks.execute.mockResolvedValueOnce([{ title: "User renamed this" }]);

    await expect(
      applyGeneratedSessionTitle({
        sessionId: "session-1",
        currentTitle: "",
        nextTitle: "Planning",
        documents: [],
      }),
    ).rejects.toThrow("title changed while generating");

    expect(mocks.executeTransaction).not.toHaveBeenCalled();
    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
  });

  it("rolls back the generated title when any enhanced-note document guard is stale", async () => {
    mocks.execute.mockResolvedValueOnce([{ title: "" }]);
    mocks.executeTransaction.mockRejectedValueOnce(
      new Error("expected 1 row affected"),
    );

    await expect(
      applyGeneratedSessionTitle({
        sessionId: "session-1",
        currentTitle: "",
        nextTitle: "Planning",
        documents: [
          {
            id: "summary-1",
            currentContent: "old summary",
            currentContentFormat: "markdown",
            nextContent: '{"type":"doc"}',
          },
        ],
      }),
    ).rejects.toThrow("expected 1 row affected");

    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
  });
});
