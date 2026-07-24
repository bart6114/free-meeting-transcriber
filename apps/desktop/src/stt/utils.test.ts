import { describe, expect, it } from "vitest";

import type { LiveTranscriptDelta } from "@hypr/plugin-transcription";

import {
  createTranscriptAccumulator,
  updateTranscriptHints,
  upsertSpeakerAssignment,
} from "./utils";

import type { SegmentKey } from "~/stt/live-segment";

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
    upsertSpeakerAssignment(
      store,
      "transcript-1",
      remoteSpeakerKey(2),
      "Alice",
      "word-1",
    );
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
    upsertSpeakerAssignment(
      store,
      "transcript-1",
      remoteSpeakerKey(2),
      "Alice",
      "word-1",
    );
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

function remoteSpeakerKey(speakerIndex: number | null): SegmentKey {
  return {
    channel: "RemoteParty",
    speaker_index: speakerIndex,
    speaker_human_id: null,
  } as SegmentKey;
}

describe("upsertSpeakerAssignment", () => {
  it("removes a stale channel-wide assignment when reassigning a speaker", () => {
    const store = createStore({
      words: JSON.stringify([
        {
          id: "old-word",
          text: " hello",
          start_ms: 0,
          end_ms: 100,
          channel: 1,
        },
        {
          id: "new-word",
          text: " there",
          start_ms: 100,
          end_ms: 200,
          channel: 1,
        },
      ]),
      speaker_hints: JSON.stringify([
        {
          id: "old-word:speaker_label",
          word_id: "old-word",
          type: "speaker_label",
          value: "Alice",
        },
        {
          id: "new-word:provider_speaker_index",
          word_id: "new-word",
          type: "provider_speaker_index",
          value: JSON.stringify({ channel: 1, speaker_index: 2 }),
        },
      ]),
    });

    upsertSpeakerAssignment(
      store,
      "transcript-1",
      remoteSpeakerKey(2),
      "Bob",
      "new-word",
    );

    expect(
      JSON.parse(
        store.getCell("transcripts", "transcript-1", "speaker_hints") as string,
      ),
    ).toEqual([
      {
        id: "new-word:provider_speaker_index",
        word_id: "new-word",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
      {
        id: "new-word:speaker_label",
        word_id: "new-word",
        type: "speaker_label",
        value: "Bob",
      },
    ]);
  });

  it("keeps other speaker assignments on the same channel", () => {
    const store = createStore({
      words: JSON.stringify([
        {
          id: "speaker-1-word",
          text: " first",
          start_ms: 0,
          end_ms: 100,
          channel: 1,
        },
        {
          id: "speaker-2-word-old",
          text: " second",
          start_ms: 100,
          end_ms: 200,
          channel: 1,
        },
        {
          id: "speaker-2-word-new",
          text: " later",
          start_ms: 200,
          end_ms: 300,
          channel: 1,
        },
      ]),
      speaker_hints: JSON.stringify([
        {
          id: "speaker-1-word:provider_speaker_index",
          word_id: "speaker-1-word",
          type: "provider_speaker_index",
          value: JSON.stringify({ channel: 1, speaker_index: 1 }),
        },
        {
          id: "speaker-1-word:speaker_label",
          word_id: "speaker-1-word",
          type: "speaker_label",
          value: "Alice",
        },
        {
          id: "speaker-2-word-old:provider_speaker_index",
          word_id: "speaker-2-word-old",
          type: "provider_speaker_index",
          value: JSON.stringify({ channel: 1, speaker_index: 2 }),
        },
        {
          id: "speaker-2-word-old:speaker_label",
          word_id: "speaker-2-word-old",
          type: "speaker_label",
          value: "Bob",
        },
        {
          id: "speaker-2-word-new:provider_speaker_index",
          word_id: "speaker-2-word-new",
          type: "provider_speaker_index",
          value: JSON.stringify({ channel: 1, speaker_index: 2 }),
        },
      ]),
    });

    upsertSpeakerAssignment(
      store,
      "transcript-1",
      remoteSpeakerKey(2),
      "Carol",
      "speaker-2-word-new",
    );

    expect(
      JSON.parse(
        store.getCell("transcripts", "transcript-1", "speaker_hints") as string,
      ),
    ).toEqual([
      {
        id: "speaker-1-word:provider_speaker_index",
        word_id: "speaker-1-word",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 1 }),
      },
      {
        id: "speaker-1-word:speaker_label",
        word_id: "speaker-1-word",
        type: "speaker_label",
        value: "Alice",
      },
      {
        id: "speaker-2-word-old:provider_speaker_index",
        word_id: "speaker-2-word-old",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
      {
        id: "speaker-2-word-new:provider_speaker_index",
        word_id: "speaker-2-word-new",
        type: "provider_speaker_index",
        value: JSON.stringify({ channel: 1, speaker_index: 2 }),
      },
      {
        id: "speaker-2-word-new:speaker_label",
        word_id: "speaker-2-word-new",
        type: "speaker_label",
        value: "Carol",
      },
    ]);
  });

  it("keeps assignments on a different channel untouched", () => {
    const store = createStore({
      words: JSON.stringify([
        {
          id: "direct-word",
          text: " hi",
          start_ms: 0,
          end_ms: 100,
          channel: 0,
        },
        {
          id: "remote-word",
          text: " there",
          start_ms: 100,
          end_ms: 200,
          channel: 1,
        },
      ]),
      speaker_hints: JSON.stringify([
        {
          id: "direct-word:speaker_label",
          word_id: "direct-word",
          type: "speaker_label",
          value: "Me",
        },
      ]),
    });

    upsertSpeakerAssignment(
      store,
      "transcript-1",
      remoteSpeakerKey(null),
      "Bob",
      "remote-word",
    );

    expect(
      JSON.parse(
        store.getCell("transcripts", "transcript-1", "speaker_hints") as string,
      ),
    ).toEqual([
      {
        id: "direct-word:speaker_label",
        word_id: "direct-word",
        type: "speaker_label",
        value: "Me",
      },
      {
        id: "remote-word:speaker_label",
        word_id: "remote-word",
        type: "speaker_label",
        value: "Bob",
      },
    ]);
  });
});
