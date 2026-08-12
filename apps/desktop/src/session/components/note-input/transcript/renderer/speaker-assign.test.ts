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

const {
  assignTranscriptSpeakerMock,
  ensurePersonMock,
  usePeopleMock,
  useTranscriptMock,
} = vi.hoisted(() => ({
  assignTranscriptSpeakerMock: vi.fn(),
  ensurePersonMock: vi.fn(),
  usePeopleMock: vi.fn(),
  useTranscriptMock: vi.fn(),
}));

vi.mock("~/stt/queries", () => ({
  assignTranscriptSpeaker: assignTranscriptSpeakerMock,
  useTranscript: useTranscriptMock,
}));

vi.mock("~/people/queries", () => ({
  usePeople: usePeopleMock,
  ensurePerson: ensurePersonMock,
}));

beforeEach(() => {
  cleanup();
  vi.clearAllMocks();
  assignTranscriptSpeakerMock.mockResolvedValue(undefined);
  ensurePersonMock.mockImplementation(async (name: string) => ({
    id: name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_"),
    name: name.trim(),
  }));
  usePeopleMock.mockReturnValue([]);
  useTranscriptMock.mockReturnValue(null);
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

  it("is inert while the session is recording", () => {
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
        disabled: true,
      }),
    );

    const trigger = screen.getByRole("button", {
      name: "Speaker 2",
    }) as HTMLButtonElement;
    expect(trigger.disabled).toBe(true);
    fireEvent.click(trigger);
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("ensures a person and assigns their id on Enter", async () => {
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

    fireEvent.change(input, { target: { value: "Alice Smith" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(ensurePersonMock).toHaveBeenCalledWith("Alice Smith");
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith({
        transcriptId: "transcript-1",
        segmentKey: {
          channel: "RemoteParty",
          speaker_index: 2,
          speaker_human_id: null,
        },
        speakerLabel: "alice_smith",
        anchorWordId: "word-1",
      });
    });
    await waitFor(() => expect(onAssigned).toHaveBeenCalledWith("alice_smith"));
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
        expect.objectContaining({ speakerLabel: "bob" }),
      );
    });
  });

  it("lists matching people and assigns the clicked person without ensure", async () => {
    usePeopleMock.mockReturnValue([
      { id: "bob_peters", name: "Bob Peters" },
      { id: "kim", name: "Kim" },
    ]);
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
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "bo" } });

    const option = screen.getByRole("option", { name: "Bob Peters" });
    expect(screen.queryByRole("option", { name: "Kim" })).toBeNull();
    fireEvent.click(option);

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith(
        expect.objectContaining({ speakerLabel: "bob_peters" }),
      );
    });
    expect(ensurePersonMock).not.toHaveBeenCalled();
    await waitFor(() => expect(onAssigned).toHaveBeenCalledWith("bob_peters"));
  });

  it("commits exactly once when a suggestion click races the blur save", async () => {
    usePeopleMock.mockReturnValue([{ id: "bob_peters", name: "Bob Peters" }]);
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

    const option = screen.getByRole("option", { name: "Bob Peters" });
    fireEvent.click(option);
    fireEvent.blur(input);

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledTimes(1);
    });
    expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith(
      expect.objectContaining({ speakerLabel: "bob_peters" }),
    );
    expect(ensurePersonMock).not.toHaveBeenCalled();
  });

  it("selects the highlighted suggestion with arrow keys and Enter", async () => {
    usePeopleMock.mockReturnValue([
      { id: "anna", name: "Anna" },
      { id: "bob_peters", name: "Bob Peters" },
    ]);
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
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith(
        expect.objectContaining({ speakerLabel: "bob_peters" }),
      );
    });
    expect(ensurePersonMock).not.toHaveBeenCalled();
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
    expect(ensurePersonMock).not.toHaveBeenCalled();
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
    expect(ensurePersonMock).not.toHaveBeenCalled();
  });

  it("skips the assignment when ensurePerson fails", async () => {
    ensurePersonMock.mockRejectedValue(new Error("disk full"));
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Bob" },
    });
    fireEvent.blur(screen.getByRole("textbox"));

    await waitFor(() => expect(consoleError).toHaveBeenCalled());
    expect(assignTranscriptSpeakerMock).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });
});

describe("SpeakerRenameControl on a diarized channel", () => {
  function diarizedTranscript(withAssignment: boolean) {
    return {
      id: "transcript-1",
      ownerUserId: "self",
      sessionId: "session-1",
      startedAt: 0,
      words: [
        { id: "word-1", text: "hello", start_ms: 0, end_ms: 100, channel: 1 },
        { id: "word-9", text: "there", start_ms: 100, end_ms: 200, channel: 1 },
      ],
      speakerHints: [
        {
          word_id: "word-1",
          type: "provider_speaker_index",
          value: JSON.stringify({ channel: 1, speaker_index: 2 }),
        },
        {
          word_id: "word-9",
          type: "provider_speaker_index",
          value: JSON.stringify({ channel: 1, speaker_index: 0 }),
        },
        ...(withAssignment
          ? [{ word_id: "word-9", type: "speaker_label", value: "kim" }]
          : []),
      ],
    };
  }

  it("defaults the first assignment to every speaker index on the channel", async () => {
    useTranscriptMock.mockReturnValue(diarizedTranscript(false));
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    const checkbox = screen.getByRole("checkbox");
    expect(checkbox.getAttribute("data-state")).toBe("checked");

    const input = screen.getByRole("textbox") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Alice" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledTimes(2);
    });
    expect(
      assignTranscriptSpeakerMock.mock.calls.map(([call]) => call),
    ).toEqual([
      {
        transcriptId: "transcript-1",
        segmentKey: {
          channel: "RemoteParty",
          speaker_index: 2,
          speaker_human_id: null,
        },
        speakerLabel: "alice",
        anchorWordId: "word-1",
      },
      {
        transcriptId: "transcript-1",
        segmentKey: {
          channel: "RemoteParty",
          speaker_index: 0,
          speaker_human_id: null,
        },
        speakerLabel: "alice",
        anchorWordId: "word-9",
      },
    ]);
  });

  it("assigns only the clicked cluster when the checkbox is unchecked", async () => {
    useTranscriptMock.mockReturnValue(diarizedTranscript(false));
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    fireEvent.click(screen.getByRole("checkbox"));

    const input = screen.getByRole("textbox") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Alice" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledTimes(1);
    });
    expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith({
      transcriptId: "transcript-1",
      segmentKey: {
        channel: "RemoteParty",
        speaker_index: 2,
        speaker_human_id: null,
      },
      speakerLabel: "alice",
      anchorWordId: "word-1",
    });
  });

  it("skips the channel-wide offer once the channel has an assignment", async () => {
    useTranscriptMock.mockReturnValue(diarizedTranscript(true));
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    expect(screen.queryByRole("checkbox")).toBeNull();

    const input = screen.getByRole("textbox") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Alice" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(assignTranscriptSpeakerMock).toHaveBeenCalledTimes(1);
    });
    expect(assignTranscriptSpeakerMock).toHaveBeenCalledWith(
      expect.objectContaining({
        segmentKey: {
          channel: "RemoteParty",
          speaker_index: 2,
          speaker_human_id: null,
        },
        anchorWordId: "word-1",
      }),
    );
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
