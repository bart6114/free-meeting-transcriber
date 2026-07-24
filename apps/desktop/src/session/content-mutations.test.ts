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
}));

vi.mock("~/db", () => ({
  executeTransaction: mocks.executeTransaction,
}));

import {
  applyGeneratedSessionTitle,
  persistGeneratedEnhancedNote,
} from "./content-mutations";

describe("session content SQLite corrections", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("saves generated content and deterministic tag rows atomically", async () => {
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

  it("rolls back a generated title when any enhanced-note document guard is stale", async () => {
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
    expect(statements).toHaveLength(2);
    expect(statements[0].sql).toContain("AND title = ?");
    expect(statements[0]).toMatchObject({ expectedRowsAffected: 1 });
    expect(statements[1].sql).toContain("AND body = ?");
    expect(statements[1]).toMatchObject({ expectedRowsAffected: 1 });
    // The raw note is stamped separately, file-first (title-success.ts's
    // applyGeneratedNoteTitle) -- this SQL path must never target it.
    expect(statements[1].sql).not.toContain("'note'");
    expect(statements[1].sql).toContain(
      "kind IN ('summary', 'template_output')",
    );
  });
});
