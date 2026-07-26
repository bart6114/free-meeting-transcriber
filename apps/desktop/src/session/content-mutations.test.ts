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
  sessionUpdateEnhancedDoc: vi.fn(
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
    sessionUpdateEnhancedDoc: mocks.sessionUpdateEnhancedDoc,
  },
}));

import {
  applyGeneratedSessionTitle,
  persistGeneratedEnhancedNote,
} from "./content-mutations";

describe("session content corrections", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.sessionUpdateMeta.mockResolvedValue({ status: "ok", data: null });
    mocks.sessionUpdateEnhancedDoc.mockResolvedValue({
      status: "ok",
      data: null,
    });
  });

  it("saves generated content through the store CAS, then deterministic tag rows", async () => {
    mocks.execute.mockResolvedValueOnce([
      { tag_id: "launch" },
      { tag_id: "prep" },
    ]);

    await persistGeneratedEnhancedNote({
      sessionId: "session-1",
      ownerUserId: "user-1",
      note: {
        id: "summary-1",
        currentMarkdown: "old summary",
        nextMarkdown: "# New summary",
      },
      tagNames: ["launch", "launch", "prep"],
    });

    // The doc body goes file-first through the store, guarded by the file's current
    // markdown -- never a raw session_documents UPDATE.
    expect(mocks.sessionUpdateEnhancedDoc).toHaveBeenCalledWith(
      "session-1",
      "summary-1",
      {
        markdown: "# New summary",
        expected_markdown: "old summary",
      },
    );

    const statements = mocks.executeTransaction.mock.calls[0][0];
    expect(statements).toHaveLength(4);
    expect(statements[0].sql).toContain("INSERT INTO tags");
    expect(statements[0].params[0]).toBe("launch");
    expect(statements[1].sql).toContain("INSERT INTO session_tags");
    expect(statements[1].params[0]).toBe("session-1:launch");
    expect(
      statements.every((statement) => statement.expectedRowsAffected === 1),
    ).toBe(true);
  });

  it("rejects (and skips the tag writes) when the store CAS finds a stale summary", async () => {
    mocks.sessionUpdateEnhancedDoc.mockResolvedValueOnce({
      status: "error",
      error: "conflict: enhanced doc summary-1 body changed since it was read",
    });

    await expect(
      persistGeneratedEnhancedNote({
        sessionId: "session-1",
        ownerUserId: "user-1",
        note: {
          id: "summary-1",
          currentMarkdown: "stale summary",
          nextMarkdown: "# New summary",
        },
        tagNames: ["launch"],
      }),
    ).rejects.toThrow("conflict");
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
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
        currentMarkdown: "old summary",
        nextMarkdown: "# New summary",
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
          currentMarkdown: "old summary",
          nextMarkdown: "# New summary",
        },
        tagNames: ["launch"],
      }),
    ).resolves.toBeUndefined();
  });

  it("skips the tag transaction and meta tag write entirely when generation produced no tags", async () => {
    await persistGeneratedEnhancedNote({
      sessionId: "session-1",
      ownerUserId: "user-1",
      note: {
        id: "summary-1",
        currentMarkdown: "old summary",
        nextMarkdown: "# New summary",
      },
      tagNames: [],
    });

    expect(mocks.executeTransaction).not.toHaveBeenCalled();
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
          currentMarkdown: "old summary",
          nextMarkdown: "# Planning\n\nold summary",
        },
      ],
    });

    // Each summary is stamped file-first through the store's markdown CAS -- never raw
    // session_documents SQL, and never the raw note (which title-success stamps
    // separately through session_read_note/session_write_note).
    expect(mocks.sessionUpdateEnhancedDoc).toHaveBeenCalledWith(
      "session-1",
      "summary-1",
      {
        markdown: "# Planning\n\nold summary",
        expected_markdown: "old summary",
      },
    );
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
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

    expect(mocks.sessionUpdateEnhancedDoc).not.toHaveBeenCalled();
    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
  });

  it("rolls back the generated title when any enhanced-note document guard is stale", async () => {
    mocks.execute.mockResolvedValueOnce([{ title: "" }]);
    mocks.sessionUpdateEnhancedDoc.mockResolvedValueOnce({
      status: "error",
      error: "conflict: enhanced doc summary-1 body changed since it was read",
    });

    await expect(
      applyGeneratedSessionTitle({
        sessionId: "session-1",
        currentTitle: "",
        nextTitle: "Planning",
        documents: [
          {
            id: "summary-1",
            currentMarkdown: "old summary",
            nextMarkdown: "# Planning\n\nold summary",
          },
        ],
      }),
    ).rejects.toThrow("conflict");

    expect(mocks.sessionUpdateMeta).not.toHaveBeenCalled();
  });
});
