import type { ReactNode } from "react";

export type TodoProvider = {
  id: string;
  displayName: string;
  icon: ReactNode;
  permission?: "reminders";
  platform?: "macos" | "all";
};

// GitHub (and Linear, never wired up in this UI) were removed (Task 4
// review fix): both were Nango/OAuth-backed and required the hosted
// backend removed in Task 1 — they could never connect again in this
// fork. Apple Reminders (EventKit, fully local) is the only remaining
// provider.
export const TODO_PROVIDERS: TodoProvider[] = [
  {
    id: "apple-reminders",
    displayName: "Apple Reminders",
    icon: (
      <img
        src="/assets/apple-reminders.png"
        alt="Apple Reminders"
        className="size-5 rounded-[4px] object-cover"
      />
    ),
    permission: "reminders",
    platform: "macos",
  },
];
