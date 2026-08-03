import type { Tab } from "~/store/zustand/tabs";

export const NOTE_SURFACE_MIN_WIDTH_PX = 500;
export const LEFT_SIDEBAR_MIN_WIDTH_PX = 220;

export function usesNoteSurfaceMinWidth(tab: Pick<Tab, "type"> | null) {
  return tab?.type === "sessions" || tab?.type === "empty";
}
