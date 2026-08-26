import {
  autoUpdate,
  computePosition,
  flip,
  offset,
  shift,
  type VirtualElement,
} from "@floating-ui/dom";
import {
  useEditorEffect,
  useEditorEventCallback,
  useEditorState,
} from "@handlewithcare/react-prosemirror";
import {
  BoldIcon,
  CodeIcon,
  HighlighterIcon,
  ItalicIcon,
  StrikethroughIcon,
} from "lucide-react";
import { toggleMark } from "prosemirror-commands";
import type { MarkType } from "prosemirror-model";
import type { EditorState } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";

import { cn } from "@hypr/utils";

import { schema } from "../note/schema";

export function selectionTouchesTitleHeading(state: EditorState): boolean {
  const firstNode = state.doc.firstChild;
  if (
    !firstNode ||
    firstNode.type !== state.schema.nodes.heading ||
    firstNode.attrs.level !== 1 ||
    state.selection.empty
  ) {
    return false;
  }

  const titleStart = 1;
  const titleEnd = firstNode.nodeSize - 1;
  const { from, to } = state.selection;

  return from < titleEnd && to > titleStart;
}

function isMarkActive(state: EditorState, type: MarkType): boolean {
  const { from, $from, to, empty } = state.selection;
  if (empty) {
    return !!type.isInSet(state.storedMarks || $from.marks());
  }
  return state.doc.rangeHasMark(from, to, type);
}

const TOOLBAR_BUTTONS: {
  id: string;
  icon: React.ComponentType<{ className?: string }>;
  markType: MarkType;
}[] = [
  { id: "bold", icon: BoldIcon, markType: schema.marks.bold },
  { id: "italic", icon: ItalicIcon, markType: schema.marks.italic },
  { id: "strike", icon: StrikethroughIcon, markType: schema.marks.strike },
  { id: "code", icon: CodeIcon, markType: schema.marks.code },
  { id: "highlight", icon: HighlighterIcon, markType: schema.marks.highlight },
];

export function FormatToolbar() {
  const toolbarRef = useRef<HTMLDivElement>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  const selectionRef = useRef<{
    view: EditorView;
    from: number;
    to: number;
  } | null>(null);
  const selectionReferenceRef = useRef<VirtualElement>({
    getBoundingClientRect: () => {
      const selection = selectionRef.current;
      if (!selection) return new DOMRect();

      const start = selection.view.coordsAtPos(selection.from);
      const end = selection.view.coordsAtPos(selection.to);
      return new DOMRect(
        Math.min(start.left, end.left),
        start.top,
        Math.abs(end.right - start.left),
        end.bottom - start.top,
      );
    },
  });
  const desiredVisibleRef = useRef(false);
  const positionedRef = useRef(false);
  const positionFrameRef = useRef<number | null>(null);
  const afterPaintFrameRef = useRef<number | null>(null);
  const positionRequestRef = useRef(0);

  const editorState = useEditorState();
  const shouldShowToolbar = editorState
    ? !editorState.selection.empty && !selectionTouchesTitleHeading(editorState)
    : false;

  const toggle = useEditorEventCallback((view, markType: MarkType) => {
    if (!view) return;
    toggleMark(markType)(view.state, (tr) => view.dispatch(tr));
    view.focus();
  });

  const schedulePosition = useCallback(() => {
    if (!desiredVisibleRef.current || positionFrameRef.current !== null) return;

    positionFrameRef.current = requestAnimationFrame(() => {
      positionFrameRef.current = null;
      const toolbar = toolbarRef.current;
      if (!toolbar || !desiredVisibleRef.current) return;

      const request = ++positionRequestRef.current;
      void computePosition(selectionReferenceRef.current, toolbar, {
        placement: "top",
        strategy: "fixed",
        middleware: [offset(8), flip(), shift({ padding: 8 })],
      }).then(({ x, y }) => {
        if (
          request !== positionRequestRef.current ||
          !desiredVisibleRef.current
        ) {
          return;
        }

        Object.assign(toolbar.style, {
          left: `${x}px`,
          top: `${y}px`,
          opacity: "1",
          pointerEvents: "auto",
        });
        positionedRef.current = true;

        if (!cleanupRef.current) {
          cleanupRef.current = autoUpdate(
            selectionReferenceRef.current,
            toolbar,
            schedulePosition,
          );
        }
      });
    });
  }, []);

  const scheduleInitialPosition = useCallback(() => {
    if (afterPaintFrameRef.current !== null) return;

    afterPaintFrameRef.current = requestAnimationFrame(() => {
      afterPaintFrameRef.current = null;
      schedulePosition();
    });
  }, [schedulePosition]);

  useEditorEffect((view) => {
    if (!view || !shouldShowToolbar) {
      selectionRef.current = null;
      desiredVisibleRef.current = false;
      positionedRef.current = false;
      positionRequestRef.current++;
      if (positionFrameRef.current !== null) {
        cancelAnimationFrame(positionFrameRef.current);
        positionFrameRef.current = null;
      }
      if (afterPaintFrameRef.current !== null) {
        cancelAnimationFrame(afterPaintFrameRef.current);
        afterPaintFrameRef.current = null;
      }
      if (toolbarRef.current) {
        Object.assign(toolbarRef.current.style, {
          opacity: "0",
          pointerEvents: "none",
        });
      }
      return;
    }

    const { from, to } = view.state.selection;
    selectionRef.current = { view, from, to };
    selectionReferenceRef.current.contextElement = view.dom;
    desiredVisibleRef.current = true;

    if (positionedRef.current) {
      schedulePosition();
    } else {
      scheduleInitialPosition();
    }
  });

  useEffect(
    () => () => {
      desiredVisibleRef.current = false;
      positionRequestRef.current++;
      cleanupRef.current?.();
      if (positionFrameRef.current !== null) {
        cancelAnimationFrame(positionFrameRef.current);
      }
      if (afterPaintFrameRef.current !== null) {
        cancelAnimationFrame(afterPaintFrameRef.current);
      }
    },
    [],
  );

  if (!editorState) return null;

  return createPortal(
    <div
      ref={toolbarRef}
      aria-hidden={!shouldShowToolbar}
      className={cn([
        "border-border bg-card/95 fixed flex items-center gap-0.5 rounded-lg border p-1",
        "shadow-[0_2px_8px_rgba(0,0,0,0.08),0_18px_42px_-16px_rgba(0,0,0,0.34)] backdrop-blur-sm",
      ])}
      style={{
        top: 0,
        left: 0,
        zIndex: 40,
        opacity: 0,
        pointerEvents: "none",
      }}
      onMouseDown={(e) => e.preventDefault()}
    >
      {TOOLBAR_BUTTONS.map((button) => {
        const active =
          shouldShowToolbar && isMarkActive(editorState, button.markType);
        return (
          <button
            key={button.id}
            tabIndex={shouldShowToolbar ? 0 : -1}
            className={cn([
              "flex size-8 items-center justify-center rounded-md",
              "cursor-pointer border-none transition-colors",
              active
                ? "bg-accent text-foreground"
                : "text-muted-foreground hover:bg-accent bg-transparent",
            ])}
            onClick={() => toggle(button.markType)}
          >
            <button.icon className="size-4" />
          </button>
        );
      })}
    </div>,
    document.body,
  );
}
