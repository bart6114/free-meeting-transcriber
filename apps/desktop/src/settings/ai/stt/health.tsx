import { Spinner } from "@hypr/ui/components/ui/spinner";

import { useConfigValues } from "~/shared/config";
import { isFmtrLocalSttModel } from "~/stt/capabilities";
import { useSTTConnection } from "~/stt/useSTTConnection";

export type HealthStatus = {
  status: "pending" | "error" | "success" | null;
  message?: string;
};

export function HealthStatusIndicator() {
  const health = useConnectionHealth();

  if (health.status === "pending") {
    return <Spinner size={14} className="text-muted-foreground shrink-0" />;
  }

  return null;
}

export function useConnectionHealth(): HealthStatus {
  const { conn, local } = useSTTConnection();
  const { current_stt_provider, current_stt_model } = useConfigValues([
    "current_stt_provider",
    "current_stt_model",
  ] as const);

  const isLocalModel = isFmtrLocalSttModel(
    current_stt_provider,
    current_stt_model,
  );

  if (current_stt_provider === "fmtr" && current_stt_model && !isLocalModel) {
    return {
      status: "error",
      message: "Selected model is no longer available.",
    };
  }

  if (isLocalModel) {
    const serverStatus = local.data?.status ?? "unavailable";
    if (serverStatus === "not_downloaded") {
      return {
        status: "error",
        message: "Selected model is not downloaded.",
      };
    }
    if (serverStatus === "loading") {
      return {
        status: "pending",
        message: "Local STT server is starting up…",
      };
    }
    if (serverStatus === "ready" && conn) {
      return { status: "success" };
    }
    return {
      status: "error",
      message: "Could not connect to the local speech-to-text model.",
    };
  }

  if (!conn) {
    return { status: "error", message: "Provider not configured." };
  }

  return { status: "success" };
}
