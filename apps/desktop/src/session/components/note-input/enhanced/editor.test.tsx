import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { EnhancedEditor as SessionEnhancedEditor } from "./editor";

const hoisted = vi.hoisted(() => ({
  content: JSON.stringify({ type: "doc", content: [] }),
  sessionTitle: "Weekly sync",
  persistContent: vi.fn(() => Promise.resolve()),
  noteEditorProps: [] as Record<string, unknown>[],
}));

vi.mock("@hypr/editor/markdown", () => ({
  parseJsonContent: (value: string) => JSON.parse(value),
}));

vi.mock("@hypr/editor/note", () => ({
  normalizePortableAttachmentUrls: (value: unknown) => value,
  NoteEditor: (props: Record<string, unknown>) => {
    hoisted.noteEditorProps.push(props);

    return <div>Note editor</div>;
  },
}));

vi.mock("~/session/hooks/useAttachmentResolver", () => ({
  useAttachmentResolver: () => () => null,
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

vi.mock("~/session/queries", () => ({
  useEnhancedNote: () => ({ content: hoisted.content }),
  useUpdateEnhancedNoteContent: () => hoisted.persistContent,
}));

function EnhancedEditor(
  props: Omit<
    React.ComponentProps<typeof SessionEnhancedEditor>,
    "sessionTitle"
  >,
) {
  return (
    <SessionEnhancedEditor {...props} sessionTitle={hoisted.sessionTitle} />
  );
}

describe("EnhancedEditor", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    hoisted.noteEditorProps = [];
    hoisted.content = JSON.stringify({ type: "doc", content: [] });
    hoisted.sessionTitle = "Weekly sync";
    hoisted.persistContent = vi.fn(() => Promise.resolve());
  });

  it("shows the session title as the first line for persisted notes", () => {
    hoisted.content = JSON.stringify({
      type: "doc",
      content: [
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Summary Section" }],
        },
      ],
    });

    render(
      <EnhancedEditor
        sessionId="session-1"
        enhancedNoteId="note-1"
        content={hoisted.content}
      />,
    );

    const props = hoisted.noteEditorProps[hoisted.noteEditorProps.length - 1];

    expect(props?.className).toContain("session-note-editor");
    expect(props?.className).toContain("enhanced-summary-editor");
    expect(props?.placeholderComponent).toEqual(expect.any(Function));
    expect(props?.syncContentWhenFocused).toBe(false);
    expect(props?.handleChange).not.toBe(hoisted.persistContent);
    expect(props?.taskSource).toEqual({ type: "enhanced_note", id: "note-1" });
    expect(props?.initialContent).toMatchObject({
      type: "doc",
      content: [
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Weekly sync" }],
        },
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Summary Section" }],
        },
      ],
    });
  });

  it("does not rerender the editor when its props are unchanged", () => {
    const view = render(
      <EnhancedEditor
        sessionId="session-1"
        enhancedNoteId="note-1"
        content={hoisted.content}
      />,
    );

    view.rerender(
      <EnhancedEditor
        sessionId="session-1"
        enhancedNoteId="note-1"
        content={hoisted.content}
      />,
    );

    expect(hoisted.noteEditorProps).toHaveLength(1);
  });

  it("persists content and updates the session title from the first line", () => {
    render(
      <EnhancedEditor
        sessionId="session-1"
        enhancedNoteId="note-1"
        content={hoisted.content}
      />,
    );

    const props = hoisted.noteEditorProps[hoisted.noteEditorProps.length - 1];
    const input = {
      type: "doc",
      content: [
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Edited title" }],
        },
      ],
    };

    (props?.handleChange as (input: unknown) => void)(input);

    expect(hoisted.persistContent).toHaveBeenCalledWith(
      JSON.stringify(input),
      "Edited title",
    );
  });

  it("keeps streamed previews syncing while focused", () => {
    const contentOverride = {
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "text", text: "Generating" }] },
      ],
    };

    render(
      <EnhancedEditor
        sessionId="session-1"
        enhancedNoteId="note-1"
        content={hoisted.content}
        contentOverride={contentOverride}
      />,
    );

    const props = hoisted.noteEditorProps[hoisted.noteEditorProps.length - 1];

    expect(props?.syncContentWhenFocused).toBe(true);
    expect(props?.handleChange).toBeUndefined();
    expect(props?.taskSource).toBeUndefined();
    expect(props?.initialContent).toMatchObject({
      type: "doc",
      content: [
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Weekly sync" }],
        },
        {
          type: "paragraph",
          content: [{ type: "text", text: "Generating" }],
        },
      ],
    });
  });
});
