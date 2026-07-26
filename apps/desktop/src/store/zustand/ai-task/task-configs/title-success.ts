import type { TaskConfig } from ".";

import {
  applyGeneratedSessionTitle,
  type SessionDocumentContentUpdate,
} from "~/session/content-mutations";
import { loadSessionContentSnapshot } from "~/session/content-queries";
import { ensureMarkdownFirstLineTitle } from "~/session/title-content";
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

  // Markdown-based since D-3: the CAS runs against the doc file's markdown body, so the
  // update carries the snapshot's markdown (what the file held when we read it) and the
  // markdown-stamped result.
  const documents: SessionDocumentContentUpdate[] = snapshot.enhancedNotes
    .filter((note) => note.markdown.trim())
    .map((note) => ({
      id: note.id,
      currentMarkdown: note.markdown,
      nextMarkdown: ensureMarkdownFirstLineTitle(note.markdown, trimmed),
    }));

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

export function getPersistableGeneratedTitle(text: string): string {
  const trimmed = text.trim();
  return trimmed && trimmed !== "<EMPTY>" ? trimmed : "";
}

export const titleSuccess: Pick<TaskConfig<"title">, "onSuccess"> = {
  onSuccess,
};
