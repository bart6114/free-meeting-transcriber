import { cleanup, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SessionPeople } from "./session-people";

const mocks = vi.hoisted(() => ({
  useSessionTranscripts: vi.fn(),
  usePeople: vi.fn(),
}));

vi.mock("~/stt/queries", () => ({
  useSessionTranscripts: mocks.useSessionTranscripts,
}));

vi.mock("~/people/queries", () => ({
  usePeople: mocks.usePeople,
}));

function transcriptWithLabels(labels: string[]) {
  return {
    speakerHints: labels.map((value, index) => ({
      id: `hint-${value}-${index}`,
      word_id: `word-${value}-${index}`,
      type: "speaker_label",
      value,
    })),
  };
}

beforeEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.usePeople.mockReturnValue([]);
  mocks.useSessionTranscripts.mockReturnValue([]);
});

describe("SessionPeople", () => {
  it("renders nothing when no speakers are named", () => {
    mocks.useSessionTranscripts.mockReturnValue([transcriptWithLabels([])]);

    const { container } = render(<SessionPeople sessionId="session-1" />);

    expect(container.firstChild).toBeNull();
  });

  it("shows registry names for assigned person ids, deduped across transcripts", () => {
    mocks.usePeople.mockReturnValue([{ id: "bob_peters", name: "Bob Peters" }]);
    mocks.useSessionTranscripts.mockReturnValue([
      transcriptWithLabels(["bob_peters"]),
      transcriptWithLabels(["bob_peters", "kim"]),
    ]);

    render(<SessionPeople sessionId="session-1" />);

    expect(screen.getByText("Bob Peters")).toBeTruthy();
    // Legacy raw-name hint with no registry entry renders as itself.
    expect(screen.getByText("kim")).toBeTruthy();
    expect(screen.getAllByText(/Bob Peters|kim/)).toHaveLength(2);
  });
});
