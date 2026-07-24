import { cn } from "@hypr/utils";

export type MainSurfaceChrome = "default" | "top" | "top-borderless" | "left";

export function MainShellScaffold({
  children,
  edgeToEdge = false,
  mainSurfaceChrome,
}: {
  children: React.ReactNode;
  edgeToEdge?: boolean;
  mainSurfaceChrome?: MainSurfaceChrome;
}) {
  const resolvedMainSurfaceChrome =
    mainSurfaceChrome ?? (edgeToEdge ? "top" : "default");
  const hasTopMainSurfaceChrome =
    resolvedMainSurfaceChrome === "top" ||
    resolvedMainSurfaceChrome === "top-borderless";

  return (
    <div
      className={cn([
        "bg-background flex h-full gap-1 overflow-hidden",
        !hasTopMainSurfaceChrome && "pl-1",
        hasTopMainSurfaceChrome && [
          "[&_[data-main-surface]]:rounded-t-xl",
          "[&_[data-main-surface]]:rounded-b-none",
          "[&_[data-main-surface]]:border-x-0",
          resolvedMainSurfaceChrome === "top"
            ? "[&_[data-main-surface]]:border-t"
            : "[&_[data-main-surface]]:!border-t-0",
          "[&_[data-main-surface]]:border-b-0",
        ],
        resolvedMainSurfaceChrome === "left" && [
          "[&_[data-main-surface]]:rounded-l-xl",
          "[&_[data-main-surface]]:rounded-r-none",
          "[&_[data-main-surface]]:border-y-0",
          "[&_[data-main-surface]]:border-r-0",
          "[&_[data-main-surface]]:border-l",
        ],
      ])}
      data-testid="main-app-shell"
    >
      {children}
    </div>
  );
}
