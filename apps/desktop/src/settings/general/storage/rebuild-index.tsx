import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";

import { Button } from "@hypr/ui/components/ui/button";
import { sonnerToast } from "@hypr/ui/components/ui/toast";

import { commands } from "~/types/tauri.gen";

export function RebuildIndexRow() {
  const { t } = useLingui();

  const rebuildMutation = useMutation({
    mutationFn: async () => {
      const result = await commands.sessionRebuildIndex();
      if (result.status === "error") {
        throw new Error(result.error);
      }
      return result.data;
    },
    onSuccess: () => {
      sonnerToast.success(t`Index rebuilt from the files in your folder.`);
    },
    onError: (error: Error) => {
      sonnerToast.error(error.message);
    },
  });

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_9rem] items-center gap-3">
      <div className="border-border bg-muted flex min-w-0 items-center gap-3 rounded-lg border px-4 py-3">
        <RefreshCwIcon className="text-muted-foreground size-4 shrink-0" />
        <p className="text-muted-foreground min-w-0 flex-1 truncate text-left text-sm">
          <Trans>
            Re-read every session file and rebuild the database index
          </Trans>
        </p>
      </div>
      <Button
        variant="outline"
        className="h-9 w-full justify-center"
        onClick={() => rebuildMutation.mutate()}
        disabled={rebuildMutation.isPending}
      >
        {rebuildMutation.isPending ? (
          <Trans>Rebuilding...</Trans>
        ) : (
          <Trans>Rebuild index</Trans>
        )}
      </Button>
    </div>
  );
}
