import { Trans, useLingui } from "@lingui/react/macro";
import { create } from "zustand";

import { Button } from "@hypr/ui/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@hypr/ui/components/ui/dialog";

type PendingConfirm = {
  speakerCount: number;
  resolve: (confirmed: boolean) => void;
};

const useRegenerateConfirm = create<{
  pending: PendingConfirm | null;
  request: (speakerCount: number) => Promise<boolean>;
  settle: (confirmed: boolean) => void;
}>((set, get) => ({
  pending: null,
  request: (speakerCount) =>
    new Promise((resolve) => {
      get().pending?.resolve(false);
      set({ pending: { speakerCount, resolve } });
    }),
  settle: (confirmed) => {
    const pending = get().pending;
    if (!pending) return;
    set({ pending: null });
    pending.resolve(confirmed);
  },
}));

export function confirmRegenerateSpeakerReset(
  speakerCount: number,
): Promise<boolean> {
  return useRegenerateConfirm.getState().request(speakerCount);
}

export function RegenerateTranscriptConfirmDialog() {
  const pending = useRegenerateConfirm((state) => state.pending);
  const settle = useRegenerateConfirm((state) => state.settle);
  const { t } = useLingui();

  return (
    <Dialog
      open={pending !== null}
      onOpenChange={(open) => {
        if (!open) settle(false);
      }}
    >
      {pending !== null && (
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              <Trans>Re-transcribing resets speaker names</Trans>
            </DialogTitle>
            <DialogDescription>
              {pending.speakerCount === 1
                ? t`This transcript has 1 assigned speaker. Re-transcribing replaces the transcript and removes that speaker assignment.`
                : t`This transcript has ${pending.speakerCount} assigned speakers. Re-transcribing replaces the transcript and removes those speaker assignments.`}
            </DialogDescription>
          </DialogHeader>

          <DialogFooter>
            <Button variant="ghost" onClick={() => settle(false)}>
              <Trans>Cancel</Trans>
            </Button>
            <Button onClick={() => settle(true)}>
              <Trans>Re-transcribe</Trans>
            </Button>
          </DialogFooter>
        </DialogContent>
      )}
    </Dialog>
  );
}
