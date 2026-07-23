import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import { commands as localSttCommands } from "@hypr/plugin-local-stt";

import { type ProviderId } from "~/settings/ai/stt/shared";
import { useConfigValues } from "~/shared/config";
import { isHyprnoteLocalSttModel } from "~/stt/capabilities";

// STT is on-device only: the connection is always a local server the
// `local-stt` plugin spins up for the selected model. There is no cloud or
// generic baseUrl+apiKey provider left to connect to.
export const useSTTConnection = () => {
  const { current_stt_provider, current_stt_model } = useConfigValues([
    "current_stt_provider",
    "current_stt_model",
  ] as const) as {
    current_stt_provider: ProviderId | undefined;
    current_stt_model: string | undefined;
  };

  const localModel = isHyprnoteLocalSttModel(
    current_stt_provider,
    current_stt_model,
  )
    ? current_stt_model
    : null;
  const isLocalModel = !!localModel;

  const local = useQuery({
    enabled: current_stt_provider === "hyprnote",
    queryKey: ["stt-connection", current_stt_provider, localModel],
    refetchInterval: 1000,
    queryFn: async () => {
      if (!localModel) {
        return null;
      }

      const downloaded = await localSttCommands.isModelDownloaded(localModel);
      if (downloaded.status !== "ok" || !downloaded.data) {
        return { status: "not_downloaded" as const, connection: null };
      }

      const serverResult = await localSttCommands.getServerForModel(localModel);

      if (serverResult.status !== "ok") {
        return null;
      }

      const server = serverResult.data;

      if (server?.status === "ready" && server.url) {
        return {
          status: "ready" as const,
          connection: {
            provider: current_stt_provider!,
            model: localModel,
            baseUrl: server.url,
            apiKey: "",
          },
        };
      }

      return {
        status: server?.status ?? "loading",
        connection: null,
      };
    },
  });

  const connection = useMemo(() => {
    if (!current_stt_provider || !current_stt_model || !isLocalModel) {
      return null;
    }

    return local.data?.connection ?? null;
  }, [current_stt_provider, current_stt_model, isLocalModel, local.data]);

  return {
    conn: connection,
    local,
    isLocalModel,
  };
};
