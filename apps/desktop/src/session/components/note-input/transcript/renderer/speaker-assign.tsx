import { useCallback, useEffect, useRef, useState } from "react";

import { cn } from "@hypr/utils";

import type { Segment } from "~/stt/live-segment";
import { assignTranscriptSpeaker } from "~/stt/queries";

export function SpeakerRenameControl({
  segment,
  transcriptId,
  color,
  label,
  className,
  onAssigned,
}: {
  segment: Segment;
  transcriptId: string;
  color: string;
  label: string;
  className?: string;
  onAssigned?: (speakerLabel: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(label);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!editing) {
      setDraft(label);
    }
  }, [editing, label]);

  const startEditing = useCallback(() => {
    setDraft(label);
    setEditing(true);
  }, [label]);

  const handleSave = useCallback(() => {
    setEditing(false);
    const speakerLabel = draft.trim();
    if (!speakerLabel || speakerLabel === label) {
      return;
    }

    const anchorWordId = getAssignmentAnchorWordId(segment);
    if (!anchorWordId) {
      return;
    }

    void assignTranscriptSpeaker({
      transcriptId,
      segmentKey: segment.key,
      speakerLabel,
      anchorWordId,
    })
      .then(() => onAssigned?.(speakerLabel))
      .catch((error) => {
        console.error("[transcript] failed to assign speaker", error);
      });
  }, [draft, label, onAssigned, segment, transcriptId]);

  if (editing) {
    return (
      <input
        ref={inputRef}
        autoFocus
        type="text"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={handleSave}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            inputRef.current?.blur();
          } else if (e.key === "Escape") {
            e.preventDefault();
            setDraft(label);
            setEditing(false);
          }
        }}
        style={{ color }}
        className={cn([
          "-my-0.5 min-w-0 rounded-full bg-transparent py-0.5 pr-2",
          "border-none outline-hidden",
          className,
        ])}
      />
    );
  }

  return (
    <button
      type="button"
      onClick={startEditing}
      className={cn([
        "-my-0.5 cursor-pointer rounded-full py-0.5 pr-2",
        "underline-offset-2 hover:underline focus-visible:underline",
        className,
      ])}
      style={{ color }}
    >
      {label}
    </button>
  );
}

export function getAssignmentAnchorWordId(
  segment: Segment,
): string | undefined {
  const word = segment.words.find(
    (word) => typeof word.id === "string" && word.id.length > 0,
  );
  return typeof word?.id === "string" ? word.id : undefined;
}
