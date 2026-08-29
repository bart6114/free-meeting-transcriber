import { Plugin, PluginKey } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";

export type FileUploadResult = {
  url: string;
  attachmentId: string;
  path: string;
};

export type FileDropResult = boolean | void | { remainingFiles: File[] };

export type FileHandlerConfig = {
  onDrop?: (
    files: File[],
    pos?: number,
    items?: DataTransferItemList,
  ) => FileDropResult;
  onPaste?: (files: File[], items?: DataTransferItemList) => FileDropResult;
  onFileUpload?: (file: File) => Promise<FileUploadResult>;
  onFileUploadError?: (file: File, error: unknown) => void;
};

const IMAGE_MIME_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

function isImageFile(file: File) {
  return IMAGE_MIME_TYPES.includes(file.type);
}

function isFileDropRemainder(
  result: FileDropResult,
): result is { remainingFiles: File[] } {
  return Boolean(
    result &&
    typeof result === "object" &&
    "remainingFiles" in result &&
    Array.isArray(result.remainingFiles),
  );
}

function insertImage(
  view: EditorView,
  url: string,
  attachmentId: string | null,
  pos?: number,
) {
  const imageType = view.state.schema.nodes.image;
  const node = imageType.create({ src: url, attachmentId });
  const tr =
    pos != null
      ? view.state.tr.insert(pos, node)
      : view.state.tr.replaceSelectionWith(node);
  view.dispatch(tr);
  return node.nodeSize;
}

function insertFileAttachment(
  view: EditorView,
  attrs: {
    attachmentId: string;
    name: string;
    mimeType: string;
    src: string;
    path: string;
    size: number;
  },
  pos?: number,
) {
  const attachmentType = view.state.schema.nodes.fileAttachment;
  if (!attachmentType) return 0;
  const node = attachmentType.create(attrs);
  const tr =
    pos != null
      ? view.state.tr.insert(pos, node)
      : view.state.tr.replaceSelectionWith(node);
  view.dispatch(tr);
  return node.nodeSize;
}

async function handleFiles(
  view: EditorView,
  config: FileHandlerConfig,
  files: File[],
  pos?: number,
) {
  let insertPos = pos;

  for (const file of files) {
    if (config.onFileUpload) {
      try {
        const result = await config.onFileUpload(file);
        const insertedSize = isImageFile(file)
          ? insertImage(view, result.url, result.attachmentId, insertPos)
          : insertFileAttachment(
              view,
              {
                attachmentId: result.attachmentId,
                name: file.name,
                mimeType: file.type,
                src: result.url,
                path: result.path,
                size: file.size,
              },
              insertPos,
            );
        if (insertPos != null) {
          insertPos += insertedSize;
        }
      } catch (error) {
        console.error("Failed to upload file:", error);
        config.onFileUploadError?.(file, error);
      }
    } else if (isImageFile(file)) {
      const reader = new FileReader();
      const filePos = insertPos;
      reader.readAsDataURL(file);
      reader.onload = () => {
        insertImage(view, reader.result as string, null, filePos);
      };
      if (insertPos != null) {
        insertPos += 1;
      }
    }
  }
}

export function handleFileDrop(
  view: EditorView,
  config: FileHandlerConfig,
  files: File[],
  pos?: number,
  items?: DataTransferItemList,
) {
  if (files.length === 0) return false;

  if (config.onDrop) {
    const result = config.onDrop(files, pos, items);
    if (result === true) return true;
    if (result === false) return false;
    if (isFileDropRemainder(result)) {
      if (result.remainingFiles.length === 0) return true;

      void handleFiles(view, config, result.remainingFiles, pos);
      return true;
    }
  }

  void handleFiles(view, config, files, pos);
  return true;
}

export function fileHandlerPlugin(config: FileHandlerConfig) {
  return new Plugin({
    key: new PluginKey("fileHandler"),
    props: {
      handleDrop(view, event) {
        const files = Array.from(event.dataTransfer?.files ?? []);
        if (files.length === 0) return false;

        event.preventDefault();
        const pos = view.posAtCoords({
          left: event.clientX,
          top: event.clientY,
        })?.pos;

        return handleFileDrop(
          view,
          config,
          files,
          pos,
          event.dataTransfer?.items,
        );
      },

      handlePaste(view, event) {
        const files = Array.from(event.clipboardData?.files ?? []);
        if (files.length === 0) return false;

        if (config.onPaste) {
          const result = config.onPaste(files, event.clipboardData?.items);
          if (result === true) return true;
          if (result === false) return false;
          if (isFileDropRemainder(result)) {
            if (result.remainingFiles.length === 0) return true;

            void handleFiles(view, config, result.remainingFiles);
            return true;
          }
        }

        void handleFiles(view, config, files);
        return true;
      },
    },
  });
}
