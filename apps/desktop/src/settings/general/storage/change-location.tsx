import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { homeDir } from "@tauri-apps/api/path";
import { open as selectFolder } from "@tauri-apps/plugin-dialog";
import { FolderIcon } from "lucide-react";
import { useState } from "react";

import { commands as openerCommands } from "@hypr/plugin-opener2";
import { commands as settingsCommands } from "@hypr/plugin-settings";
import { Button } from "@hypr/ui/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@hypr/ui/components/ui/dialog";
import { sonnerToast } from "@hypr/ui/components/ui/toast";

import { ObsidianVaultList } from "./obsidian-vault-list";
import { displayPath } from "./path-utils";

import { scheduleAutomaticRelaunch } from "~/shared/relaunch";
import { commands as tauriCommands } from "~/types/tauri.gen";

const VAULT_BASE_QUERY_KEY = ["vault-base-path"] as const;

export function ChangeLocationRow() {
  const { t } = useLingui();
  const queryClient = useQueryClient();
  const [pendingPath, setPendingPath] = useState<string | null>(null);

  const { data: home } = useQuery({ queryKey: ["home-dir"], queryFn: homeDir });

  const { data: vaultBase } = useQuery({
    queryKey: VAULT_BASE_QUERY_KEY,
    queryFn: async () => {
      const result = await settingsCommands.vaultBase();
      if (result.status === "error") {
        throw new Error(result.error);
      }
      return result.data;
    },
  });

  const { data: obsidianVaults } = useQuery({
    queryKey: ["obsidian-vaults"],
    queryFn: async () => {
      const result = await settingsCommands.obsidianVaults();
      if (result.status === "error") return [];
      return result.data;
    },
  });

  const { data: destinationHasFiles } = useQuery({
    queryKey: ["vault-destination-has-files", pendingPath],
    enabled: pendingPath !== null,
    queryFn: async () => {
      const result = await settingsCommands.isEmptyOrMissingDir(pendingPath!);
      if (result.status === "error") return false;
      return !result.data;
    },
  });

  const changeMutation = useMutation({
    mutationFn: async ({
      newPath,
      keepOriginal,
    }: {
      newPath: string;
      keepOriginal: boolean;
    }) => {
      const result = await tauriCommands.relocateVault(newPath, keepOriginal);
      if (result.status === "error") {
        throw new Error(result.error);
      }
    },
    onSuccess: async () => {
      setPendingPath(null);
      await queryClient.invalidateQueries({ queryKey: VAULT_BASE_QUERY_KEY });
      await scheduleAutomaticRelaunch();
    },
    onError: (error) => {
      sonnerToast.error(error.message);
    },
  });

  const openPickerForPath = (path: string) => {
    changeMutation.reset();
    setPendingPath(path);
  };

  const handleChange = async () => {
    const selected = await selectFolder({
      title: t`Choose storage location`,
      directory: true,
      multiple: false,
      defaultPath: vaultBase ?? undefined,
    });

    if (selected && selected !== vaultBase) {
      openPickerForPath(selected);
    }
  };

  const handleOpenPath = () => {
    if (vaultBase) {
      openerCommands.openPath(vaultBase, null);
    }
  };

  const closeDialog = () => {
    if (changeMutation.isPending) return;
    setPendingPath(null);
  };

  const detectedVaults = (obsidianVaults ?? []).filter(
    (v) => v.path !== vaultBase,
  );

  return (
    <>
      <div className="flex flex-col gap-3">
        <div className="grid grid-cols-[minmax(0,1fr)_9rem] items-center gap-3">
          <div className="border-border bg-muted flex min-w-0 items-center gap-3 rounded-lg border px-4 py-3">
            <FolderIcon className="text-muted-foreground size-4 shrink-0" />
            <button
              onClick={handleOpenPath}
              className="text-muted-foreground min-w-0 flex-1 truncate text-left text-sm hover:underline"
            >
              {displayPath(vaultBase, home)}
            </button>
          </div>
          <Button
            variant="outline"
            className="h-9 w-full justify-center"
            onClick={handleChange}
            disabled={changeMutation.isPending}
          >
            <Trans>Change</Trans>
          </Button>
        </div>

        <ObsidianVaultList
          vaults={detectedVaults}
          home={home}
          disabled={changeMutation.isPending}
          onSelect={openPickerForPath}
        />
      </div>

      <Dialog
        open={pendingPath !== null}
        onOpenChange={(open) => {
          if (!open) closeDialog();
        }}
      >
        {pendingPath !== null && (
          <DialogContent>
            <DialogHeader>
              <DialogTitle>
                <Trans>Change storage location?</Trans>
              </DialogTitle>
              <DialogDescription>
                <Trans>
                  App restarts to apply. Move relocates your files; copy leaves
                  the originals behind.
                </Trans>
              </DialogDescription>
            </DialogHeader>

            <p className="text-muted-foreground truncate text-sm">
              {displayPath(pendingPath, home)}
            </p>

            {destinationHasFiles && (
              <p className="text-brand text-xs">
                <Trans>
                  This folder already contains files, so it can only be copied
                  into. Vault files will be mixed in with the existing ones.
                </Trans>
              </p>
            )}

            {changeMutation.error && (
              <p className="text-destructive text-sm">
                {changeMutation.error.message}
              </p>
            )}

            <DialogFooter>
              <Button
                variant="ghost"
                onClick={closeDialog}
                disabled={changeMutation.isPending}
              >
                <Trans>Cancel</Trans>
              </Button>
              <Button
                variant="outline"
                onClick={() =>
                  changeMutation.mutate({
                    newPath: pendingPath,
                    keepOriginal: true,
                  })
                }
                disabled={changeMutation.isPending}
              >
                {changeMutation.isPending &&
                changeMutation.variables?.keepOriginal
                  ? t`Copying...`
                  : t`Copy`}
              </Button>
              {!destinationHasFiles && (
                <Button
                  onClick={() =>
                    changeMutation.mutate({
                      newPath: pendingPath,
                      keepOriginal: false,
                    })
                  }
                  disabled={changeMutation.isPending}
                >
                  {changeMutation.isPending &&
                  !changeMutation.variables?.keepOriginal
                    ? t`Moving...`
                    : t`Move`}
                </Button>
              )}
            </DialogFooter>
          </DialogContent>
        )}
      </Dialog>
    </>
  );
}
