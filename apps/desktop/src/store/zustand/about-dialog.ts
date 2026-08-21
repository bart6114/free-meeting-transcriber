import { create } from "zustand";

type AboutDialogState = {
  open: boolean;
  setOpen: (open: boolean) => void;
};

export const useAboutDialog = create<AboutDialogState>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));
