import { isTauri } from "@tauri-apps/api/core";

import {
  type DeepLink,
  commands as deeplink2Commands,
  events as deeplink2Events,
} from "@hypr/plugin-deeplink2";
import { dismissInstruction } from "@hypr/plugin-windows";

import { useAuth } from "~/auth";
import { subscribeThenDrainDeepLinks } from "~/shared/deeplink";
import { useLatestRef } from "~/shared/hooks/useLatestRef";
import { useMountEffect } from "~/shared/hooks/useMountEffect";

export function useDeeplinkHandler() {
  const auth = useAuth();
  const authRef = useLatestRef(auth);

  useMountEffect(() => {
    if (!isTauri()) {
      return;
    }

    const handleDeepLink = (payload: DeepLink) => {
      if (payload.to === "/auth/callback") {
        const { access_token, refresh_token } = payload.search;
        if (access_token && refresh_token) {
          void authRef.current.setSessionFromTokens(
            access_token,
            refresh_token,
          );
        }
      } else if (payload.to === "/billing/refresh") {
        void authRef.current.refreshSession();
        void dismissInstruction();
      }
    };
    const deepLinkSubscription = subscribeThenDrainDeepLinks({
      listen: (handler) =>
        deeplink2Events.deepLinkEvent.listen(({ payload }) => {
          handler(payload);
        }),
      takePendingDeepLinks: deeplink2Commands.takePendingDeepLinks,
      handle: handleDeepLink,
    });

    return () => {
      void deepLinkSubscription.then((fn) => fn()).catch(() => {});
    };
  });
}
