import type { ReactNode } from "react";

export function SettingsPageTitle({ title }: { title: ReactNode }) {
  return (
    <h2 className="text-2xl leading-none font-semibold tracking-tight">
      {title}
    </h2>
  );
}
