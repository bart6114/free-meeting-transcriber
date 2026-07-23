import type { ServerStatus } from "@hypr/plugin-local-stt";

import type { DownloadProgress, ToastCondition, ToastType } from "./types";

import type { DevtoolsToastPreview } from "~/store/zustand/devtools-toast-preview";

type ToastRegistryEntry = {
  toast: ToastType;
  condition: ToastCondition;
};

type ToastRegistryParams = {
  hasLLMConfigured: boolean;
  hasSttConfigured: boolean;
  isAiTranscriptionTabActive: boolean;
  isAiIntelligenceTabActive: boolean;
  isBatchTranscribingInActiveTranscriptTab: boolean;
  cloudsyncInitialSyncToastId: string | null;
  hasActiveDownload: boolean;
  downloadingModel: string | null;
  activeDownloads: DownloadProgress[];
  localSttStatus: ServerStatus | null;
  isLocalSttModel: boolean;
  onOpenLLMSettings: () => void;
  onOpenSTTSettings: () => void;
};

type DevtoolsToastPreviewParams = {
  preview: DevtoolsToastPreview;
  onOpenLLMSettings: () => void;
  onOpenSTTSettings: () => void;
};

export function createToastRegistry({
  hasLLMConfigured,
  hasSttConfigured,
  isAiTranscriptionTabActive,
  isAiIntelligenceTabActive,
  isBatchTranscribingInActiveTranscriptTab,
  cloudsyncInitialSyncToastId,
  hasActiveDownload,
  downloadingModel,
  activeDownloads,
  localSttStatus,
  isLocalSttModel,
  onOpenLLMSettings,
  onOpenSTTSettings,
}: ToastRegistryParams): ToastRegistryEntry[] {
  const downloadTitle =
    activeDownloads.length === 1 && downloadingModel
      ? `Downloading ${downloadingModel}`
      : `Downloading ${activeDownloads.length} models`;

  // order matters
  return [
    {
      toast: {
        id: "downloading-model",
        description: downloadTitle,
        dismissible: false,
        loading: true,
      },
      condition: () => hasActiveDownload,
    },
    {
      toast: {
        id: cloudsyncInitialSyncToastId ?? "cloudsync-initial-sync",
        description: "Syncing your data in the background...",
        dismissible: true,
        loading: true,
      },
      condition: () => cloudsyncInitialSyncToastId !== null,
    },
    {
      toast: {
        id: "local-stt-loading",
        description: "Starting transcription...",
        dismissible: false,
        loading: true,
      },
      condition: () =>
        isLocalSttModel &&
        localSttStatus === "loading" &&
        !hasActiveDownload &&
        !isBatchTranscribingInActiveTranscriptTab,
    },
    {
      toast: {
        id: "local-stt-unreachable",
        description: "Transcription unavailable",
        primaryAction: {
          label: "Settings",
          onClick: onOpenSTTSettings,
        },
        dismissible: true,
        variant: "error",
      },
      condition: () =>
        isLocalSttModel &&
        localSttStatus === "unreachable" &&
        !hasActiveDownload &&
        !isAiTranscriptionTabActive,
    },
    {
      toast: {
        id: "missing-stt",
        description: "Transcription model needed",
        primaryAction: {
          label: "Add",
          onClick: onOpenSTTSettings,
        },
        dismissible: false,
      },
      condition: () => !hasSttConfigured && !isAiTranscriptionTabActive,
    },
    {
      toast: {
        id: "missing-llm",
        description: "Language model needed",
        primaryAction: {
          label: "Add",
          onClick: onOpenLLMSettings,
        },
        dismissible: true,
      },
      condition: () =>
        hasSttConfigured && !hasLLMConfigured && !isAiIntelligenceTabActive,
    },
  ];
}

export function getToastToShow(
  registry: ToastRegistryEntry[],
  isDismissed: (id: string) => boolean,
): ToastType | null {
  for (const entry of registry) {
    if (entry.condition() && !isDismissed(entry.toast.id)) {
      return entry.toast;
    }
  }
  return null;
}

export function createDevtoolsToastPreview({
  preview,
  onOpenLLMSettings,
  onOpenSTTSettings,
}: DevtoolsToastPreviewParams): ToastType {
  switch (preview) {
    case "language-model":
      return {
        id: "devtools-missing-llm",
        description: "Language model needed",
        primaryAction: {
          label: "Add",
          onClick: onOpenLLMSettings,
        },
        dismissible: true,
      };
    case "transcription-model":
      return {
        id: "devtools-missing-stt",
        description: "Transcription model needed",
        primaryAction: {
          label: "Add",
          onClick: onOpenSTTSettings,
        },
        dismissible: false,
      };
    case "transcription-error":
      return {
        id: "devtools-local-stt-unreachable",
        description: "Transcription unavailable",
        primaryAction: {
          label: "Settings",
          onClick: onOpenSTTSettings,
        },
        dismissible: true,
        variant: "error",
      };
    case "download":
      return {
        id: "devtools-downloading-model",
        description: "Downloading model",
        dismissible: false,
        loading: true,
      };
  }
}
