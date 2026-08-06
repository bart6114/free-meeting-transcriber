import { useMemo } from "react";

import { cn } from "@hypr/utils";

import { SpeakerRenameControl } from "./speaker-assign";
import { useSegmentColorVars } from "./utils";

import { useListener } from "~/stt/contexts";
import type { Segment } from "~/stt/live-segment";
import { SegmentKeyUtils, SpeakerLabelManager } from "~/stt/live-segment";
import { useTranscript, useTranscriptLabelContext } from "~/stt/queries";

export function SegmentHeader({
  segment,
  transcriptId,
  speakerLabelManager,
}: {
  segment: Segment;
  transcriptId: string;
  speakerLabelManager?: SpeakerLabelManager;
}) {
  const colorVars = useSegmentColorVars(segment.key);
  const label = useSpeakerLabel(segment.key, transcriptId, speakerLabelManager);
  const recording = useSessionIsRecording(transcriptId);
  const headerClassName = cn([
    "bg-card sticky top-0 z-20",
    "-mx-3 px-3 py-1",
    "timecode text-muted-foreground",
    "flex items-center gap-3",
    "[--segment-color:var(--segment-color-light)]",
    "dark:[--segment-color:var(--segment-color-dark)]",
  ]);

  return (
    <div className={headerClassName} style={colorVars}>
      <SpeakerRenameControl
        segment={segment}
        transcriptId={transcriptId}
        color="var(--segment-color)"
        label={label}
        disabled={recording}
      />
    </div>
  );
}

function useSpeakerLabel(
  key: Segment["key"],
  transcriptId: string,
  manager?: SpeakerLabelManager,
) {
  const labelContext = useTranscriptLabelContext(transcriptId);

  return useMemo(
    () => SegmentKeyUtils.renderLabel(key, labelContext, manager),
    [key, labelContext, manager],
  );
}

// Renaming while the session records races the live transcript buffer (the store-side
// write clears it and the next flush can clobber both words and the new hint), so the
// control is disabled until recording finishes.
function useSessionIsRecording(transcriptId: string) {
  const sessionId = useTranscript(transcriptId)?.sessionId;

  return useListener((state) => {
    if (!sessionId) {
      return false;
    }
    const mode = state.getSessionMode(sessionId);
    return mode === "active" || mode === "finalizing";
  });
}
