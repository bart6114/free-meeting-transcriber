import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RawEditor as SessionRawEditor } from "./raw";

const hoisted = vi.hoisted(() => ({
  rawMd: JSON.stringify({ type: "doc", content: [] }),
  sessionTitle: "Weekly sync",
  persistChange: vi.fn(() => Promise.resolve()),
  noteEditorProps: [] as Record<string, unknown>[],
  json2md: vi.fn(() => "markdown"),
  sessionWriteNote: vi.fn(
    (): Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    > => Promise.resolve({ status: "ok", data: null }),
  ),
  sonnerToastError: vi.fn(),
}));

vi.mock("@hypr/editor/markdown", () => ({
  parseJsonContent: (value: string) => JSON.parse(value),
  json2md: hoisted.json2md,
}));

vi.mock("@hypr/editor/note", () => ({
  normalizePortableAttachmentUrls: (value: unknown) => value,
  NoteEditor: (props: Record<string, unknown>) => {
    hoisted.noteEditorProps.push(props);

    return <div>Note editor</div>;
  },
}));

vi.mock("@hypr/plugin-analytics", () => ({
  commands: {
    event: vi.fn(),
  },
}));

vi.mock("@hypr/ui/components/ui/toast", () => ({
  sonnerToast: { error: hoisted.sonnerToastError },
}));

vi.mock("@hypr/plugin-opener2", () => ({
  commands: { openUrl: vi.fn() },
}));

vi.mock("~/editor-bridge/app-link-view", () => ({
  AppLinkView: () => null,
}));

vi.mock("~/editor-bridge/mention-config", () => ({
  useMentionConfig: () => ({ users: [] }),
}));

vi.mock("~/editor-bridge/open-editor-link", () => ({
  openEditorLink: vi.fn(),
}));

vi.mock("~/editor-bridge/session-mention-drop", () => ({
  sessionMentionDropConfig: { read: () => null },
}));

vi.mock("~/editor-bridge/session-view", () => ({
  SessionNodeView: () => null,
}));

vi.mock("~/session/components/shared", () => ({
  hasStoredNoteContent: (value: unknown) => Boolean(value),
}));

vi.mock("~/session/queries", () => ({
  useUpdateSession: () => hoisted.persistChange,
}));

vi.mock("~/session/hooks/useAttachmentResolver", () => ({
  useAttachmentResolver: () => () => null,
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    sessionWriteNote: hoisted.sessionWriteNote,
  },
}));

function RawEditor({
  sessionId,
  className,
}: {
  sessionId: string;
  className?: string;
}) {
  return (
    <SessionRawEditor
      sessionId={sessionId}
      rawMd={hoisted.rawMd}
      sessionTitle={hoisted.sessionTitle}
      className={className}
    />
  );
}

describe("RawEditor", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    hoisted.noteEditorProps = [];
    hoisted.rawMd = JSON.stringify({ type: "doc", content: [] });
    hoisted.sessionTitle = "Weekly sync";
    hoisted.persistChange = vi.fn(() => Promise.resolve());
    hoisted.json2md.mockReset().mockReturnValue("markdown");
    hoisted.sessionWriteNote
      .mockReset()
      .mockResolvedValue({ status: "ok", data: null });
    hoisted.sonnerToastError.mockReset();
  });

  it("uses the shared session note editor styling", () => {
    render(<RawEditor sessionId="session-1" className="custom-editor-class" />);

    const props = hoisted.noteEditorProps[hoisted.noteEditorProps.length - 1];

    expect(props?.className).toContain("session-note-editor");
    expect(props?.className).toContain("custom-editor-class");
    expect(props?.placeholderComponent).toEqual(expect.any(Function));
    expect(props?.initialContent).toMatchObject({
      type: "doc",
      content: [
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Weekly sync" }],
        },
      ],
    });
  });

  it("shows a persistent toast when saving the note fails", async () => {
    hoisted.sessionWriteNote.mockResolvedValue({
      status: "error",
      error: "disk full",
    });

    render(<RawEditor sessionId="session-1" />);

    const props = hoisted.noteEditorProps[hoisted.noteEditorProps.length - 1];
    const handleChange = props?.handleChange as (input: unknown) => void;
    handleChange({ type: "doc", content: [] });

    await waitFor(() =>
      expect(hoisted.sessionWriteNote).toHaveBeenCalledWith(
        "session-1",
        "markdown",
      ),
    );
    await waitFor(() =>
      expect(hoisted.sonnerToastError).toHaveBeenCalledWith(
        expect.stringContaining("Note is NOT being saved"),
        expect.objectContaining({ id: "note-save-failed:session-1" }),
      ),
    );
  });
});
