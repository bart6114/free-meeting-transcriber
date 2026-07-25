import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getAssignmentAnchorWordId,
  SpeakerRenameControl,
} from "./speaker-assign";

import type { Segment } from "~/stt/live-segment";

const { assignTranscriptSpeakerMock } = vi.hoisted(() => ({
  assignTranscriptSpeakerMock: vi.fn(),
}));

vi.mock("~/stt/queries", () => ({
  assignTranscriptSpeaker: assignTranscriptSpeakerMock,
}));

beforeEach(() => {
  cleanup();
  vi.clearAllMocks();
  assignTranscriptSpeakerMock.mockResolvedValue(undefined);
});

function remoteSegment(): Segment {
  return {
    id: "segment-1",
    key: {
      channel: "RemoteParty",
      speaker_index: 2,
      speaker_human_id: null,
    },
    start_ms: 0,
    end_ms: 100,
    text: "hello",
    words: [
      {
        id: "word-1",
        text: "hello",
        start_ms: 0,
        end_ms: 100,
        channel: "RemoteParty",
        is_final: true,
      },
    ],
  } as Segment;
}

describe("SpeakerRenameControl", () => {
  it("renders the current label as a clickable pill", () => {
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    const trigger = screen.getByRole("button", { name: "Speaker 2" });
    expect(trigger.className).toContain("rounded-full");
    expect(trigger.className).toContain("hover:underline");
  });

  it("turns into a prefilled input on click and saves the rename on Enter", async () => {
    const onAssigned = vi.fn();
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
        onAssigned,
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("Speaker 2");

    fireEvent.change(input, { target: { value: "Alice" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith({
        transcriptId: "transcript-1",
        segmentKey: {
          channel: "RemoteParty",
          speaker_index: 2,
          speaker_human_id: null,
        },
        speakerLabel: "Alice",
        anchorWordId: "word-1",
      });
    });
    await waitFor(() => expect(onAssigned).toHaveBeenCalledWith("Alice"));
  });

  it("saves the rename on blur without pressing Enter", async () => {
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    const input = screen.getByRole("textbox") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Bob" } });
    fireEvent.blur(input);

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith(
        expect.objectContaining({ speakerLabel: "Bob" }),
      );
    });
  });

  it("discards the draft on Escape without saving", () => {
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    const input = screen.getByRole("textbox") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Bob" } });
    fireEvent.keyDown(input, { key: "Escape" });

    expect(assignTranscriptSpeakerMock).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Speaker 2" })).toBeTruthy();
  });

  it("does not assign when the trimmed value is empty or unchanged", async () => {
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "   " } });
    fireEvent.blur(screen.getByRole("textbox"));

    expect(assignTranscriptSpeakerMock).not.toHaveBeenCalled();
  });
});

describe("getAssignmentAnchorWordId", () => {
  it("uses the first available word id in the segment", () => {
    const segment = {
      id: "segment-1",
      key: {
        channel: "RemoteParty",
        speaker_index: 1,
        speaker_human_id: null,
      },
      speaker_label: "Speaker 1",
      start_ms: 0,
      end_ms: 200,
      text: "hello there",
      words: [
        {
          text: "hello",
          start_ms: 0,
          end_ms: 100,
          channel: "RemoteParty",
          is_final: true,
        },
        {
          id: "word-2",
          text: "there",
          start_ms: 100,
          end_ms: 200,
          channel: "RemoteParty",
          is_final: true,
        },
      ],
    } as Segment;

    expect(getAssignmentAnchorWordId(segment)).toBe("word-2");
  });
});
