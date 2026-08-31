// @vitest-environment jsdom

import { waitFor } from "@testing-library/react";
import { EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { afterEach, describe, expect, it, vi } from "vitest";

import { schema } from "../note/schema";
import {
  type FileHandlerConfig,
  fileHandlerPlugin,
  handleFileDrop,
} from "./file-handler";

const views: EditorView[] = [];

afterEach(() => {
  for (const view of views.splice(0)) {
    view.destroy();
  }
});

describe("handleFileDrop", () => {
  it("inserts GIF files as images", async () => {
    const config = { onFileUpload: createUploader() };
    const view = createView(config);
    const file = new File(["gif"], "motion.gif", { type: "image/gif" });

    expect(
      handleFileDrop(view, config, [file], view.state.doc.content.size),
    ).toBe(true);

    await waitFor(() => expect(view.state.doc.childCount).toBe(2));
    expect(view.state.doc.child(1).type).toBe(schema.nodes.image);
    expect(view.state.doc.child(1).attrs).toMatchObject({
      attachmentId: "motion.gif",
      src: "asset:/motion.gif",
    });
  });

  it("inserts arbitrary files as attachment cards in drop order", async () => {
    const config = { onFileUpload: createUploader() };
    const view = createView(config);
    const files = [
      new File(["pdf"], "brief.pdf", { type: "application/pdf" }),
      new File(["print('hi')"], "script.py"),
    ];

    handleFileDrop(view, config, files, view.state.doc.content.size);

    await waitFor(() => expect(view.state.doc.childCount).toBe(3));
    expect(view.state.doc.child(1).type).toBe(schema.nodes.fileAttachment);
    expect(view.state.doc.child(1).attrs).toMatchObject({
      name: "brief.pdf",
      mimeType: "application/pdf",
    });
    expect(view.state.doc.child(2).attrs).toMatchObject({
      name: "script.py",
      mimeType: "",
    });
  });

  it("inserts only files remaining after host drop handling", async () => {
    const audio = new File(["audio"], "clip.mp3", { type: "audio/mpeg" });
    const attachment = new File(["code"], "script.py");
    const onFileUpload = createUploader();
    const config = {
      onDrop: () => ({ remainingFiles: [attachment] }),
      onFileUpload,
    };
    const view = createView(config);

    handleFileDrop(
      view,
      config,
      [audio, attachment],
      view.state.doc.content.size,
    );

    await waitFor(() => expect(view.state.doc.childCount).toBe(2));
    expect(onFileUpload).toHaveBeenCalledOnce();
    expect(onFileUpload).toHaveBeenCalledWith(
      { kind: "file", file: attachment, name: attachment.name },
      expect.any(String),
    );
  });

  it("reports upload failures without inserting a node", async () => {
    const error = new Error("disk full");
    const file = new File(["code"], "script.py");
    const onFileUploadError = vi.fn();
    const config = {
      onFileUpload: vi.fn().mockRejectedValue(error),
      onFileUploadError,
    };
    const view = createView(config);

    handleFileDrop(view, config, [file], view.state.doc.content.size);

    await waitFor(() =>
      expect(onFileUploadError).toHaveBeenCalledWith(
        { kind: "file", file, name: file.name },
        error,
      ),
    );
    expect(view.state.doc.childCount).toBe(1);
    expect(
      document.querySelector("[data-file-upload-placeholder]"),
    ).not.toBeNull();
  });

  it("renders a placeholder synchronously and maps it through edits", async () => {
    let resolveUpload!: (value: {
      attachmentId: string;
      path: string;
      url: string;
    }) => void;
    const config = {
      onFileUpload: vi.fn(
        () =>
          new Promise<{
            attachmentId: string;
            path: string;
            url: string;
          }>((resolve) => {
            resolveUpload = resolve;
          }),
      ),
    };
    const view = createView(config);
    const file = new File(["pdf"], "brief.pdf", { type: "application/pdf" });

    handleFileDrop(view, config, [file], view.state.doc.content.size);

    expect(
      document.querySelector("[data-file-upload-placeholder]")?.textContent,
    ).toContain("brief.pdf");
    view.dispatch(view.state.tr.insertText("before", 1));
    resolveUpload({
      attachmentId: "brief.pdf",
      path: "/vault/attachments/brief.pdf",
      url: "asset:/brief.pdf",
    });

    await waitFor(() => expect(view.state.doc.childCount).toBe(2));
    expect(view.state.doc.textContent).toContain("before");
    expect(view.state.doc.child(1).attrs.attachmentId).toBe("brief.pdf");
  });
});

function createView(config: FileHandlerConfig) {
  const host = document.createElement("div");
  document.body.append(host);
  const state = EditorState.create({
    schema,
    doc: schema.node("doc", null, [schema.node("paragraph")]),
    plugins: [fileHandlerPlugin(config)],
  });
  const view = new EditorView(host, {
    state,
    dispatchTransaction(transaction) {
      view.updateState(view.state.apply(transaction));
    },
  });
  views.push(view);
  return view;
}

function createUploader() {
  return vi.fn(async (candidate: { name: string }) => ({
    attachmentId: candidate.name,
    path: `/vault/attachments/${candidate.name}`,
    url: `asset:/${candidate.name}`,
  }));
}
