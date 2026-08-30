import { Trans } from "@lingui/react/macro";
import { TrashIcon } from "lucide-react";
import { useCallback } from "react";

import { DropdownMenuItem } from "@hypr/ui/components/ui/dropdown-menu";
import { cn } from "@hypr/utils";

import { useDeleteSession } from "~/session/hooks/useDeleteSession";
import { useSessionSummary } from "~/session/queries";

export function DeleteNote({ sessionId }: { sessionId: string }) {
  const deleteSession = useDeleteSession();
  const title = useSessionSummary(sessionId)?.title;

  const handleDeleteNote = useCallback(() => {
    deleteSession(sessionId, { title });
  }, [sessionId, deleteSession, title]);

  return (
    <DropdownMenuItem
      onClick={handleDeleteNote}
      className={cn([
        "text-destructive cursor-pointer",
        "hover:bg-destructive/10 hover:text-destructive",
      ])}
    >
      <TrashIcon />
      <span>
        <Trans>Delete</Trans>
      </span>
    </DropdownMenuItem>
  );
}
