import { describe, expect, it } from "vitest";

import { json2md, md2json } from "@hypr/editor/markdown";

// Guards the file-canonical note path end to end: `raw.tsx`'s load converts a `notes.md` file's
// markdown into editor JSON via `md2json` (see `useSessionRawMd`), and its save converts the
// edited JSON back to markdown via `json2md` (see `persistChange`) before calling
// `sessionWriteNote`. A fixture covering headings, bold, lists, and a code fence together (the
// shapes real meeting notes actually mix) pins that this round trip is byte-stable -- if TipTap
// or the markdown serializer ever reorders/reformats something, this test is the tripwire.
const MEMO_FIXTURE = `# Meeting Notes

## Action Items

This is a **bold** statement about the plan.

- First item
- Second item
- Third item

1. Step one
2. Step two

\`\`\`js
const x = 1;
console.log(x);
\`\`\`

Some trailing text.`;

// Pinned normalization: `json2md`'s list serializer (prosemirror-markdown's default
// `renderList`) always blank-line-separates list items on the way out, even when the source
// used single-line-separated ("tight") items -- there is no tight/loose distinction in this
// schema. A hand-written or externally-synced `notes.md` with tight lists is NOT byte-stable
// through one editor load/save cycle; it converges to this loose form and stays there.
const MEMO_FIXTURE_AFTER_SAVE = `# Meeting Notes

## Action Items

This is a **bold** statement about the plan.

- First item

- Second item

- Third item

1. Step one

2. Step two

\`\`\`js
const x = 1;
console.log(x);
\`\`\`

Some trailing text.`;

describe("notes.md round trip (load -> editor JSON -> save)", () => {
  it("loose-ifies tight lists on the first save (documented normalization, not data loss)", () => {
    const editorContent = md2json(MEMO_FIXTURE);
    const savedMarkdown = json2md(editorContent);

    expect(savedMarkdown).toBe(MEMO_FIXTURE_AFTER_SAVE);
  });

  it("is byte-stable once a file has already been through one save (steady state)", () => {
    const editorContent = md2json(MEMO_FIXTURE_AFTER_SAVE);
    const savedMarkdown = json2md(editorContent);

    expect(savedMarkdown).toBe(MEMO_FIXTURE_AFTER_SAVE);
  });

  it("preserves heading levels, marks, and list structure through the editor JSON", () => {
    const editorContent = md2json(MEMO_FIXTURE);
    const [h1, h2, bold, bulletList, orderedList, codeBlock] =
      editorContent.content ?? [];

    expect(h1).toMatchObject({ type: "heading", attrs: { level: 1 } });
    expect(h2).toMatchObject({ type: "heading", attrs: { level: 2 } });
    expect(
      bold?.content?.some((node) =>
        node.marks?.some((mark) => mark.type === "bold"),
      ),
    ).toBe(true);
    expect(bulletList).toMatchObject({ type: "bulletList" });
    expect(bulletList?.content).toHaveLength(3);
    expect(orderedList).toMatchObject({ type: "orderedList" });
    expect(orderedList?.content).toHaveLength(2);
    expect(codeBlock).toMatchObject({
      type: "codeBlock",
      attrs: { language: "js" },
    });
  });
});
