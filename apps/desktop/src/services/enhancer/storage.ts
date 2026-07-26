import {
  loadSessionContentSnapshot,
  type SessionContentSnapshot,
} from "~/session/content-queries";
import { isStoreConflictError } from "~/session/store-errors";
import { id } from "~/shared/utils";
import { enqueueDatabaseWrite } from "~/shared/write-queue";
import { commands } from "~/types/tauri.gen";

export type EnhancerNote = SessionContentSnapshot["enhancedNotes"][number];

export function ensureSummaryDocument(
  sessionId: string,
  templateId?: string,
): Promise<EnhancerNote> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    const snapshot = await loadSessionContentSnapshot(sessionId);
    if (!snapshot) {
      throw new Error(`Session ${sessionId} no longer exists`);
    }

    const normalizedTemplateId = templateId ?? "";
    const existing = snapshot.enhancedNotes.find(
      (note) => note.templateId === normalizedTemplateId,
    );
    if (existing) {
      return existing;
    }

    const noteId = id();
    const position =
      snapshot.enhancedNotes.reduce(
        (highest, note) => Math.max(highest, note.position),
        0,
      ) + 1;
    // File-first: `sessions/<id>/enhanced/<noteId>.md` is the doc's canonical home, and the
    // store's dual-write seeds the `session_documents` index row (readers stay on SQL until
    // Phase E). The store refuses to create a doc for a session without `_meta.json`, which
    // replaces the old INSERT's `expectedRowsAffected` session-existence guard.
    const result = await commands.sessionWriteEnhancedDoc({
      id: noteId,
      session_id: sessionId,
      kind: normalizedTemplateId ? "template_output" : "summary",
      title: "Summary",
      template_id: normalizedTemplateId,
      sort_order: position,
      markdown: "",
    });
    if (result.status === "error") {
      throw new Error(
        `Failed to create summary document for session ${sessionId}: ${result.error}`,
      );
    }

    return {
      id: noteId,
      title: "Summary",
      markdown: "",
      content: "",
      contentFormat: "md",
      templateId: normalizedTemplateId,
      position,
    };
  });
}

export function replaceSummaryDocumentTemplate({
  sessionId,
  noteId,
  templateId,
  title,
}: {
  sessionId: string;
  noteId: string;
  templateId?: string;
  title: string;
}): Promise<void> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    const normalizedTemplateId = templateId ?? "";
    const result = await commands.sessionUpdateEnhancedDoc(sessionId, noteId, {
      kind: normalizedTemplateId ? "template_output" : "summary",
      template_id: normalizedTemplateId,
      title,
      markdown: "",
    });
    if (result.status === "error") {
      throw new Error(
        `Failed to replace summary ${noteId} template: ${result.error}`,
      );
    }
  });
}

export function updateSummaryDocumentTitleIfCurrent({
  sessionId,
  noteId,
  currentTitle,
  nextTitle,
}: {
  sessionId: string;
  noteId: string;
  currentTitle: string;
  nextTitle: string;
}): Promise<void> {
  return enqueueDatabaseWrite(`session:${sessionId}`, async () => {
    const result = await commands.sessionUpdateEnhancedDoc(sessionId, noteId, {
      title: nextTitle,
      expected_title: currentTitle,
    });
    if (result.status === "error") {
      // A CAS miss (user renamed the tab meanwhile) is the expected no-op outcome, same
      // as the old `WHERE title = ?` update affecting zero rows -- only real failures
      // propagate.
      if (isStoreConflictError(result.error)) {
        return;
      }
      throw new Error(
        `Failed to hydrate summary ${noteId} title: ${result.error}`,
      );
    }
  });
}
