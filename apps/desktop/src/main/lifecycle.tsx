import { useRouteContext } from "@tanstack/react-router";
import { useCallback, useEffect, useRef } from "react";

import { useLanguageModel } from "~/ai/hooks";
import { takePendingWelcomeSession } from "~/onboarding/welcome-note";
import { initEnhancerService } from "~/services/enhancer";
import { useConfigValue } from "~/shared/config";
import { useDesktopTabLifecycle } from "~/shared/desktop-tab-lifecycle";
import { useTabs } from "~/store/zustand/tabs";
import { MainListenerControlBridge } from "~/stt/window-control";

export function useClassicMainLifecycle() {
  const openNew = useTabs((state) => state.openNew);

  const openDefaultEmptyTab = useCallback(() => {
    openNew({ type: "empty" });
  }, [openNew]);

  const openPendingWelcomeTab = useCallback(() => {
    const welcomeSessionId = takePendingWelcomeSession();
    if (welcomeSessionId) {
      openNew({ type: "sessions", id: welcomeSessionId });
    }
  }, [openNew]);

  useDesktopTabLifecycle({
    onEmpty: openDefaultEmptyTab,
    onInitialized: openPendingWelcomeTab,
    onZeroTabs: openDefaultEmptyTab,
  });
}

export function ClassicMainServices() {
  return (
    <>
      <MainListenerControlBridge />
      <EnhancerInit />
    </>
  );
}

function EnhancerInit() {
  const { aiTaskStore } = useRouteContext({
    from: "__root__",
  });

  const model = useLanguageModel();
  const selectedTemplateId = useConfigValue("selected_template_id");

  const modelRef = useRef(model);
  modelRef.current = model;
  const templateIdRef = useRef(selectedTemplateId);
  templateIdRef.current = selectedTemplateId;

  useEffect(() => {
    if (!aiTaskStore) return;

    const service = initEnhancerService({
      aiTaskStore,
      getModel: () => modelRef.current,
      getSelectedTemplateId: () => templateIdRef.current || undefined,
    });

    return () => service.dispose();
  }, [aiTaskStore]);

  return null;
}
