import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  type DragEvent,
  type HTMLAttributes,
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import {
  type FileHandlerConfig,
  handleFileDrop,
  type NoteEditorRef,
} from "@hypr/editor/note";
import { sonnerToast } from "@hypr/ui/components/ui/toast";

import { useFileUpload } from "~/shared/hooks/useFileUpload";
import { isAudioUploadFile, useUploadFile } from "~/stt/useUploadFile";

export type NoteFileDragKind = "audio" | "files" | "mixed";

export function useNoteFileHandlerConfig(
  sessionId: string,
  editorRef: RefObject<NoteEditorRef | null>,
) {
  const onFileUpload = useFileUpload(sessionId);
  const { processAudioFile } = useUploadFile(sessionId);
  const [fileDragKind, setFileDragKind] = useState<NoteFileDragKind | null>(
    null,
  );
  const fileDragDepthRef = useRef(0);

  const processAudioDrop = useCallback(
    (files: File[], items?: DataTransferItemList) => {
      const audioDrop = getAudioDrop(files, items);
      if (!audioDrop) {
        return null;
      }

      if (audioDrop.allowUnknownAudio) {
        processAudioFile(audioDrop.audioFile, {
          allowUnknownAudio: true,
          contentType: audioDrop.contentType,
        });
      } else {
        processAudioFile(audioDrop.audioFile);
      }
      return { remainingFiles: audioDrop.remainingFiles };
    },
    [processAudioFile],
  );

  const handleDrop = useCallback(
    (files: File[], _pos?: number, items?: DataTransferItemList) => {
      const result = processAudioDrop(files, items);
      if (!result) {
        return undefined;
      }

      return result.remainingFiles.length === 0 ? true : result;
    },
    [processAudioDrop],
  );

  const handlePaste = useCallback(
    (files: File[], items?: DataTransferItemList) =>
      handleDrop(files, undefined, items),
    [handleDrop],
  );

  const handleFileUploadError = useCallback(
    (file: File, error: unknown) => {
      const detail = error instanceof Error ? `: ${error.message}` : "";
      sonnerToast.error(`Couldn’t attach “${file.name}”${detail}`, {
        id: `attachment-upload-failed:${sessionId}`,
      });
    },
    [sessionId],
  );

  const fileHandlerConfig = useMemo<FileHandlerConfig>(
    () => ({
      onFileUpload,
      onFileUploadError: handleFileUploadError,
      onDrop: handleDrop,
      onPaste: handlePaste,
    }),
    [handleDrop, handleFileUploadError, handlePaste, onFileUpload],
  );

  const resetFileDrag = useCallback(() => {
    fileDragDepthRef.current = 0;
    setFileDragKind(null);
  }, []);

  useEffect(() => {
    resetFileDrag();
    return resetFileDrag;
  }, [resetFileDrag, sessionId]);

  const prepareFileDragEvent = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      event.preventDefault();
      event.stopPropagation();
      event.dataTransfer.dropEffect = "copy";
      setFileDragKind(classifyFileDrag(event.dataTransfer));
    },
    [],
  );

  const handleDragEnterCapture = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (
        !editorRef.current?.view ||
        !hasExternalFileDrag(event.dataTransfer)
      ) {
        return;
      }

      if (fileDragDepthRef.current === 0) {
        focusCurrentWindowForFileDrop();
      }

      fileDragDepthRef.current += 1;
      prepareFileDragEvent(event);
    },
    [editorRef, prepareFileDragEvent],
  );

  const handleDragOverCapture = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (
        !editorRef.current?.view ||
        (fileDragDepthRef.current === 0 &&
          !hasExternalFileDrag(event.dataTransfer))
      ) {
        return;
      }

      prepareFileDragEvent(event);
    },
    [editorRef, prepareFileDragEvent],
  );

  const handleDragLeaveCapture = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (
        fileDragDepthRef.current === 0 &&
        !hasExternalFileDrag(event.dataTransfer)
      ) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      fileDragDepthRef.current = Math.max(0, fileDragDepthRef.current - 1);
      if (fileDragDepthRef.current === 0) {
        setFileDragKind(null);
      }
    },
    [],
  );

  const handleDropCapture = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (!hasExternalFileDrag(event.dataTransfer)) {
        return;
      }

      const view = editorRef.current?.view;
      if (!view) {
        resetFileDrag();
        return;
      }

      const files = Array.from(event.dataTransfer.files ?? []);
      event.preventDefault();
      event.stopPropagation();
      resetFileDrag();

      if (files.length === 0) {
        sonnerToast.error("Loofah couldn’t access the dropped files", {
          id: `attachment-upload-failed:${sessionId}`,
        });
        return;
      }

      const pos =
        view.posAtCoords({
          left: event.clientX,
          top: event.clientY,
        })?.pos ?? view.state.doc.content.size;
      handleFileDrop(
        view,
        fileHandlerConfig,
        files,
        pos,
        event.dataTransfer.items,
      );
    },
    [editorRef, fileHandlerConfig, resetFileDrag, sessionId],
  );

  const fileDropTargetProps = useMemo<HTMLAttributes<HTMLDivElement>>(
    () => ({
      onDragEnterCapture: handleDragEnterCapture,
      onDragOverCapture: handleDragOverCapture,
      onDragLeaveCapture: handleDragLeaveCapture,
      onDropCapture: handleDropCapture,
      onDragEndCapture: resetFileDrag,
    }),
    [
      handleDragEnterCapture,
      handleDragLeaveCapture,
      handleDragOverCapture,
      handleDropCapture,
      resetFileDrag,
    ],
  );

  return useMemo(
    () => ({
      fileDragKind,
      fileDropTargetProps,
      fileHandlerConfig,
      resetFileDrag,
    }),
    [fileDragKind, fileDropTargetProps, fileHandlerConfig, resetFileDrag],
  );
}

function hasExternalFileDrag(dataTransfer: DataTransfer) {
  if (Array.from(dataTransfer.types ?? []).includes("Files")) {
    return true;
  }

  if (
    Array.from(dataTransfer.items ?? []).some((item) => item.kind === "file")
  ) {
    return true;
  }

  return Array.from(dataTransfer.files ?? []).length > 0;
}

function classifyFileDrag(dataTransfer: DataTransfer): NoteFileDragKind {
  const items = Array.from(dataTransfer.items ?? []);
  const files = Array.from(dataTransfer.files ?? []);
  const fileItems = items.filter((item) => item.kind === "file");
  const fileCount = fileItems.length || files.length;
  const audioCount =
    fileItems.length > 0
      ? fileItems.filter((item, index) => {
          if (item.type.startsWith("audio/")) {
            return true;
          }

          const file = files[index] ?? item.getAsFile();
          return file ? isAudioUploadFile(file) : false;
        }).length
      : files.filter(isAudioUploadFile).length;

  if (fileCount === 1 && audioCount === 1) {
    return "audio";
  }
  if (audioCount > 0) {
    return "mixed";
  }
  return "files";
}

function getAudioDrop(files: File[], items?: DataTransferItemList) {
  const dataTransferItems = Array.from(items ?? []).filter(
    (item) => item.kind === "file",
  );
  const audioFileIndex = files.findIndex((file, index) =>
    isAudioDropFile(file, dataTransferItems[index]),
  );
  if (audioFileIndex === -1) {
    return null;
  }
  const audioFile = files[audioFileIndex];

  return {
    allowUnknownAudio: !isAudioUploadFile(audioFile),
    audioFile,
    contentType:
      audioFile.type || dataTransferItems[audioFileIndex]?.type || undefined,
    remainingFiles: files.filter((file) => file !== audioFile),
  };
}

function isAudioDropFile(file: File, item?: DataTransferItem) {
  return isAudioUploadFile(file) || item?.type.startsWith("audio/") === true;
}

function focusCurrentWindowForFileDrop() {
  if (!isTauri()) {
    return;
  }

  void bringCurrentWindowToFront();
}

async function bringCurrentWindowToFront() {
  try {
    const currentWindow = getCurrentWindow();
    await currentWindow.show();
    await currentWindow.setFocus();
  } catch (error) {
    console.error("Failed to focus window for file drop", error);
  }
}
