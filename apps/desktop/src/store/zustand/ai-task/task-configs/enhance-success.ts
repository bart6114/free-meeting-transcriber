import { md2json } from "@hypr/editor/markdown";

import { createTaskId, type TaskConfig } from ".";
import {
  appendTagLineToMarkdown,
  extractEnhanceTagNames,
} from "./summary-tags";
import {
  getPersistableGeneratedTitle,
  persistGeneratedTitle,
} from "./title-success";

import { executeTransaction } from "~/db";
import {
  constrainSummaryLength,
  countNormalizedCharacters,
  getSummaryLengthPolicy,
} from "~/services/enhancer/summary-length";
import { persistGeneratedEnhancedNote } from "~/session/content-mutations";
import { loadSessionContentSnapshot } from "~/session/content-queries";
import { ensureMarkdownFirstLineTitle } from "~/session/title-content";
import { hasLiveSessionTitleDraft } from "~/store/zustand/live-title";
import { commands } from "~/types/tauri.gen";

const onSuccess: NonNullable<TaskConfig<"enhance">["onSuccess"]> = async ({
  text,
  args,
  transformedArgs,
  model,
  startTask,
  getTaskState,
  signal,
}) => {
  const lengthPolicy = getSummaryLengthPolicy(transformedArgs.transcripts);
  const constrainedText = constrainSummaryLength(text, lengthPolicy);
  if (!constrainedText) {
    return;
  }

  const tagNames = extractEnhanceTagNames(constrainedText, transformedArgs);
  const textWithTags = appendTagLineToMarkdown(constrainedText, tagNames);
  const initialSnapshot = await loadSessionContentSnapshot(args.sessionId);
  if (!initialSnapshot) {
    throw new Error(`Session ${args.sessionId} no longer exists`);
  }

  let trimmedTitle = initialSnapshot.title.trim();
  let generatedTitle = "";
  let shouldPersistGeneratedTitle = false;

  if (!trimmedTitle && !hasLiveSessionTitleDraft(args.sessionId)) {
    const titleTaskId = createTaskId(args.sessionId, "title");
    const titleTask = getTaskState(titleTaskId);

    if (titleTask?.status === "success" || titleTask?.status === "generating") {
      generatedTitle = getPersistableGeneratedTitle(titleTask.streamedText);
    } else {
      await startTask(titleTaskId, {
        model,
        taskType: "title",
        args: {
          sessionId: args.sessionId,
          enhancedNote: textWithTags,
          skipPersist: true,
        },
        onComplete: (title) => {
          generatedTitle = getPersistableGeneratedTitle(title);
        },
      });
    }

    if (signal.aborted) {
      return;
    }
  }

  const snapshot = await loadSessionContentSnapshot(args.sessionId);
  if (!snapshot) {
    throw new Error(`Session ${args.sessionId} no longer exists`);
  }
  const note = snapshot.enhancedNotes.find(
    (candidate) => candidate.id === args.enhancedNoteId,
  );
  if (!note) {
    throw new Error(`Summary ${args.enhancedNoteId} no longer exists`);
  }

  trimmedTitle = snapshot.title.trim();
  if (
    !trimmedTitle &&
    !hasLiveSessionTitleDraft(args.sessionId) &&
    generatedTitle
  ) {
    trimmedTitle = generatedTitle;
    shouldPersistGeneratedTitle = true;
  }

  const titledText = ensureMarkdownFirstLineTitle(
    constrainedText,
    trimmedTitle,
  );
  const tagLine = appendTagLineToMarkdown("", tagNames);
  const reservedTagCharacters = tagLine
    ? countNormalizedCharacters(tagLine) + 1
    : 0;
  const persistableBody = constrainSummaryLength(
    titledText,
    lengthPolicy
      ? {
          ...lengthPolicy,
          maxCharacters: Math.max(
            0,
            lengthPolicy.maxCharacters - reservedTagCharacters,
          ),
          maxSections: null,
        }
      : null,
  );
  // A reset/regenerate aborts this run; a stale run that persisted anyway
  // would overwrite the replacement's summary with old content.
  if (signal.aborted) {
    return;
  }

  const persistableText = appendTagLineToMarkdown(persistableBody, tagNames);
  await persistGeneratedEnhancedNote({
    sessionId: args.sessionId,
    ownerUserId: snapshot.ownerUserId,
    note: {
      id: note.id,
      currentContent: note.content,
      currentContentFormat: note.contentFormat,
      nextContent: JSON.stringify(md2json(persistableText)),
    },
    tagNames,
  });

  // `sessionWriteDocument` only has a single `summary.md` slot per session, so it only
  // mirrors the default (non-templated) summary -- custom template_output notes have no
  // file-canonical home yet and stay index-only, same as before this cutover.
  if (!note.templateId) {
    const documentWrite = await commands.sessionWriteDocument(
      args.sessionId,
      "summary",
      persistableText,
    );
    if (documentWrite.status === "error") {
      throw new Error(
        `Failed to write summary to session store: ${documentWrite.error}`,
      );
    }

    // `sessionWriteDocument` upserts its own `session_documents` row, keyed
    // `{sessionId}:summary`, to keep the file and index in sync -- but the *real* summary
    // row (read by `useEnhancedNoteRecords`, no id filter beyond kind) lives at `note.id`,
    // a separate randomly-generated id from `ensureSummaryDocument`. Left alone, the store's
    // shadow row would show up as a second, blank "Summary" tab. Hide it immediately; the
    // file write above already landed, which is the part this cutover cares about. Guarded
    // by an id check so a session whose real summary id happens to already equal the shadow
    // id (shouldn't happen -- `id()` is a random UUID -- but never risk tombstoning the note
    // actually shown to the user) is left untouched.
    const shadowRowId = `${args.sessionId}:summary`;
    if (note.id !== shadowRowId) {
      await executeTransaction([
        {
          sql: `
            UPDATE session_documents
            SET deleted_at = ?, updated_at = ?
            WHERE id = ? AND session_id = ? AND kind = 'summary' AND deleted_at IS NULL
          `,
          params: [
            new Date().toISOString(),
            new Date().toISOString(),
            shadowRowId,
            args.sessionId,
          ],
        },
      ]);
    }
  }

  if (shouldPersistGeneratedTitle && !signal.aborted) {
    await persistGeneratedTitle({
      text: generatedTitle,
      args: { sessionId: args.sessionId },
    });
  }
};

export const enhanceSuccess: Pick<TaskConfig<"enhance">, "onSuccess"> = {
  onSuccess,
};
