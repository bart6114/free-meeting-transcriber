import { EditorState } from "prosemirror-state";
import { describe, expect, it } from "vitest";

import { schema } from "./schema";
import { normalizeTitleHeadingDoc, titleHeadingPlugin } from "./title-layout";

describe("normalizeTitleHeadingDoc", () => {
  it("keeps a body paragraph after an empty title", () => {
    const doc = schema.node("doc", null, [schema.node("paragraph")]);

    expect(normalizeTitleHeadingDoc(doc).toJSON()).toEqual({
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 1 } },
        { type: "paragraph" },
      ],
    });
  });

  it("converts the first paragraph into a title heading", () => {
    const doc = schema.node("doc", null, [
      schema.node("paragraph", null, [schema.text("Planning")]),
      schema.node("paragraph", null, [schema.text("Follow up")]),
    ]);

    expect(normalizeTitleHeadingDoc(doc).toJSON()).toEqual({
      type: "doc",
      content: [
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Planning" }],
        },
        {
          type: "paragraph",
          content: [{ type: "text", text: "Follow up" }],
        },
      ],
    });
  });

  it("inserts an empty title heading before non-text blocks", () => {
    const doc = schema.node("doc", null, [
      schema.node("bulletList", null, [
        schema.node("listItem", null, [
          schema.node("paragraph", null, [schema.text("Follow up")]),
        ]),
      ]),
    ]);

    expect(normalizeTitleHeadingDoc(doc).toJSON()).toMatchObject({
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 1 } },
        { type: "bulletList" },
      ],
    });
  });
});

describe("titleHeadingPlugin", () => {
  it("restores a body paragraph when only an empty title remains", () => {
    const doc = schema.node("doc", null, [
      schema.node("heading", { level: 1 }, [schema.text("Planning")]),
      schema.node("paragraph", null, [schema.text("Follow up")]),
    ]);
    const state = EditorState.create({
      schema,
      doc,
      plugins: [titleHeadingPlugin()],
    });

    const result = state.applyTransaction(
      state.tr.delete(1, state.doc.content.size),
    );

    expect(result.state.doc.toJSON()).toEqual({
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 1 } },
        { type: "paragraph" },
      ],
    });
  });

  it("restores the first block to a title heading after edits", () => {
    const doc = schema.node("doc", null, [
      schema.node("heading", { level: 1 }, [schema.text("Planning")]),
      schema.node("paragraph", null, [schema.text("Follow up")]),
    ]);
    const state = EditorState.create({
      schema,
      doc,
      plugins: [titleHeadingPlugin()],
    });

    const result = state.applyTransaction(
      state.tr.setNodeMarkup(0, schema.nodes.paragraph),
    );

    expect(result.state.doc.firstChild?.type).toBe(schema.nodes.heading);
    expect(result.state.doc.firstChild?.attrs.level).toBe(1);
  });
});

describe("titleTrailerPlugin", () => {
  it("places the host element as a widget directly after the title node", async () => {
    const { titleTrailerPlugin } = await import("./title-layout");
    const element = document.createElement("div");
    const plugin = titleTrailerPlugin(element);
    const doc = schema.node("doc", null, [
      schema.node("heading", { level: 1 }, [schema.text("Planning")]),
      schema.node("paragraph", null, [schema.text("Body")]),
    ]);
    const state = EditorState.create({ doc, plugins: [plugin] });

    expect(element.contentEditable).toBe("false");

    const decorations = plugin.props.decorations?.call(plugin, state);
    expect(decorations).toBeTruthy();
    const found = (
      decorations as import("prosemirror-view").DecorationSet
    ).find();
    expect(found).toHaveLength(1);
    expect(found[0]?.from).toBe(doc.firstChild!.nodeSize);
    const spec = (
      found[0] as unknown as { spec: { stopEvent?: () => boolean } }
    ).spec;
    expect(spec.stopEvent?.()).toBe(true);
  });
});
