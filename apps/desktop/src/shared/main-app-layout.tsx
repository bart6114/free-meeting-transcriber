import { Outlet, useNavigate } from "@tanstack/react-router";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";

import { events as windowsEvents } from "@hypr/plugin-windows";

import { useNewNote } from "./useNewNote";

import { DevtoolsFloatingPanelHost } from "~/devtools-panel/host";
import { UndoDeleteToast } from "~/sidebar/toast/undo-delete-toast";
import { useAboutDialog } from "~/store/zustand/about-dialog";
import { isTabInputSupported, useTabs } from "~/store/zustand/tabs";

export default function MainAppLayout() {
  useNavigationEvents();
  useFullscreenAttribute();

  return <MainAppContent />;
}

function MainAppContent() {
  return (
    <>
      <Outlet />
      <UndoDeleteToast />
      <DevtoolsFloatingPanelHost />
    </>
  );
}

const useFullscreenAttribute = () => {
  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    const appWindow = getCurrentWindow();
    let cancelled = false;

    const sync = () => {
      void appWindow
        .isFullscreen()
        .then((fullscreen) => {
          if (!cancelled) {
            document.documentElement.toggleAttribute(
              "data-fullscreen",
              fullscreen,
            );
          }
        })
        .catch(() => {});
    };

    sync();
    const unlisten = appWindow.onResized(sync);

    return () => {
      cancelled = true;
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);
};

const useNavigationEvents = () => {
  const navigate = useNavigate();
  const openNew = useTabs((state) => state.openNew);
  const openNewNote = useNewNote({ behavior: "new" });

  useEffect(() => {
    (window as any).__HYPR_NAVIGATE__ = (path: string) => {
      const match = path.match(/^\/app\/([^/]+)\/(.+)$/);
      if (!match) return;
      const [, type, id] = match;
      if (type === "session") {
        openNew({ type: "sessions", id });
      }
    };

    let unlistenNavigate: (() => void) | undefined;
    let unlistenOpenTab: (() => void) | undefined;

    const webview = getCurrentWebviewWindow();

    void windowsEvents
      .navigate(webview)
      .listen(({ payload }) => {
        if (payload.path === "/app/new") {
          openNewNote();
        } else if (payload.path === "/app/about") {
          useAboutDialog.getState().setOpen(true);
        } else if (payload.path === "/app/settings") {
          const tab = (payload.search?.tab as string) ?? "app";
          openNew({ type: "settings", state: { tab } });
        } else {
          void navigate({
            to: payload.path,
            search: payload.search ?? undefined,
          });
        }
      })
      .then((fn) => {
        unlistenNavigate = fn;
      });

    void windowsEvents
      .openTab(webview)
      .listen(({ payload }) => {
        if (payload.tab.type === "sessions" && payload.tab.id === "new") {
          openNewNote();
        } else if (!isTabInputSupported(payload.tab)) {
          return;
        } else {
          openNew(payload.tab);
        }
      })
      .then((fn) => {
        unlistenOpenTab = fn;
      });

    return () => {
      delete (window as any).__HYPR_NAVIGATE__;
      unlistenNavigate?.();
      unlistenOpenTab?.();
    };
  }, [navigate, openNew, openNewNote]);
};
