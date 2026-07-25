import { type ReactNode } from "react";

import { useShell } from "~/contexts/shell";
import { LeftSidebar } from "~/sidebar";
import {
  hasCustomSidebarTab,
  useCustomSidebarEffect,
} from "~/sidebar/use-custom-sidebar";
import { useTabs } from "~/store/zustand/tabs";

export function ClassicMainSidebar({
  forceMount = false,
  timelineHeader,
}: {
  forceMount?: boolean;
  timelineHeader?: ReactNode;
} = {}) {
  const { leftsidebar } = useShell();
  const currentTab = useTabs((state) => state.currentTab);
  const isOnboarding = currentTab?.type === "onboarding";

  const hasCustomSidebar = hasCustomSidebarTab(currentTab);

  useCustomSidebarEffect(hasCustomSidebar, leftsidebar);

  if ((!leftsidebar.expanded && !forceMount) || isOnboarding) {
    return null;
  }

  return <LeftSidebar timelineHeader={timelineHeader} />;
}
