import { describe, expect, it } from "vitest";

import {
  buildPastSessionNotes,
  buildSessionKeyFactsStatements,
  type PastSessionNotesData,
} from "./past-notes";

describe("buildPastSessionNotes", () => {
  it("builds descending past notes from recurring and same-title sessions", () => {
    const data = makeData({
      sessions: {
        current: {
          title: "Weekly Product Sync",
          created_at: "2026-06-03T10:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2026-06-03T10:00:00.000Z",
            recurrence_series_id: "series-1",
          }),
          raw_md: "",
        },
        previous: {
          title: "Weekly Product Sync",
          created_at: "2026-05-28T10:00:00.000Z",
          event_json: JSON.stringify({
            started_at: "2026-05-28T10:00:00.000Z",
            recurrence_series_id: "series-1",
          }),
          raw_md: "",
        },
        same_title: {
          title: "Weekly Product Sync",
          created_at: "2026-05-27T10:00:00.000Z",
          event_json: "",
          raw_md: "Raw note text should not feed insights.",
        },
        older: {
          title: "Older Product Sync",
          created_at: "2026-05-21T10:00:00.000Z",
          event_json: "",
          raw_md: "Reviewed onboarding follow-ups and assigned owners.",
        },
        future: {
          title: "Future Product Sync",
          created_at: "2026-06-10T10:00:00.000Z",
          event_json: "",
          raw_md: "Should not show up.",
        },
      },
      enhanced_notes: {
        previous_summary: {
          session_id: "previous",
          content:
            "Aligned on transcript panel behavior. Past notes should stay short and scannable.",
          position: 0,
        },
        same_title_summary: {
          session_id: "same_title",
          content: "Confirmed notification copy and reviewed follow-ups.",
          position: 0,
        },
      },
    });

    const result = buildPastSessionNotes(data, "current", "self");

    expect(result.notes).toEqual([
      {
        sessionId: "previous",
        title: "Weekly Product Sync",
        dateLabel: "May 28, 2026",
        participantNames: [],
        summary: null,
        isGenerating: false,
      },
      {
        sessionId: "same_title",
        title: "Weekly Product Sync",
        dateLabel: "May 27, 2026",
        participantNames: [],
        summary: null,
        isGenerating: false,
      },
    ]);
    expect(result.missing.map((request) => request.sessionId)).toEqual([
      "previous",
      "same_title",
    ]);
    expect(result.requests.map((request) => request.sourceText)).toEqual([
      "Aligned on transcript panel behavior. Past notes should stay short and scannable.",
      "Confirmed notification copy and reviewed follow-ups.",
    ]);
  });

  it("does not treat sessions with a different title or series as related", () => {
    const data = makeData({
      sessions: {
        current: {
          title: "Design sync",
          created_at: "2026-06-03T10:00:00.000Z",
          event_json: "",
          raw_md: "",
        },
        different_topic: {
          title: "Dev sync",
          created_at: "2026-06-01T10:00:00.000Z",
          event_json: "",
          raw_md: "Discussed release branch status.",
        },
      },
    });

    const result = buildPastSessionNotes(data, "current", "self");

    expect(result.notes).toEqual([]);
    expect(result.missing).toEqual([]);
  });
});

describe("buildSessionKeyFactsStatements", () => {
  it("copies workspace ownership from the parent session", () => {
    const statements = buildSessionKeyFactsStatements(
      [
        {
          sessionId: "session-1",
          userId: "user-1",
          content: "One fact",
          sourceHash: "hash-1",
        },
      ],
      "2026-07-13T00:00:00.000Z",
    );

    expect(statements[1]?.sql).toContain("session.workspace_id");
    expect(statements[1]?.sql).toContain("FROM sessions AS session");
    expect(statements[1]?.params).toContain("session-1");
  });
});

function makeData(
  tables: Record<string, Record<string, Record<string, unknown>>>,
): PastSessionNotesData {
  return {
    sessions: Object.fromEntries(
      Object.entries(tables.sessions ?? {}).map(([id, row]) => [
        id,
        {
          id,
          user_id: String(row.user_id ?? "self"),
          title: String(row.title ?? ""),
          created_at: String(row.created_at ?? ""),
          event_json: String(row.event_json ?? ""),
        },
      ]),
    ),
    enhancedNotes: Object.values(tables.enhanced_notes ?? {}).map((row) => ({
      session_id: String(row.session_id ?? ""),
      content: String(row.content ?? ""),
      position: Number(row.position ?? 0),
    })),
    keyFacts: Object.fromEntries(
      Object.values(tables.session_key_facts ?? {}).map((row) => [
        String(row.session_id ?? ""),
        {
          session_id: String(row.session_id ?? ""),
          user_id: String(row.user_id ?? ""),
          created_at: String(row.created_at ?? ""),
          updated_at: String(row.updated_at ?? ""),
          content: String(row.content ?? ""),
          source_hash: String(row.source_hash ?? ""),
        },
      ]),
    ),
  };
}
