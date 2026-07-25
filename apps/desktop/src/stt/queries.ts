import { useMemo } from "react";

import { executeTransaction, liveQueryClient, useLiveQuery } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";
import type { RenderLabelContext, SegmentKey } from "~/stt/live-segment";
import {
  collectAssignedHumanIdsFromTranscriptRows,
  type TranscriptRow,
} from "~/stt/render-transcript";
import type { SpeakerHintWithId, WordWithId } from "~/stt/types";
import {
  createTranscriptAccumulator,
  upsertSpeakerAssignment,
} from "~/stt/utils";
import type { TranscriptSpeakerHint, TranscriptWord } from "~/types/tauri.gen";
import { commands } from "~/types/tauri.gen";

type TranscriptSqlRow = {
  id: string;
  owner_user_id: string;
  session_id: string;
  started_at_ms: number;
  ended_at_ms: number | null;
  words_json: string;
  speaker_hints_json: string;
};

type TranscriptMutationSqlRow = {
  session_id: string;
  started_at_ms: number;
  words_json: string;
  speaker_hints_json: string;
};

type TranscriptInsert = {
  id: string;
  sessionId: string;
  ownerUserId: string;
  createdAt: string;
  startedAt: number;
  endedAt?: number;
  memo?: string;
  source?: string;
  provider?: string;
  model?: string;
  language?: string;
  words?: WordWithId[];
  speakerHints?: SpeakerHintWithId[];
  replaceSession?: boolean;
};

export type TranscriptRecord = {
  id: string;
  ownerUserId: string;
  sessionId: string;
  startedAt: number;
  endedAt?: number;
  words: NonNullable<TranscriptRow["words"]>;
  speakerHints: NonNullable<TranscriptRow["speaker_hints"]>;
};

const EMPTY_TRANSCRIPTS: TranscriptRecord[] = [];
const EMPTY_IDS: string[] = [];

const TRANSCRIPT_COLUMNS = `
  id,
  owner_user_id,
  session_id,
  started_at_ms,
  ended_at_ms,
  words_json,
  speaker_hints_json
`;

export function useSessionTranscripts(sessionId: string): TranscriptRecord[] {
  const { data = EMPTY_TRANSCRIPTS } = useLiveQuery<
    TranscriptSqlRow,
    TranscriptRecord[]
  >({
    sql: `
      SELECT ${TRANSCRIPT_COLUMNS}
      FROM transcripts
      WHERE session_id = ? AND deleted_at IS NULL
      ORDER BY started_at_ms, created_at, id
    `,
    params: [sessionId],
    enabled: Boolean(sessionId),
    mapRows: (rows) => rows.map(mapTranscriptRow),
  });
  return sessionId ? data : EMPTY_TRANSCRIPTS;
}

export function useTranscript(transcriptId: string): TranscriptRecord | null {
  const { data = null } = useLiveQuery<
    TranscriptSqlRow,
    TranscriptRecord | null
  >({
    sql: `
      SELECT ${TRANSCRIPT_COLUMNS}
      FROM transcripts
      WHERE id = ? AND deleted_at IS NULL
      LIMIT 1
    `,
    params: [transcriptId],
    enabled: Boolean(transcriptId),
    mapRows: (rows) => (rows[0] ? mapTranscriptRow(rows[0]) : null),
  });
  return transcriptId ? data : null;
}

export function useTranscriptLabelContext(
  transcriptId: string,
): RenderLabelContext | undefined {
  const transcript = useTranscript(transcriptId);
  const assignedSpeakerLabels = useMemo(
    () =>
      transcript
        ? collectAssignedHumanIdsFromTranscriptRows([
            { speaker_hints: transcript.speakerHints },
          ])
        : EMPTY_IDS,
    [transcript],
  );

  return useMemo(() => {
    if (!transcript) return undefined;

    const labels = new Set(assignedSpeakerLabels);
    return {
      getSelfHumanId: () => transcript.ownerUserId || undefined,
      getHumanName: (speakerLabel) =>
        labels.has(speakerLabel) ? speakerLabel : undefined,
      getParticipantHumanIds: () => EMPTY_IDS,
    };
  }, [assignedSpeakerLabels, transcript]);
}

// `source`/`provider`/`model`/`language` are accepted for caller compatibility but not yet
// persisted -- the store's `TranscriptWithData` shape (Tasks 6-8) has no columns for them.
export function createTranscript(input: TranscriptInsert): Promise<void> {
  return enqueueDatabaseWrite(`transcript:${input.id}`, async () => {
    if (input.replaceSession) {
      // `session_write_transcript` upserts one transcript by id; it has no "replace the whole
      // session's transcript set" primitive. Tombstone every other row via the index (index
      // bookkeeping, not a content write -- the words/hints payload below still goes through
      // the store) so a batch re-run doesn't show the old and new transcript side by side.
      const now = new Date().toISOString();
      await executeTransaction([
        {
          sql: `
            UPDATE transcripts
            SET deleted_at = ?, updated_at = ?
            WHERE session_id = ? AND id != ? AND deleted_at IS NULL
          `,
          params: [now, now, input.sessionId, input.id],
        },
      ]);
    }

    await writeTranscriptOrThrow(input.sessionId, {
      id: input.id,
      user_id: input.ownerUserId,
      created_at: input.createdAt,
      session_id: input.sessionId,
      started_at: input.startedAt,
      ended_at: input.endedAt ?? null,
      memo_md: input.memo ?? "",
      words: (input.words ?? []).map(toTranscriptWord),
      speaker_hints: (input.speakerHints ?? []).map(toTranscriptSpeakerHint),
    });
  });
}

export function appendTranscriptWordsAndHints(
  transcriptId: string,
  words: WordWithId[],
  hints: SpeakerHintWithId[],
  options?: { mode?: "append" | "replace" },
): Promise<void> {
  return mutateTranscript(transcriptId, (store) => {
    const accumulator = createTranscriptAccumulator(store, transcriptId);
    accumulator.appendWordsAndHints(words, hints, options);
    accumulator.dispose();
  });
}

export function assignTranscriptSpeaker({
  transcriptId,
  segmentKey,
  speakerLabel,
  anchorWordId,
}: {
  transcriptId: string;
  segmentKey: SegmentKey;
  speakerLabel: string;
  anchorWordId: string;
}): Promise<void> {
  return mutateTranscript(transcriptId, (store) => {
    upsertSpeakerAssignment(
      store,
      transcriptId,
      segmentKey,
      speakerLabel,
      anchorWordId,
    );
  });
}

// Zeroes the transcript's content via a full overwrite rather than truly deleting the row --
// the store has no per-transcript delete, only per-session (`sessionDelete`). An empty
// `words_json` reads the same as "no transcript" everywhere the index is queried
// (`useSessionHasTranscript` etc).
export function softDeleteTranscript(
  sessionId: string,
  transcriptId: string,
): Promise<void> {
  return enqueueDatabaseWrite(`transcript:${transcriptId}`, () =>
    writeTranscriptOrThrow(sessionId, {
      id: transcriptId,
      session_id: sessionId,
      words: [],
      speaker_hints: [],
    }),
  );
}

async function writeTranscriptOrThrow(
  sessionId: string,
  transcript: Parameters<typeof commands.sessionWriteTranscript>[1],
): Promise<void> {
  const result = await commands.sessionWriteTranscript(sessionId, transcript);
  if (result.status === "error") {
    throw new Error(result.error);
  }
}

// `WordWithId`/`SpeakerHintWithId` are TinyBase storage types: every field is typed
// `T | undefined` regardless of whether the zod schema marks it optional, because storage
// cells can be genuinely absent at runtime. The Rust `TranscriptWord`/`TranscriptSpeakerHint`
// types are stricter (`text`/`start_ms`/`end_ms`/`channel`/`word_id` are required) -- default
// missing values rather than reject the word outright, so a partially-populated live word
// still lands on disk instead of silently vanishing from the transcript.
function toTranscriptWord(word: WordWithId): TranscriptWord {
  return {
    id: word.id ?? null,
    text: word.text ?? "",
    start_ms: word.start_ms ?? 0,
    end_ms: word.end_ms ?? 0,
    channel: word.channel ?? 0,
    speaker: word.speaker ?? null,
    metadata: parseMetadata(word.metadata),
  };
}

// `WordWithId.metadata` is TinyBase's JSON-stringified storage representation (see the
// ToStorageType comment above); the Rust command wants the parsed object.
function parseMetadata(
  metadata: string | undefined,
): TranscriptWord["metadata"] {
  if (!metadata) return null;
  try {
    return JSON.parse(metadata) as TranscriptWord["metadata"];
  } catch {
    return null;
  }
}

function toTranscriptSpeakerHint(
  hint: SpeakerHintWithId,
): TranscriptSpeakerHint {
  return {
    id: hint.id ?? null,
    word_id: hint.word_id ?? "",
    type: hint.type ?? "",
    value: hint.value ?? null,
  };
}

function mapTranscriptRow(row: TranscriptSqlRow): TranscriptRecord {
  return {
    id: row.id,
    ownerUserId: row.owner_user_id,
    sessionId: row.session_id,
    startedAt: Number(row.started_at_ms),
    endedAt: row.ended_at_ms === null ? undefined : Number(row.ended_at_ms),
    words: parseJsonArray(row.words_json, row.id, "words"),
    speakerHints: parseJsonArray(
      row.speaker_hints_json,
      row.id,
      "speaker hints",
    ),
  };
}

function parseJsonArray<T>(value: string, rowId: string, field: string): T[] {
  try {
    const parsed = JSON.parse(value);
    if (Array.isArray(parsed)) return parsed as T[];
  } catch (error) {
    console.error(`[transcript] failed to parse ${field} for ${rowId}`, error);
  }

  return [];
}

// `enqueueDatabaseWrite`'s per-`transcriptId` queue already serializes every caller in this
// module (append/speaker-rename), so the read-compute-write below no longer needs its own
// compare-and-swap -- the old SQL CAS guarded against concurrent *SQL* writers, and
// `write_transcript` doesn't have an equivalent primitive to CAS against. A live-recording
// debounced flush can still race this from the Rust side (out of this queue's reach); see
// `write_transcript`'s doc for the one direction that's guarded (batch-supersedes-buffer).
async function mutateTranscript(
  transcriptId: string,
  mutation: (store: MemoryTranscriptStore) => void,
): Promise<void> {
  return enqueueDatabaseWrite(`transcript:${transcriptId}`, async () => {
    const rows = await liveQueryClient.execute<TranscriptMutationSqlRow>(
      `
        SELECT session_id, started_at_ms, words_json, speaker_hints_json
        FROM transcripts
        WHERE id = ? AND deleted_at IS NULL
        LIMIT 1
      `,
      [transcriptId],
    );
    const current = rows[0];
    if (!current) {
      throw new Error(`Transcript ${transcriptId} does not exist`);
    }

    assertJsonArray(current.words_json, transcriptId, "words");
    assertJsonArray(current.speaker_hints_json, transcriptId, "speaker hints");
    const next = mutateTranscriptSnapshot(
      current.words_json,
      current.speaker_hints_json,
      transcriptId,
      mutation,
    );

    await writeTranscriptOrThrow(current.session_id, {
      id: transcriptId,
      session_id: current.session_id,
      started_at: current.started_at_ms,
      words: (JSON.parse(next.wordsJson) as WordWithId[]).map(toTranscriptWord),
      speaker_hints: (JSON.parse(next.hintsJson) as SpeakerHintWithId[]).map(
        toTranscriptSpeakerHint,
      ),
    });
  });
}

type MemoryTranscriptStore = {
  getCell: (
    tableId: "transcripts",
    rowId: string,
    cellId: "words" | "speaker_hints",
  ) => string;
  setCell: (
    tableId: "transcripts",
    rowId: string,
    cellId: "words" | "speaker_hints",
    value: string,
  ) => void;
};

function mutateTranscriptSnapshot(
  wordsJson: string,
  hintsJson: string,
  transcriptId: string,
  mutation: (store: MemoryTranscriptStore) => void,
) {
  const snapshot = { wordsJson, hintsJson };
  const store: MemoryTranscriptStore = {
    getCell: (_tableId, rowId, cellId) => {
      if (rowId !== transcriptId) return "[]";
      return cellId === "words" ? snapshot.wordsJson : snapshot.hintsJson;
    },
    setCell: (_tableId, rowId, cellId, value) => {
      if (rowId !== transcriptId) return;
      if (cellId === "words") {
        snapshot.wordsJson = value;
      } else {
        snapshot.hintsJson = value;
      }
    },
  };

  mutation(store);
  return snapshot;
}

function assertJsonArray(value: string, rowId: string, field: string): void {
  try {
    if (Array.isArray(JSON.parse(value))) return;
  } catch {
    // Report the same corruption error for malformed and non-array payloads.
  }

  throw new Error(`Transcript ${rowId} has invalid ${field} data`);
}
