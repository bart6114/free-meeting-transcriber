import type { Node as PMNode } from "prosemirror-model";
import {
  Plugin,
  PluginKey,
  type Selection,
  TextSelection,
} from "prosemirror-state";

import {
  getChangedRanges,
  type ChangedRange,
  hasChangedRange,
} from "./changed-ranges";

type JSONContent = {
  type?: string;
  content?: JSONContent[];
};

export function ensureImageTrailingParagraphs<T extends JSONContent>(
  content: T,
): T {
  if (content.type !== "doc" || !content.content) return content;

  const next: JSONContent[] = [];
  for (let i = 0; i < content.content.length; i++) {
    const child = content.content[i];
    next.push(child);
    if (child.type === "image") {
      const after = content.content[i + 1];
      if (!after || after.type !== "paragraph") {
        next.push({ type: "paragraph" });
      }
    }
  }
  if (next.length === content.content.length) return content;
  return { ...content, content: next as T["content"] };
}

export function imageTrailingParagraphPlugin() {
  return new Plugin({
    key: new PluginKey("imageTrailingParagraph"),
    appendTransaction(transactions, _oldState, newState) {
      if (!transactions.some((tr) => tr.docChanged)) return null;

      const { doc, schema } = newState;
      const imageType = schema.nodes.image;
      const paragraphType = schema.nodes.paragraph;
      if (!imageType || !paragraphType) return null;

      const changedRanges = getChangedRanges(transactions);
      const insertions = getImageTrailingParagraphInsertions(
        doc,
        changedRanges,
      );
      const selectionImageEnd = getChangedImageEndAfterSelection(
        doc,
        newState.selection,
        changedRanges,
      );

      if (insertions.length === 0 && selectionImageEnd === null) return null;

      const tr = newState.tr;
      for (let i = insertions.length - 1; i >= 0; i--) {
        tr.insert(insertions[i], paragraphType.create());
      }
      if (selectionImageEnd !== null) {
        const paragraphStart = tr.mapping.map(selectionImageEnd, -1);
        tr.setSelection(
          TextSelection.create(tr.doc, paragraphStart + 1),
        ).scrollIntoView();
      }
      return tr;
    },
  });
}

function getChangedImageEndAfterSelection(
  doc: PMNode,
  selection: Selection,
  changedRanges: ChangedRange[],
) {
  const { $from } = selection;
  if (
    !selection.empty ||
    !$from.parent.isTextblock ||
    $from.parentOffset !== $from.parent.content.size
  ) {
    return null;
  }

  const imagePos = $from.after($from.depth);
  const image = doc.nodeAt(imagePos);
  return image?.type === doc.type.schema.nodes.image &&
    hasChangedRange(changedRanges, imagePos, imagePos + image.nodeSize)
    ? imagePos + image.nodeSize
    : null;
}

function getImageTrailingParagraphInsertions(
  doc: PMNode,
  ranges: ChangedRange[],
) {
  const imageType = doc.type.schema.nodes.image;
  const paragraphType = doc.type.schema.nodes.paragraph;
  if (!imageType || !paragraphType) return [];

  const insertions = new Set<number>();
  const inspectImage = (
    node: PMNode | null | undefined,
    pos: number,
    index: number,
  ) => {
    if (!node || node.type !== imageType) return;
    const next = doc.maybeChild(index + 1);
    if (!next || next.type !== paragraphType) {
      insertions.add(pos + node.nodeSize);
    }
  };

  const inspectAround = (pos: number) => {
    const clamped = Math.max(0, Math.min(pos, doc.content.size));
    const before = doc.childBefore(clamped);
    if (before.node) {
      inspectImage(before.node, before.offset, before.index);
    }

    const after = doc.childAfter(clamped);
    if (after.node) {
      inspectImage(after.node, after.offset, after.index);
    }
  };

  for (const range of ranges) {
    inspectAround(range.from);
    inspectAround(range.to);

    if (range.from >= range.to) {
      continue;
    }

    doc.nodesBetween(range.from, range.to, (node, pos, parent, index) => {
      if (parent !== doc || typeof index !== "number") {
        return true;
      }

      inspectImage(node, pos, index);
      return false;
    });
  }

  return Array.from(insertions).sort((a, b) => a - b);
}
