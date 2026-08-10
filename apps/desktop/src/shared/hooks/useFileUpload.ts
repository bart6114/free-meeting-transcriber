import { useQueryClient } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback } from "react";

import {
  type AttachmentSaveResult,
  commands as fsSyncCommands,
} from "@hypr/plugin-fs-sync";

import { catalogLocalNoteAttachment, sha256Hex } from "~/session/attachments";
import { sessionAttachmentPathsQueryKey } from "~/session/hooks/useAttachmentResolver";

export type FileUploadResult = AttachmentSaveResult & {
  url: string;
};

export function useFileUpload(sessionId: string) {
  const queryClient = useQueryClient();

  return useCallback(
    async (file: File): Promise<FileUploadResult> => {
      const filename = file.name;
      const arrayBuffer = await file.arrayBuffer();
      const sha256 = await sha256Hex(arrayBuffer);
      const data = Array.from(new Uint8Array(arrayBuffer));

      const result = await fsSyncCommands.attachmentSave(
        sessionId,
        data,
        filename,
      );

      if (result.status === "error") {
        throw new Error(result.error);
      }

      const { path, attachmentId } = result.data;
      try {
        await catalogLocalNoteAttachment({
          sessionId,
          attachmentId,
          filename,
          contentType: file.type,
          sizeBytes: arrayBuffer.byteLength,
          sha256,
        });
      } catch (error) {
        try {
          const cleanup = await fsSyncCommands.attachmentRemove(
            sessionId,
            attachmentId,
          );
          if (cleanup.status === "error") {
            console.error("[attachment] failed to roll back local file");
          }
        } catch {
          console.error("[attachment] failed to roll back local file");
        }
        throw error;
      }
      void queryClient.invalidateQueries({
        queryKey: sessionAttachmentPathsQueryKey(sessionId),
      });
      return { path, attachmentId, url: convertFileSrc(path) };
    },
    [queryClient, sessionId],
  );
}
