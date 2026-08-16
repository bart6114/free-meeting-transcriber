import type { MouseEvent, ReactNode } from "react";

import { cn } from "@hypr/utils";

export function TimelineMeta({ children }: { children: ReactNode }) {
  return (
    <div className="text-muted-foreground inline-flex shrink-0 items-center gap-1 font-mono text-[10px] tracking-[0.03em] tabular-nums select-none">
      {children}
    </div>
  );
}

export function TimelineShell({
  contentClassName,
  leading,
  meta,
  main,
  onContextMenu,
}: {
  contentClassName?: string;
  leading: ReactNode;
  meta?: ReactNode;
  main: ReactNode;
  onContextMenu?: (event: MouseEvent<HTMLDivElement>) => void;
}) {
  return (
    <div
      className="w-full rounded-xl bg-transparent select-none"
      onContextMenu={onContextMenu}
    >
      <div
        className={cn([
          "flex items-center",
          "w-full max-w-full",
          contentClassName,
        ])}
      >
        {leading}
        <div className="min-w-0 flex-1">{main}</div>
        {meta}
      </div>
    </div>
  );
}
