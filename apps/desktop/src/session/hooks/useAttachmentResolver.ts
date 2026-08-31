import { useQuery } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useCallback, useMemo } from "react";

import type { AttachmentResolver } from "@hypr/editor/node-views";
import {
  type AttachmentInfo,
  commands as fsSyncCommands,
} from "@hypr/plugin-fs-sync";

export function sessionAttachmentPathsQueryKey(sessionId: string) {
  return ["session", sessionId, "attachment-paths"] as const;
}

export function useSessionAttachments(sessionId: string) {
  return useQuery({
    queryKey: sessionAttachmentPathsQueryKey(sessionId),
    queryFn: async () => {
      const result = await fsSyncCommands.attachmentList(sessionId);
      if (result.status === "error") {
        throw new Error(result.error);
      }
      return result.data;
    },
    retry: false,
  });
}

export function useAttachmentResolver(sessionId: string): AttachmentResolver {
  const { data = EMPTY_ATTACHMENTS } = useSessionAttachments(sessionId);
  const attachments = useMemo(
    () =>
      new Map(
        data.map((attachment) => [
          attachment.attachmentId,
          {
            path: attachment.path,
            src: convertFileSrc(attachment.path),
          },
        ]),
      ),
    [data],
  );

  return useCallback(
    (attachmentId: string) => attachments.get(attachmentId) ?? null,
    [attachments],
  );
}

const EMPTY_ATTACHMENTS: AttachmentInfo[] = [];
