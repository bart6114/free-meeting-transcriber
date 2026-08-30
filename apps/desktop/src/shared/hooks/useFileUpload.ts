import { useQueryClient } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback } from "react";

import {
  type AttachmentSaveResult,
  commands as fsSyncCommands,
} from "@hypr/plugin-fs-sync";

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
      void queryClient.invalidateQueries({
        queryKey: sessionAttachmentPathsQueryKey(sessionId),
      });
      return { path, attachmentId, url: convertFileSrc(path) };
    },
    [queryClient, sessionId],
  );
}
