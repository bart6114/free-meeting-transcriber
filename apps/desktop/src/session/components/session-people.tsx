import { useMemo, useState } from "react";
import { createPortal } from "react-dom";

import { cn } from "@hypr/utils";

import { usePeople } from "~/people/queries";
import { type TranscriptRecord, useSessionTranscripts } from "~/stt/queries";
import { collectAssignedHumanIdsFromTranscriptRows } from "~/stt/render-transcript";

export function useSessionPeopleNames(sessionId: string): string[] {
  const transcripts = useSessionTranscripts(sessionId);
  return usePeopleNames(transcripts);
}

function usePeopleNames(transcripts: readonly TranscriptRecord[]): string[] {
  const people = usePeople();

  return useMemo(() => {
    const ids = collectAssignedHumanIdsFromTranscriptRows(
      transcripts.map((transcript) => ({
        speaker_hints: transcript.speakerHints,
      })),
    );
    const nameById = new Map(people.map((person) => [person.id, person.name]));
    return [...new Set(ids.map((id) => nameById.get(id) ?? id))];
  }, [people, transcripts]);
}

export function SessionPeople({
  sessionId,
  className,
}: {
  sessionId: string;
  className?: string;
}) {
  const names = useSessionPeopleNames(sessionId);
  return <PeoplePills names={names} className={className} />;
}

export function SessionPeopleFromTranscripts({
  transcripts,
  className,
}: {
  transcripts: readonly TranscriptRecord[];
  className?: string;
}) {
  const names = usePeopleNames(transcripts);
  return <PeoplePills names={names} className={className} />;
}

function PeoplePills({
  names,
  className,
}: {
  names: readonly string[];
  className?: string;
}) {
  if (names.length === 0) {
    return null;
  }

  return (
    <div className={cn(["flex flex-wrap items-center gap-1.5", className])}>
      {names.map((name) => (
        <span
          key={name}
          className="bg-accent/60 text-muted-foreground rounded-full px-2 py-0.5 text-xs font-medium"
        >
          {name}
        </span>
      ))}
    </div>
  );
}

/// Pills that render *below the in-document title* of the memo/summary
/// editors: the title is the document's first node, so the row rides a
/// ProseMirror title-trailer widget. Pass `element` to the editor and render
/// `portal` anywhere in the React tree. The editor supports a single trailer
/// element, so extra below-title content rides along via `trailing`.
export function useSessionPeopleTitleTrailer(
  transcripts: readonly TranscriptRecord[],
  trailing?: React.ReactNode,
): {
  element: HTMLElement;
  portal: React.ReactNode;
} {
  const [element] = useState(() => {
    const node = document.createElement("div");
    node.className = "select-none";
    return node;
  });

  return {
    element,
    portal: createPortal(
      <>
        <SessionPeopleFromTranscripts
          transcripts={transcripts}
          className="mt-1 mb-3"
        />
        {trailing}
      </>,
      element,
    ),
  };
}
