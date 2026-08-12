import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  Popover,
  PopoverAnchor,
  PopoverContent,
} from "@hypr/ui/components/ui/popover";
import { cn } from "@hypr/utils";

import { ensurePerson, type Person, usePeople } from "~/people/queries";
import type { Segment } from "~/stt/live-segment";
import { assignTranscriptSpeaker } from "~/stt/queries";

export function SpeakerRenameControl({
  segment,
  transcriptId,
  color,
  label,
  className,
  disabled,
  onAssigned,
}: {
  segment: Segment;
  transcriptId: string;
  color: string;
  label: string;
  className?: string;
  disabled?: boolean;
  onAssigned?: (speakerLabel: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(label);
  const [touched, setTouched] = useState(false);
  const [highlightIndex, setHighlightIndex] = useState(-1);
  // Optimistic label: shown from commit until the store round trip updates the
  // label prop, so the rename feels instant on large transcripts.
  const [pendingLabel, setPendingLabel] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const committedRef = useRef(false);
  const commitLabelRef = useRef(label);
  // Distinguishes settles of superseded commits (rapid successive renames) from
  // the latest one: only the latest commit may clear or keep the pending label.
  const commitSeqRef = useRef(0);
  const commitSettledRef = useRef(true);
  const labelRef = useRef(label);
  labelRef.current = label;
  const people = usePeople();

  useEffect(() => {
    // Before the latest commit settles, a label change belongs to an OLDER
    // rename's round trip — the pending label (the user's newest intent) stays.
    if (
      pendingLabel !== null &&
      commitSettledRef.current &&
      label !== commitLabelRef.current
    ) {
      setPendingLabel(null);
    }
  }, [label, pendingLabel]);

  useEffect(() => {
    if (!editing) {
      setDraft(label);
    }
  }, [editing, label]);

  const suggestions = useMemo(() => {
    if (!editing) {
      return [];
    }
    // The prefilled draft is the current label, not a search query — only filter
    // once the user has typed.
    const query = touched ? draft.trim().toLowerCase() : "";
    return people
      .filter((person) => {
        if (person.name === label) {
          return false;
        }
        return !query || person.name.toLowerCase().includes(query);
      })
      .slice(0, 8);
  }, [draft, editing, label, people, touched]);

  useEffect(() => {
    setHighlightIndex(-1);
  }, [draft]);

  const startEditing = useCallback(() => {
    committedRef.current = false;
    setDraft(label);
    setTouched(false);
    setHighlightIndex(-1);
    setEditing(true);
  }, [label]);

  const commit = useCallback(
    (
      optimisticName: string,
      run: () => Promise<{ id: string; name: string }>,
    ) => {
      if (committedRef.current) {
        return;
      }
      committedRef.current = true;
      const seq = ++commitSeqRef.current;
      commitLabelRef.current = label;
      commitSettledRef.current = false;
      setPendingLabel(optimisticName);
      setEditing(false);
      run()
        .then((person) => {
          onAssigned?.(person.id);
          if (seq !== commitSeqRef.current) {
            return;
          }
          commitSettledRef.current = true;
          // Two shapes where the label prop will never (or already did) move,
          // so waiting on the effect above would strand the pending label: the
          // rename resolved to the name already shown (ensurePerson reuses
          // people case-insensitively), or the prop updated before this settle.
          if (
            person.name === commitLabelRef.current ||
            labelRef.current !== commitLabelRef.current
          ) {
            setPendingLabel(null);
          }
        })
        .catch((error) => {
          console.error("[transcript] failed to assign speaker", error);
          if (seq !== commitSeqRef.current) {
            return;
          }
          commitSettledRef.current = true;
          setPendingLabel(null);
        });
    },
    [label, onAssigned],
  );

  const saveFreeText = useCallback(() => {
    if (committedRef.current) {
      return;
    }

    const name = draft.trim();
    const anchorWordId = getAssignmentAnchorWordId(segment);
    if (!name || name === label || !anchorWordId) {
      committedRef.current = true;
      setEditing(false);
      return;
    }

    commit(name, async () => {
      const person = await ensurePerson(name);
      await assignTranscriptSpeaker({
        transcriptId,
        segmentKey: segment.key,
        speakerLabel: person.id,
        anchorWordId,
      });
      return { id: person.id, name: person.name };
    });
  }, [commit, draft, label, segment, transcriptId]);

  const selectPerson = useCallback(
    (person: Person) => {
      const anchorWordId = getAssignmentAnchorWordId(segment);
      if (!anchorWordId) {
        committedRef.current = true;
        setEditing(false);
        return;
      }

      commit(person.name, async () => {
        await assignTranscriptSpeaker({
          transcriptId,
          segmentKey: segment.key,
          speakerLabel: person.id,
          anchorWordId,
        });
        return { id: person.id, name: person.name };
      });
    },
    [commit, segment, transcriptId],
  );

  if (editing) {
    return (
      <Popover open={suggestions.length > 0}>
        <PopoverAnchor asChild>
          <input
            ref={inputRef}
            autoFocus
            type="text"
            value={draft}
            onChange={(e) => {
              setTouched(true);
              setDraft(e.target.value);
            }}
            onBlur={saveFreeText}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setHighlightIndex((index) =>
                  suggestions.length === 0
                    ? -1
                    : (index + 1) % suggestions.length,
                );
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setHighlightIndex((index) =>
                  suggestions.length === 0
                    ? -1
                    : (index <= 0 ? suggestions.length : index) - 1,
                );
              } else if (e.key === "Enter") {
                e.preventDefault();
                const highlighted = suggestions[highlightIndex];
                if (highlighted) {
                  selectPerson(highlighted);
                } else {
                  inputRef.current?.blur();
                }
              } else if (e.key === "Escape") {
                e.preventDefault();
                committedRef.current = true;
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
        </PopoverAnchor>
        <PopoverContent
          align="start"
          className="w-56 p-1"
          onOpenAutoFocus={(e) => e.preventDefault()}
        >
          <ul role="listbox">
            {suggestions.map((person, index) => (
              <li key={person.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={index === highlightIndex}
                  onPointerDown={(e) => e.preventDefault()}
                  onClick={() => selectPerson(person)}
                  onMouseEnter={() => setHighlightIndex(index)}
                  className={cn([
                    "w-full rounded-sm px-2 py-1 text-left text-sm",
                    index === highlightIndex && "bg-accent",
                  ])}
                >
                  {person.name}
                </button>
              </li>
            ))}
          </ul>
        </PopoverContent>
      </Popover>
    );
  }

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={startEditing}
      className={cn([
        "-my-0.5 rounded-full py-0.5 pr-2",
        disabled
          ? "cursor-default"
          : "cursor-pointer underline-offset-2 hover:underline focus-visible:underline",
        className,
      ])}
      style={{ color }}
    >
      {pendingLabel ?? label}
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
