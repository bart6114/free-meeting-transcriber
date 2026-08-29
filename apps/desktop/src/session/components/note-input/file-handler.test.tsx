import {
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { EditorView } from "prosemirror-view";
import { useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FileHandlerConfig, NoteEditorRef } from "@hypr/editor/note";

import { FileDropTarget } from "./file-drop-target";
import { useNoteFileHandlerConfig } from "./file-handler";

const mocks = vi.hoisted(() => ({
  fileUpload: vi.fn(),
  focusWindow: vi.fn(),
  handleFileDrop: vi.fn(),
  processAudioFile: vi.fn(),
  showWindow: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    setFocus: mocks.focusWindow,
    show: mocks.showWindow,
  }),
}));

vi.mock("@hypr/editor/note", () => ({
  handleFileDrop: mocks.handleFileDrop,
}));

vi.mock("@hypr/ui/components/ui/toast", () => ({
  sonnerToast: { error: mocks.toastError },
}));

vi.mock("~/shared/hooks/useFileUpload", () => ({
  useFileUpload: () => mocks.fileUpload,
}));

vi.mock("~/stt/useUploadFile", () => ({
  AUDIO_EXTENSIONS: ["wav", "mp3", "m4a"],
  isAudioUploadFile: (file: Pick<File, "name" | "type">) =>
    file.type.startsWith("audio/") || /\.(wav|mp3|m4a)$/i.test(file.name),
  useUploadFile: () => ({ processAudioFile: mocks.processAudioFile }),
}));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.showWindow.mockResolvedValue(undefined);
  mocks.focusWindow.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe("useNoteFileHandlerConfig", () => {
  it("shows the attachment hint for a Finder file drag before files are readable", async () => {
    render(<Harness />);
    const target = screen.getByTestId("drop-target");
    const dataTransfer = createDataTransfer([], [""]);

    fireEvent.dragEnter(target, { dataTransfer });

    expect(screen.getByRole("status").className).toContain("bg-background/30");
    expect(
      screen.getByText("Drop files here to attach to note"),
    ).not.toBeNull();
    expect(dataTransfer.dropEffect).toBe("copy");
    await waitFor(() => expect(mocks.focusWindow).toHaveBeenCalledOnce());
  });

  it("shows adaptive hints for audio and mixed drags", () => {
    const view = render(<Harness />);
    const target = screen.getByTestId("drop-target");
    const audio = new File(["audio"], "clip.mp3", { type: "audio/mpeg" });

    fireEvent.dragEnter(target, {
      dataTransfer: createDataTransfer([audio]),
    });
    expect(
      screen.getByText("Drop to upload and transcribe audio"),
    ).not.toBeNull();

    view.unmount();
    render(<Harness />);
    const mixedTarget = screen.getByTestId("drop-target");
    fireEvent.dragEnter(mixedTarget, {
      dataTransfer: createDataTransfer([
        audio,
        new File(["code"], "script.py"),
      ]),
    });
    expect(
      screen.getByText(
        "Audio will be transcribed; other files will be attached.",
      ),
    ).not.toBeNull();
  });

  it("keeps the overlay active across nested drag boundaries", () => {
    render(<Harness />);
    const target = screen.getByTestId("drop-target");
    const dataTransfer = createDataTransfer([new File(["code"], "script.py")]);

    fireEvent.dragEnter(target, { dataTransfer });
    fireEvent.dragEnter(target, { dataTransfer });
    fireEvent.dragLeave(target, { dataTransfer });

    expect(screen.getByRole("status")).not.toBeNull();

    fireEvent.dragLeave(target, { dataTransfer });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("uses the pointer position and appends when the pointer is outside the editor", () => {
    const mappedView = createEditorView(7);
    const view = render(<Harness view={mappedView} />);
    const file = new File(["code"], "script.py");
    const mappedTransfer = createDataTransfer([file]);

    fireEvent.drop(screen.getByTestId("drop-target"), {
      clientX: 12,
      clientY: 34,
      dataTransfer: mappedTransfer,
    });
    expect(mocks.handleFileDrop).toHaveBeenLastCalledWith(
      mappedView,
      expect.any(Object),
      [file],
      7,
      mappedTransfer.items,
    );

    view.unmount();
    const blankView = createEditorView(null);
    render(<Harness view={blankView} />);
    const blankTransfer = createDataTransfer([file]);
    const dropEvent = createEvent.drop(screen.getByTestId("drop-target"), {
      dataTransfer: blankTransfer,
    });
    fireEvent(screen.getByTestId("drop-target"), dropEvent);

    expect(dropEvent.defaultPrevented).toBe(true);
    expect(mocks.handleFileDrop).toHaveBeenLastCalledWith(
      blankView,
      expect.any(Object),
      [file],
      42,
      blankTransfer.items,
    );
  });

  it("routes audio through the shared drop config exactly once", () => {
    mocks.handleFileDrop.mockImplementation(
      (
        _view: EditorView,
        config: FileHandlerConfig,
        files: File[],
        pos: number,
        items: DataTransferItemList,
      ) => config.onDrop?.(files, pos, items),
    );
    render(<Harness />);
    const audio = new File(["audio"], "clip.mp3", { type: "audio/mpeg" });

    fireEvent.drop(screen.getByTestId("drop-target"), {
      dataTransfer: createDataTransfer([audio]),
    });

    expect(mocks.processAudioFile).toHaveBeenCalledOnce();
    expect(mocks.processAudioFile).toHaveBeenCalledWith(audio);
  });

  it("uses drag item MIME when routing mixed audio and attachments", () => {
    mocks.handleFileDrop.mockImplementation(
      (
        _view: EditorView,
        config: FileHandlerConfig,
        files: File[],
        pos: number,
        items: DataTransferItemList,
      ) => config.onDrop?.(files, pos, items),
    );
    render(<Harness />);
    const audio = new File(["audio"], "clip");
    const attachment = new File(["code"], "script.py");

    fireEvent.drop(screen.getByTestId("drop-target"), {
      dataTransfer: createDataTransfer(
        [audio, attachment],
        ["audio/mpeg", "text/x-python"],
      ),
    });

    expect(mocks.processAudioFile).toHaveBeenCalledWith(audio, {
      allowUnknownAudio: true,
      contentType: "audio/mpeg",
    });
    expect(mocks.handleFileDrop.mock.results[0]?.value).toEqual({
      remainingFiles: [attachment],
    });
  });

  it("reports unreadable drops and upload failures", () => {
    const view = render(<Harness />);
    fireEvent.drop(screen.getByTestId("drop-target"), {
      dataTransfer: createDataTransfer([], [""]),
    });
    expect(mocks.toastError).toHaveBeenCalledWith(
      "Loofah couldn’t access the dropped files",
      expect.any(Object),
    );

    view.unmount();
    mocks.handleFileDrop.mockImplementation(
      (_view: EditorView, config: FileHandlerConfig, files: File[]) => {
        config.onFileUploadError?.(files[0], new Error("disk full"));
        return true;
      },
    );
    render(<Harness />);
    const file = new File(["code"], "script.py");
    fireEvent.drop(screen.getByTestId("drop-target"), {
      dataTransfer: createDataTransfer([file]),
    });
    expect(mocks.toastError).toHaveBeenLastCalledWith(
      "Couldn’t attach “script.py”: disk full",
      expect.any(Object),
    );
  });

  it("ignores non-file drags", () => {
    render(<Harness />);
    fireEvent.dragEnter(screen.getByTestId("drop-target"), {
      dataTransfer: {
        dropEffect: "none",
        files: [],
        items: [],
        types: ["text/plain"],
      },
    });

    expect(screen.queryByRole("status")).toBeNull();
    expect(mocks.focusWindow).not.toHaveBeenCalled();
  });
});

function Harness({ view = createEditorView(null) }: { view?: EditorView }) {
  const editorRef = useRef<NoteEditorRef>({
    commands: {} as NoteEditorRef["commands"],
    flushPendingChanges: vi.fn(),
    view,
  });
  const { fileDragKind, fileDropTargetProps } = useNoteFileHandlerConfig(
    "session-1",
    editorRef,
  );

  return (
    <div data-testid="drop-target" {...fileDropTargetProps}>
      <FileDropTarget kind={fileDragKind} />
    </div>
  );
}

function createEditorView(pos: number | null) {
  return {
    posAtCoords: vi.fn(() => (pos === null ? null : { inside: 0, pos })),
    state: { doc: { content: { size: 42 } } },
  } as unknown as EditorView;
}

function createDataTransfer(files: File[], itemTypes?: string[]) {
  const types = itemTypes ?? files.map((file) => file.type);
  const itemCount = Math.max(files.length, types.length);
  return {
    dropEffect: "none",
    files,
    items: Array.from({ length: itemCount }, (_, index) => ({
      getAsFile: () => files[index] ?? null,
      kind: "file",
      type: types[index] ?? files[index]?.type ?? "",
    })),
    types: ["Files"],
  } as unknown as DataTransfer;
}
