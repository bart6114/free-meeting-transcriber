import { isTauri } from "@tauri-apps/api/core";
import { useCallback, useLayoutEffect, useRef } from "react";

import { commands as windowsCommands } from "@hypr/plugin-windows";

import {
  LEFT_SIDEBAR_MIN_WIDTH_PX,
  NOTE_SURFACE_MIN_WIDTH_PX,
  usesNoteSurfaceMinWidth,
} from "./layout-widths";

import { useShell } from "~/contexts/shell";
import { type Tab, useTabs } from "~/store/zustand/tabs";

export function MainBodyPanel({ children }: { children: React.ReactNode }) {
  const { leftsidebar } = useShell();
  const currentTab = useTabs((state) => state.currentTab);
  const bodyPanelContainerRef = useRef<HTMLDivElement>(null);
  const reserveNoteSurfaceMinWidth = usesNoteSurfaceMinWidth(currentTab);
  const collapseLeftSidebar = useCallback(() => {
    leftsidebar.setExpanded(false);
  }, [leftsidebar.setExpanded]);
  const bodyMinWidth = getMainBodyMinWidth({
    currentTab,
    leftSidebarExpanded: leftsidebar.expanded,
  });

  useNoteSurfaceWindowWidthGuard({
    bodyPanelContainerRef,
    enabled: reserveNoteSurfaceMinWidth,
    leftPanelOpen: leftsidebar.expanded,
    collapseLeftPanel: collapseLeftSidebar,
  });

  return (
    <div
      className="flex min-h-0 flex-1 overflow-hidden"
      style={{ minWidth: bodyMinWidth }}
    >
      <div
        ref={bodyPanelContainerRef}
        data-main-body-panel-container
        className="h-full min-h-0 min-w-0 flex-1 overflow-hidden"
      >
        {children}
      </div>
    </div>
  );
}

function getMainBodyMinWidth({
  currentTab,
  leftSidebarExpanded,
}: {
  currentTab: Tab | null;
  leftSidebarExpanded: boolean;
}) {
  if (!usesNoteSurfaceMinWidth(currentTab)) {
    return undefined;
  }

  return (
    NOTE_SURFACE_MIN_WIDTH_PX +
    (leftSidebarExpanded ? LEFT_SIDEBAR_MIN_WIDTH_PX : 0)
  );
}

function useNoteSurfaceWindowWidthGuard({
  bodyPanelContainerRef,
  collapseLeftPanel,
  enabled,
  leftPanelOpen,
}: {
  bodyPanelContainerRef: React.RefObject<HTMLDivElement | null>;
  collapseLeftPanel: () => void;
  enabled: boolean;
  leftPanelOpen: boolean;
}) {
  const lastVisibleBodyWidthRef = useRef<number | null>(null);
  const previousStateRef = useRef({
    enabled: false,
    leftPanelOpen: false,
  });

  useLayoutEffect(() => {
    const previousState = previousStateRef.current;
    const hasOpenPanel = enabled && leftPanelOpen;

    if (!hasOpenPanel) {
      previousStateRef.current = { enabled, leftPanelOpen };
      return;
    }

    const leftPanelJustOpened =
      leftPanelOpen && (!previousState.enabled || !previousState.leftPanelOpen);

    previousStateRef.current = { enabled, leftPanelOpen };

    if (!leftPanelJustOpened) {
      return;
    }

    const bodyPanel = bodyPanelContainerRef.current;
    if (!bodyPanel) {
      return;
    }

    const bodyWidth = getVisibleBodyWidth(bodyPanel);
    if (bodyWidth <= 0) {
      return;
    }

    const leftSidebarWidth = getLeftSidebarWidth(bodyPanel, leftPanelOpen);

    if (!isTauri()) {
      return;
    }

    const requiredBodyWidth = NOTE_SURFACE_MIN_WIDTH_PX + leftSidebarWidth;
    const widthDeficit = Math.ceil(requiredBodyWidth - bodyWidth);

    if (widthDeficit <= 0) {
      return;
    }

    void windowsCommands.windowExpandWidth(
      widthDeficit,
      null,
      false,
      true,
      false,
    );
  }, [bodyPanelContainerRef, collapseLeftPanel, enabled, leftPanelOpen]);

  useLayoutEffect(() => {
    lastVisibleBodyWidthRef.current = null;

    if (!enabled || !leftPanelOpen) {
      return;
    }

    const bodyPanel = bodyPanelContainerRef.current;
    if (!bodyPanel) {
      return;
    }

    const handleResize = () => {
      collapseLeftPanelIfNoteSurfaceWouldShrink({
        bodyPanel,
        collapseLeftPanel,
        lastVisibleBodyWidthRef,
      });
    };

    handleResize();
    window.addEventListener("resize", handleResize);

    const resizeObserver =
      typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(handleResize)
        : null;
    resizeObserver?.observe(bodyPanel);

    const shell = bodyPanel.closest<HTMLElement>(
      "[data-testid='main-app-shell']",
    );
    if (shell) {
      resizeObserver?.observe(shell);
    }

    return () => {
      window.removeEventListener("resize", handleResize);
      resizeObserver?.disconnect();
    };
  }, [bodyPanelContainerRef, collapseLeftPanel, enabled, leftPanelOpen]);
}

function collapseLeftPanelIfNoteSurfaceWouldShrink({
  bodyPanel,
  collapseLeftPanel,
  lastVisibleBodyWidthRef,
}: {
  bodyPanel: HTMLElement;
  collapseLeftPanel: () => void;
  lastVisibleBodyWidthRef: React.MutableRefObject<number | null>;
}) {
  const visibleBodyWidth = getVisibleBodyWidth(bodyPanel);
  if (visibleBodyWidth <= 0) {
    return;
  }

  const lastVisibleBodyWidth = lastVisibleBodyWidthRef.current;
  lastVisibleBodyWidthRef.current = visibleBodyWidth;

  if (
    lastVisibleBodyWidth === null ||
    visibleBodyWidth >= lastVisibleBodyWidth
  ) {
    return;
  }

  const leftSidebarWidth = getLeftSidebarWidth(bodyPanel, true);
  const noteSurfaceWidth = visibleBodyWidth - leftSidebarWidth;

  if (noteSurfaceWidth < NOTE_SURFACE_MIN_WIDTH_PX) {
    collapseLeftPanel();
  }
}

function getVisibleBodyWidth(bodyPanel: HTMLElement) {
  const bodyWidth = bodyPanel.getBoundingClientRect().width;
  const shell = bodyPanel.closest<HTMLElement>(
    "[data-testid='main-app-shell']",
  );
  if (!shell) {
    return bodyWidth;
  }

  const shellWidth = shell.getBoundingClientRect().width;
  if (shellWidth <= 0) {
    return bodyWidth;
  }

  if (bodyWidth <= 0) {
    return shellWidth;
  }

  return Math.min(bodyWidth, shellWidth);
}

function getLeftSidebarWidth(bodyPanel: HTMLElement, leftPanelOpen: boolean) {
  if (!leftPanelOpen) {
    return 0;
  }

  const leftSidebarChrome = bodyPanel.querySelector<HTMLElement>(
    "[data-left-sidebar-chrome]",
  );
  const measuredWidth = leftSidebarChrome?.getBoundingClientRect().width ?? 0;

  return measuredWidth > 0 ? measuredWidth : LEFT_SIDEBAR_MIN_WIDTH_PX;
}
