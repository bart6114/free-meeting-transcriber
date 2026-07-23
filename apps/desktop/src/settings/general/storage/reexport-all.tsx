import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";

import { Button } from "@hypr/ui/components/ui/button";
import { sonnerToast } from "@hypr/ui/components/ui/toast";

import { commands } from "~/types/tauri.gen";

export function ReExportAllFilesRow() {
  const { t } = useLingui();

  const exportMutation = useMutation({
    mutationFn: async () => {
      const result = await commands.exportVaultNow();
      if (result.status === "error") {
        throw new Error(result.error);
      }
    },
    onSuccess: () => {
      sonnerToast.success(
        t`Re-export queued — files will update in the vault within a few seconds.`,
      );
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
          <Trans>Re-render every session, contact, and calendar file</Trans>
        </p>
      </div>
      <Button
        variant="outline"
        className="h-9 w-full justify-center"
        onClick={() => exportMutation.mutate()}
        disabled={exportMutation.isPending}
      >
        {exportMutation.isPending
          ? <Trans>Exporting...</Trans>
          : <Trans>Re-export all files</Trans>}
      </Button>
    </div>
  );
}
