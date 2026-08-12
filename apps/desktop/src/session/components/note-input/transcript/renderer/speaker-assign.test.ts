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

const { assignTranscriptSpeakerMock, ensurePersonMock, usePeopleMock } =
  vi.hoisted(() => ({
    assignTranscriptSpeakerMock: vi.fn(),
    ensurePersonMock: vi.fn(),
    usePeopleMock: vi.fn(),
  }));

vi.mock("~/stt/queries", () => ({
  assignTranscriptSpeaker: assignTranscriptSpeakerMock,
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

  it("shows the typed name immediately while the assignment is pending", async () => {
    let resolveAssign!: () => void;
    assignTranscriptSpeakerMock.mockImplementation(
      () => new Promise<void>((resolve) => (resolveAssign = resolve)),
    );
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
      target: { value: "Alice Smith" },
    });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });

    // Optimistic: the new name shows before ensurePerson/assign settle.
    expect(screen.getByRole("button", { name: "Alice Smith" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Speaker 2" })).toBeNull();

    await waitFor(() => expect(assignTranscriptSpeakerMock).toHaveBeenCalled());
    resolveAssign();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Alice Smith" })).toBeTruthy(),
    );
  });

  it("shows the clicked person's name immediately while the assignment is pending", async () => {
    usePeopleMock.mockReturnValue([{ id: "bob_peters", name: "Bob Peters" }]);
    assignTranscriptSpeakerMock.mockImplementation(() => new Promise(() => {}));
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Speaker 2",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "bo" } });
    fireEvent.click(screen.getByRole("option", { name: "Bob Peters" }));

    expect(screen.getByRole("button", { name: "Bob Peters" })).toBeTruthy();
  });

  it("hands display back to the label prop once the store round trip lands", async () => {
    const onAssigned = vi.fn();
    const props = {
      segment: remoteSegment(),
      transcriptId: "transcript-1",
      color: "red",
      label: "Speaker 2",
      onAssigned,
    };
    const { rerender } = render(createElement(SpeakerRenameControl, props));

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Alice Smith" },
    });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(screen.getByRole("button", { name: "Alice Smith" })).toBeTruthy();

    await waitFor(() => expect(onAssigned).toHaveBeenCalled());
    rerender(
      createElement(SpeakerRenameControl, { ...props, label: "Alice S." }),
    );
    expect(screen.getByRole("button", { name: "Alice S." })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Alice Smith" })).toBeNull();
  });

  it("keeps the newest pending label when an older rename's label change lands first", async () => {
    let resolveAssign!: () => void;
    assignTranscriptSpeakerMock.mockImplementation(
      () => new Promise<void>((resolve) => (resolveAssign = resolve)),
    );
    const props = {
      segment: remoteSegment(),
      transcriptId: "transcript-1",
      color: "red",
      label: "Speaker 2",
    };
    const { rerender } = render(createElement(SpeakerRenameControl, props));

    fireEvent.click(screen.getByRole("button", { name: "Speaker 2" }));
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Bob" },
    });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    await waitFor(() => expect(assignTranscriptSpeakerMock).toHaveBeenCalled());

    // A label change arriving while Bob's commit is still in flight (an older
    // rename's round trip) must not evict the newest optimistic label.
    rerender(createElement(SpeakerRenameControl, { ...props, label: "Alice" }));
    expect(screen.getByRole("button", { name: "Bob" })).toBeTruthy();

    resolveAssign();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Bob" })).toBeNull(),
    );
    expect(screen.getByRole("button", { name: "Alice" })).toBeTruthy();
  });

  it("clears the pending label when the rename resolves to the name already shown", async () => {
    // ensurePerson reuses people case-insensitively: retyping "bob peters" over
    // "Bob Peters" resolves to the existing person and the label prop never
    // changes, so the raw typed text must not stick around.
    ensurePersonMock.mockResolvedValue({
      id: "bob_peters",
      name: "Bob Peters",
    });
    render(
      createElement(SpeakerRenameControl, {
        segment: remoteSegment(),
        transcriptId: "transcript-1",
        color: "red",
        label: "Bob Peters",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Bob Peters" }));
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "bob peters" },
    });
    fireEvent.blur(screen.getByRole("textbox"));
    expect(screen.getByRole("button", { name: "bob peters" })).toBeTruthy();

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Bob Peters" })).toBeTruthy(),
    );
    expect(screen.queryByRole("button", { name: "bob peters" })).toBeNull();
  });

  it("reverts to the previous label when the assignment fails", async () => {
    assignTranscriptSpeakerMock.mockRejectedValue(new Error("write failed"));
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

    expect(screen.getByRole("button", { name: "Bob" })).toBeTruthy();

    await waitFor(() => expect(consoleError).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Speaker 2" })).toBeTruthy(),
    );
    expect(screen.queryByRole("button", { name: "Bob" })).toBeNull();
    consoleError.mockRestore();
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
