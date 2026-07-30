import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  loadSessionContentSnapshot: vi.fn(),
  enqueueDatabaseWrite: vi.fn((_key: string, write: () => Promise<unknown>) =>
    write(),
  ),
  sessionWriteEnhancedDoc: vi.fn(
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

vi.mock("~/shared/write-queue", () => ({
  enqueueDatabaseWrite: mocks.enqueueDatabaseWrite,
}));

vi.mock("~/session/content-queries", () => ({
  loadSessionContentSnapshot: mocks.loadSessionContentSnapshot,
}));

vi.mock("~/shared/utils", () => ({
  id: () => "new-note",
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionWriteEnhancedDoc: mocks.sessionWriteEnhancedDoc,
    sessionUpdateEnhancedDoc: mocks.sessionUpdateEnhancedDoc,
  },
}));

import {
  ensureSummaryDocument,
  replaceSummaryDocumentTemplate,
  updateSummaryDocumentTitleIfCurrent,
} from "./storage";

function createSnapshot() {
  return {
    sessionId: "session-1",
    ownerUserId: "user-1",
    title: "Planning",
    createdAt: "2026-07-10T00:00:00.000Z",
    event: null,
    eventId: null,
    rawNoteId: "session-1",
    rawContent: "",
    rawContentFormat: "prosemirror_json",
    rawMarkdown: "",
    enhancedNotes: [
      {
        id: "existing-note",
        title: "Summary",
        markdown: "",
        content: "",
        contentFormat: "prosemirror_json",
        templateId: "template-1",
        position: 4,
      },
    ],
    transcripts: [],
    participants: [],
  };
}

describe("enhancer store-backed storage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.loadSessionContentSnapshot.mockResolvedValue(createSnapshot());
    mocks.sessionWriteEnhancedDoc.mockResolvedValue({
      status: "ok",
      data: null,
    });
    mocks.sessionUpdateEnhancedDoc.mockResolvedValue({
      status: "ok",
      data: null,
    });
  });

  it("returns the existing note for the same template", async () => {
    await expect(
      ensureSummaryDocument("session-1", "template-1"),
    ).resolves.toMatchObject({ id: "existing-note" });
    expect(mocks.sessionWriteEnhancedDoc).not.toHaveBeenCalled();
  });

  it("serializes creation through the store with the next stable position", async () => {
    const result = await ensureSummaryDocument("session-1", "template-2");

    expect(result).toMatchObject({
      id: "new-note",
      templateId: "template-2",
      position: 5,
    });
    expect(mocks.enqueueDatabaseWrite).toHaveBeenCalledWith(
      "session:session-1",
      expect.any(Function),
    );
    expect(mocks.sessionWriteEnhancedDoc).toHaveBeenCalledWith({
      id: "new-note",
      session_id: "session-1",
      kind: "template_output",
      title: "Summary",
      template_id: "template-2",
      sort_order: 5,
      markdown: "",
    });
  });

  it("creates a plain summary (kind 'summary') when no template is given", async () => {
    const snapshot = createSnapshot();
    snapshot.enhancedNotes = [];
    mocks.loadSessionContentSnapshot.mockResolvedValue(snapshot);

    await ensureSummaryDocument("session-1");

    expect(mocks.sessionWriteEnhancedDoc).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "summary", template_id: "" }),
    );
  });

  it("does not create a summary for a deleted session", async () => {
    mocks.loadSessionContentSnapshot.mockResolvedValue(null);

    await expect(ensureSummaryDocument("missing")).rejects.toThrow(
      "Session missing no longer exists",
    );
    expect(mocks.sessionWriteEnhancedDoc).not.toHaveBeenCalled();
  });

  it("surfaces a store rejection on create (the meta-existence guard)", async () => {
    mocks.sessionWriteEnhancedDoc.mockResolvedValueOnce({
      status: "error",
      error: "session session-1 has no _meta.json",
    });

    await expect(
      ensureSummaryDocument("session-1", "template-2"),
    ).rejects.toThrow("no _meta.json");
  });

  it("replaces a target summary through one store update that resets the body", async () => {
    await replaceSummaryDocumentTemplate({
      sessionId: "session-1",
      noteId: "existing-note",
      templateId: "template-2",
      title: "Customer review",
    });

    expect(mocks.sessionUpdateEnhancedDoc).toHaveBeenCalledWith(
      "session-1",
      "existing-note",
      {
        kind: "template_output",
        template_id: "template-2",
        title: "Customer review",
        markdown: "",
      },
    );
  });

  it("hydrates a title via a title CAS against the current placeholder", async () => {
    await updateSummaryDocumentTitleIfCurrent({
      sessionId: "session-1",
      noteId: "existing-note",
      currentTitle: "Summary",
      nextTitle: "One-on-one",
    });

    expect(mocks.sessionUpdateEnhancedDoc).toHaveBeenCalledWith(
      "session-1",
      "existing-note",
      {
        title: "One-on-one",
        expected_title: "Summary",
      },
    );
  });

  it("treats a title CAS conflict as a benign no-op, like the old zero-rows update", async () => {
    mocks.sessionUpdateEnhancedDoc.mockResolvedValueOnce({
      status: "error",
      error: "conflict: enhanced doc existing-note title changed",
    });

    await expect(
      updateSummaryDocumentTitleIfCurrent({
        sessionId: "session-1",
        noteId: "existing-note",
        currentTitle: "Summary",
        nextTitle: "One-on-one",
      }),
    ).resolves.toBeUndefined();
  });

  it("still surfaces a non-conflict title update failure", async () => {
    mocks.sessionUpdateEnhancedDoc.mockResolvedValueOnce({
      status: "error",
      error: "I/O error: disk full",
    });

    await expect(
      updateSummaryDocumentTitleIfCurrent({
        sessionId: "session-1",
        noteId: "existing-note",
        currentTitle: "Summary",
        nextTitle: "One-on-one",
      }),
    ).rejects.toThrow("disk full");
  });
});
