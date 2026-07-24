import { md2json, parseJsonContent } from "@hypr/editor/markdown";

import type { TaskConfig } from ".";

import {
  applyGeneratedSessionTitle,
  type SessionDocumentContentUpdate,
} from "~/session/content-mutations";
import { loadSessionContentSnapshot } from "~/session/content-queries";
import {
  ensureFirstLineTitle,
  ensureMarkdownFirstLineTitle,
} from "~/session/title-content";
import { hasLiveSessionTitleDraft } from "~/store/zustand/live-title";
import { commands } from "~/types/tauri.gen";

const onSuccess: NonNullable<TaskConfig<"title">["onSuccess"]> = async ({
  text,
  args,
}) => {
  if (args.skipPersist) {
    return;
  }

  await persistGeneratedTitle({
    text,
    args,
  });
};

export async function persistGeneratedTitle({
  text,
  args,
}: {
  text: string;
  args: { sessionId: string };
}): Promise<boolean> {
  if (!text) {
    return false;
  }

  const trimmed = getPersistableGeneratedTitle(text);
  if (!trimmed) {
    return false;
  }

  if (hasLiveSessionTitleDraft(args.sessionId)) {
    return false;
  }

  const snapshot = await loadSessionContentSnapshot(args.sessionId);
  if (!snapshot || snapshot.title.trim()) {
    return false;
  }

  if (hasLiveSessionTitleDraft(args.sessionId)) {
    return false;
  }

  const documents: SessionDocumentContentUpdate[] = snapshot.enhancedNotes
    .filter((note) => note.content.trim())
    .map((note) =>
      createTitledDocumentUpdate(
        note.id,
        note.content,
        note.contentFormat,
        trimmed,
      ),
    );

  await applyGeneratedSessionTitle({
    sessionId: args.sessionId,
    currentTitle: snapshot.title,
    nextTitle: trimmed,
    documents,
  });

  // The raw note is stamped separately, file-first: the editor reads/writes it through
  // session_read_note/session_write_note (Task 9's file-canonical note-load path), so writing
  // its title through session_documents SQL here would be invisible to the editor's next read
  // and would desync the index row's body_format from the file. Independent of the SQL
  // transaction above -- a stale note (someone edited it since the snapshot) just skips the
  // note stamp without blocking the title/summary updates.
  if (snapshot.rawMarkdown.trim()) {
    await applyGeneratedNoteTitle(
      args.sessionId,
      trimmed,
      snapshot.rawMarkdown,
    );
  }

  return true;
}

async function applyGeneratedNoteTitle(
  sessionId: string,
  title: string,
  snapshotMarkdown: string,
): Promise<void> {
  const current = await commands.sessionReadNote(sessionId);
  if (current.status === "error") {
    console.error(
      "[title] failed to read note before stamping title",
      current.error,
    );
    return;
  }

  // CAS-like guard: only stamp the title if the file still matches what the snapshot saw.
  // `null` (no file yet -- a session never touched by the store) is treated as "not equal to
  // any non-empty snapshot", so it safely skips rather than fabricating file content.
  if ((current.data ?? "") !== snapshotMarkdown) {
    return;
  }

  const titled = ensureMarkdownFirstLineTitle(snapshotMarkdown, title);
  const result = await commands.sessionWriteNote(sessionId, titled);
  if (result.status === "error") {
    console.error("[title] failed to write titled note", result.error);
  }
}

function createTitledDocumentUpdate(
  id: string,
  content: string,
  contentFormat: string,
  title: string,
): SessionDocumentContentUpdate {
  // "markdown" is the legacy-import sentinel; "md" is what the session store
  // (session_write_note/session_write_document, Tasks 5-8/9) writes -- both need md2json.
  const parsed =
    contentFormat === "markdown" || contentFormat === "md"
      ? md2json(content)
      : parseJsonContent(content);
  return {
    id,
    currentContent: content,
    currentContentFormat: contentFormat,
    nextContent: JSON.stringify(ensureFirstLineTitle(parsed, title)),
  };
}

export function getPersistableGeneratedTitle(text: string): string {
  const trimmed = text.trim();
  return trimmed && trimmed !== "<EMPTY>" ? trimmed : "";
}

export const titleSuccess: Pick<TaskConfig<"title">, "onSuccess"> = {
  onSuccess,
};
