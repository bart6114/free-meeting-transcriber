import {
  ResizablePanel,
  ResizablePanelGroup,
} from "@hypr/ui/components/ui/resizable";
import { cn } from "@hypr/utils";

export { MainShellBodyFrame } from "./body-frame";
export { useMainContentCenterOffset } from "./content-offset";
export {
  MainSessionStatusBannerHost,
  SessionStatusBannerProvider,
  useSessionStatusBanner,
} from "./session-status-banner";
export { MainShellScaffold, type MainSurfaceChrome } from "./shell-scaffold";

export function StandardContentWrapper({
  children,
  noBorder = false,
}: {
  children: React.ReactNode;
  noBorder?: boolean;
}) {
  return (
    <div className="flex h-full flex-col">
      <ResizablePanelGroup direction="vertical" className="min-h-0 flex-1">
        <ResizablePanel defaultSize={100} minSize={35} className="min-h-0">
          <MainPanel fill noBorder={noBorder}>
            {children}
          </MainPanel>
        </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  );
}

function MainPanel({
  children,
  fill,
  noBorder,
}: {
  children: React.ReactNode;
  fill: boolean;
  noBorder: boolean;
}) {
  return (
    <div
      className={cn([
        "relative flex min-h-0 flex-1 flex-col",
        fill && "h-full",
      ])}
    >
      <div
        data-main-surface
        className={cn([
          "bg-card @container relative flex min-h-0 flex-1 flex-col overflow-hidden",
          "rounded-xl",
          !noBorder && "border-border border",
        ])}
      >
        {children}
      </div>
    </div>
  );
}
