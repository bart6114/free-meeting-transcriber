import { Fragment, memo } from "react";

import { cn } from "@hypr/utils";

import type { HighlightSegment } from "./utils";

import type { SegmentWord } from "~/stt/live-segment";
import { isTranscriptWordSeekable } from "~/stt/timing";

interface WordSpanProps {
  word: SegmentWord;
  displayText: string;
  audioExists: boolean;
  onClickWord: (word: SegmentWord) => void;
  highlightSegments?: HighlightSegment[];
  isActiveMatch?: boolean;
}

export const WordSpan = memo(function WordSpan(props: WordSpanProps) {
  const content = getHighlightedContent(
    props.word,
    props.displayText,
    props.highlightSegments,
    props.isActiveMatch ?? false,
  );
  const canSeek = props.audioExists && isTranscriptWordSeekable(props.word);
  const className = cn([
    canSeek && "hover:bg-accent/60 cursor-pointer",
    !props.word.is_final && ["opacity-60", "italic"],
  ]);

  return (
    <span
      onClick={() => canSeek && props.onClickWord(props.word)}
      className={className}
      data-word-id={props.word.id}
    >
      {content}
    </span>
  );
});

function getHighlightedContent(
  word: SegmentWord,
  displayText: string,
  segments: HighlightSegment[] | undefined,
  isActive: boolean,
) {
  if (!segments) {
    return displayText;
  }

  const baseKey = word.id ?? word.text ?? "word";

  return segments.map((segment, index) =>
    segment.isMatch ? (
      <span
        key={`${baseKey}-match-${index}`}
        className={isActive ? "bg-brand/60" : "bg-brand/25"}
      >
        {segment.text}
      </span>
    ) : (
      <Fragment key={`${baseKey}-text-${index}`}>{segment.text}</Fragment>
    ),
  );
}
