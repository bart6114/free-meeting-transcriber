import { describe, expect, it } from "vitest";

import type { LiveTranscriptDelta } from "@hypr/plugin-transcription";

import { createTranscriptAccumulator, updateTranscriptHints } from "./utils";

type TranscriptRow = {
  words?: string;
  speaker_hints?: string;
};

function createStore(row: TranscriptRow) {
  const transcript = {
    words: row.words ?? JSON.stringify([]),
    speaker_hints: row.speaker_hints ?? JSON.stringify([]),
  };
  const getCellCalls: string[] = [];

  return {
    getCellCalls,
    readCell: (cellId: "words" | "speaker_hints") => transcript[cellId],
    getCell: (
      tableId: "transcripts",
      rowId: string,
      cellId: "words" | "speaker_hints",
    ) => {
      if (tableId !== "transcripts" || rowId !== "transcript-1") {
        return undefined;
      }

      getCellCalls.push(cellId);
      return transcript[cellId];
    },
    setCell: (
      tableId: "transcripts",
      rowId: string,
      cellId: "words" | "speaker_hints",
      value: string,
    ) => {
      if (tableId !== "transcripts" || rowId !== "transcript-1") {
        return;
      }

      transcript[cellId] = value;
    },
  };
}

function liveDelta(
  newWords: LiveTranscriptDelta["new_words"],
  replacedIds: string[] = [],
): LiveTranscriptDelta {
  return {
    new_words: newWords,
    replaced_ids: replacedIds,
    partials: [],
  };
}

describe("TranscriptAccumulator", () => {
  it("applies live deltas without rereading newly-created transcript JSON", () => {
    const store = createStore({});
    const accumulator = createTranscriptAccumulator(store, "transcript-1", {
      words: [],
      hints: [],
    });

    accumulator.applyLiveDelta(
      liveDelta([
        {
          id: "word-1",
          text: "hello",
          start_ms: 100,
          end_ms: 200,
          channel: 0,
          state: "final",
          speaker_index: 1,
        },
      ]),
    );
    accumulator.applyLiveDelta(
      liveDelta(
        [
          {
            id: "word-2",
            text: "hello",
            start_ms: 100,
            end_ms: 220,
            channel: 0,
            state: "final",
          },
        ],
        ["word-1"],
      ),
    );
    accumulator.dispose();

    expect(store.getCellCalls).toEqual([]);
    expect(JSON.parse(store.readCell("words"))).toEqual([
      {
        id: "word-2",
        text: "hello",
        start_ms: 100,
        end_ms: 220,
        channel: 0,
      },
    ]);
    expect(JSON.parse(store.readCell("speaker_hints"))).toEqual([]);
  });

  it("drops a speaker assignment when its anchor word is replaced by a live delta", () => {
    const store = createStore({});
    const accumulator = createTranscriptAccumulator(store, "transcript-1", {
      words: [
        {
          id: "word-1",
          text: "hello",
          start_ms: 0,
          end_ms: 100,
          channel: 0,
        },
      ],
      hints: [
        {
          id: "word-1:speaker_label",
          word_id: "word-1",
          type: "speaker_label",
          value: "Alice",
        },
      ],
    });

    accumulator.applyLiveDelta(
      liveDelta(
        [
          {
            id: "word-1b",
            text: "hello",
            start_ms: 0,
            end_ms: 110,
            channel: 0,
            state: "final",
          },
        ],
        ["word-1"],
      ),
    );
    accumulator.dispose();

    expect(JSON.parse(store.readCell("speaker_hints"))).toEqual([]);
  });

  it("appends batch chunks without reparsing stored words and hints", () => {
    const store = createStore({
      words: JSON.stringify([
        {
          id: "existing-word",
          text: "existing",
          start_ms: 0,
          end_ms: 100,
          channel: 0,
        },
      ]),
      speaker_hints: JSON.stringify([]),
    });
    const accumulator = createTranscriptAccumulator(store, "transcript-1");

    accumulator.appendWordsAndHints(
      [
        {
          id: "word-1",
          text: "hello",
          start_ms: 100,
          end_ms: 200,
          channel: 0,
        },
      ],
      [],
    );
    accumulator.appendWordsAndHints(
      [
        {
          id: "word-2",
          text: "world",
          start_ms: 200,
          end_ms: 300,
          channel: 0,
        },
      ],
      [],
    );
    accumulator.dispose();

    expect(store.getCellCalls).toEqual(["words", "speaker_hints"]);
    expect(JSON.parse(store.readCell("words"))).toEqual([
      {
        id: "existing-word",
        text: "existing",
        start_ms: 0,
        end_ms: 100,
        channel: 0,
      },
      {
        id: "word-1",
        text: "hello",
        start_ms: 100,
        end_ms: 200,
        channel: 0,
      },
      {
        id: "word-2",
        text: "world",
        start_ms: 200,
        end_ms: 300,
        channel: 0,
      },
    ]);
  });

  it("preserves live speaker assignments made while an accumulator is active", () => {
    const store = createStore({});
    const accumulator = createTranscriptAccumulator(store, "transcript-1", {
      words: [],
      hints: [],
    });

    accumulator.applyLiveDelta(
      liveDelta([
        {
          id: "word-1",
          text: "hello",
          start_ms: 0,
          end_ms: 100,
          channel: 1,
          state: "final",
          speaker_index: 2,
        },
      ]),
    );
    // Speaker assignment lands out-of-band (the Rust store owns it now); what
    // matters here is that any external hint update marks the accumulator dirty.
    updateTranscriptHints(store, "transcript-1", [
      ...JSON.parse(store.readCell("speaker_hints")),
      {
        id: "word-1:speaker_label",
        word_id: "word-1",
        type: "speaker_label",
        value: "Alice",
      },
    ]);
    accumulator.applyLiveDelta(
      liveDelta([
        {
          id: "word-2",
          text: "there",
          start_ms: 100,
          end_ms: 200,
          channel: 1,
          state: "final",
          speaker_index: 2,
        },
      ]),
    );
    accumulator.dispose();

    expect(JSON.parse(store.readCell("speaker_hints"))).toEqual([
      {
        id: "word-1:provider_speaker_index",
        word_id: "word-1",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
      {
        id: "word-1:speaker_label",
        word_id: "word-1",
        type: "speaker_label",
        value: "Alice",
      },
      {
        id: "word-2:provider_speaker_index",
        word_id: "word-2",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
    ]);
  });

  it("does not restore externally removed speaker assignments from the accumulator cache", () => {
    const store = createStore({});
    const accumulator = createTranscriptAccumulator(store, "transcript-1", {
      words: [],
      hints: [],
    });

    accumulator.applyLiveDelta(
      liveDelta([
        {
          id: "word-1",
          text: "hello",
          start_ms: 0,
          end_ms: 100,
          channel: 1,
          state: "final",
          speaker_index: 2,
        },
      ]),
    );
    updateTranscriptHints(store, "transcript-1", [
      ...JSON.parse(store.readCell("speaker_hints")),
      {
        id: "word-1:speaker_label",
        word_id: "word-1",
        type: "speaker_label",
        value: "Alice",
      },
    ]);
    accumulator.applyLiveDelta(
      liveDelta([
        {
          id: "word-2",
          text: "there",
          start_ms: 100,
          end_ms: 200,
          channel: 1,
          state: "final",
          speaker_index: 2,
        },
      ]),
    );

    const hintsWithoutAssignment = JSON.parse(
      store.readCell("speaker_hints"),
    ).filter((hint: { type?: string }) => hint.type !== "speaker_label");
    updateTranscriptHints(store, "transcript-1", hintsWithoutAssignment);

    accumulator.applyLiveDelta(
      liveDelta([
        {
          id: "word-3",
          text: "again",
          start_ms: 200,
          end_ms: 300,
          channel: 1,
          state: "final",
          speaker_index: 2,
        },
      ]),
    );
    accumulator.dispose();

    expect(JSON.parse(store.readCell("speaker_hints"))).toEqual([
      {
        id: "word-1:provider_speaker_index",
        word_id: "word-1",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
      {
        id: "word-2:provider_speaker_index",
        word_id: "word-2",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
      {
        id: "word-3:provider_speaker_index",
        word_id: "word-3",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
    ]);
  });
});

// The speaker-assignment conflict spec (stale channel-wide assignment replaced,
// other scopes on the same channel kept, other channels untouched) moved to Rust
// with `assign_transcript_speaker`: see crates/vault-write/src/transcript.rs tests.
