// @vitest-environment jsdom

import { waitFor } from "@testing-library/react";
import { EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { afterEach, describe, expect, it, vi } from "vitest";

import { schema } from "../note/schema";
import { handleFileDrop } from "./file-handler";

const views: EditorView[] = [];

afterEach(() => {
  for (const view of views.splice(0)) {
    view.destroy();
  }
});

describe("handleFileDrop", () => {
  it("inserts GIF files as images", async () => {
    const view = createView();
    const file = new File(["gif"], "motion.gif", { type: "image/gif" });

    expect(
      handleFileDrop(
        view,
        { onFileUpload: createUploader() },
        [file],
        view.state.doc.content.size,
      ),
    ).toBe(true);

    await waitFor(() => expect(view.state.doc.childCount).toBe(2));
    expect(view.state.doc.child(1).type).toBe(schema.nodes.image);
    expect(view.state.doc.child(1).attrs).toMatchObject({
      attachmentId: "motion.gif",
      src: "asset:/motion.gif",
    });
  });

  it("inserts arbitrary files as attachment cards in drop order", async () => {
    const view = createView();
    const files = [
      new File(["pdf"], "brief.pdf", { type: "application/pdf" }),
      new File(["print('hi')"], "script.py"),
    ];

    handleFileDrop(
      view,
      { onFileUpload: createUploader() },
      files,
      view.state.doc.content.size,
    );

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
    const view = createView();
    const audio = new File(["audio"], "clip.mp3", { type: "audio/mpeg" });
    const attachment = new File(["code"], "script.py");
    const onFileUpload = createUploader();

    handleFileDrop(
      view,
      {
        onDrop: () => ({ remainingFiles: [attachment] }),
        onFileUpload,
      },
      [audio, attachment],
      view.state.doc.content.size,
    );

    await waitFor(() => expect(view.state.doc.childCount).toBe(2));
    expect(onFileUpload).toHaveBeenCalledOnce();
    expect(onFileUpload).toHaveBeenCalledWith(attachment);
  });

  it("reports upload failures without inserting a node", async () => {
    const view = createView();
    const error = new Error("disk full");
    const file = new File(["code"], "script.py");
    const onFileUploadError = vi.fn();

    handleFileDrop(
      view,
      {
        onFileUpload: vi.fn().mockRejectedValue(error),
        onFileUploadError,
      },
      [file],
      view.state.doc.content.size,
    );

    await waitFor(() =>
      expect(onFileUploadError).toHaveBeenCalledWith(file, error),
    );
    expect(view.state.doc.childCount).toBe(1);
  });
});

function createView() {
  const host = document.createElement("div");
  document.body.append(host);
  const state = EditorState.create({
    schema,
    doc: schema.node("doc", null, [schema.node("paragraph")]),
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
  return vi.fn(async (file: File) => ({
    attachmentId: file.name,
    path: `/vault/attachments/${file.name}`,
    url: `asset:/${file.name}`,
  }));
}
