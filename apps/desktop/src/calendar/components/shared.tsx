import type { ReactNode } from "react";

export type CalendarProvider = {
  disabled: boolean;
  id: string;
  displayName: string;
  icon: ReactNode;
  badge?: string | null;
  platform?: "macos" | "all";
  docsPath: string;
};

// Google Calendar and Outlook were removed (Task 4 review fix): both were
// Nango/OAuth-backed and required the hosted backend removed in Task 1 —
// they could never connect again in this fork. Apple Calendar (EventKit,
// fully local) is the only remaining provider.
const _PROVIDERS = [
  {
    disabled: false,
    id: "apple",
    displayName: "Apple Calendar",
    badge: "",
    icon: (
      <img
        src="/assets/apple-calendar.png"
        alt="Apple Calendar"
        className="size-5 rounded-[4px] object-cover"
      />
    ),
    platform: "macos",
    docsPath: "https://docs.anarlog.so/calendar#apple-calendar",
  },
] as const satisfies readonly CalendarProvider[];

export const PROVIDERS: CalendarProvider[] = [..._PROVIDERS];
