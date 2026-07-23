import { describe, expect, it, vi } from "vitest";

import {
  createDevtoolsToastPreview,
  createToastRegistry,
  getToastToShow,
} from "./registry";

const baseParams = {
  hasLLMConfigured: true,
  hasSttConfigured: true,
  isAiTranscriptionTabActive: false,
  isAiIntelligenceTabActive: false,
  isBatchTranscribingInActiveTranscriptTab: false,
  cloudsyncInitialSyncToastId: null,
  hasActiveDownload: false,
  downloadingModel: null,
  activeDownloads: [],
  localSttStatus: null,
  isLocalSttModel: false,
  onOpenLLMSettings: vi.fn(),
  onOpenSTTSettings: vi.fn(),
};

describe("sidebar toast registry", () => {
  it("keeps the missing language model message short", () => {
    const toast = getToastToShow(
      createToastRegistry({
        ...baseParams,
        hasLLMConfigured: false,
      }),
      () => false,
    );

    expect(toast?.id).toBe("missing-llm");
    expect(toast?.description).toBe("Language model needed");
    expect(toast?.primaryAction?.label).toBe("Add");
  });

  it("keeps the missing transcription model message short", () => {
    const toast = getToastToShow(
      createToastRegistry({
        ...baseParams,
        hasSttConfigured: false,
      }),
      () => false,
    );

    expect(toast?.id).toBe("missing-stt");
    expect(toast?.description).toBe("Transcription model needed");
    expect(toast?.primaryAction?.label).toBe("Add");
  });

  it("hides local STT loading while the active transcript tab shows batch progress", () => {
    const toast = getToastToShow(
      createToastRegistry({
        ...baseParams,
        localSttStatus: "loading",
        isLocalSttModel: true,
        isBatchTranscribingInActiveTranscriptTab: true,
      }),
      () => false,
    );

    expect(toast).toBeNull();
  });

  it("shows local STT loading outside active transcript batch progress", () => {
    const toast = getToastToShow(
      createToastRegistry({
        ...baseParams,
        localSttStatus: "loading",
        isLocalSttModel: true,
      }),
      () => false,
    );

    expect(toast?.id).toBe("local-stt-loading");
    expect(toast?.description).toBe("Starting transcription...");
  });

  it("shows a dismissible loading toast during initial cloud sync", () => {
    const toast = getToastToShow(
      createToastRegistry({
        ...baseParams,
        cloudsyncInitialSyncToastId: "cloudsync-initial-sync-user-1",
      }),
      () => false,
    );

    expect(toast?.id).toBe("cloudsync-initial-sync-user-1");
    expect(toast?.description).toBe("Syncing your data in the background...");
    expect(toast?.dismissible).toBe(true);
    expect(toast?.loading).toBe(true);
  });

  it("no longer offers a hosted-subscription upgrade toast", () => {
    // Regression guard for the removed pro-requires-login/upgrade-to-pro
    // branches: a fully-configured install must never surface either id,
    // even in the "signed out" shape auth is permanently stuck in now that
    // accounts/billing are gone.
    const toast = getToastToShow(createToastRegistry(baseParams), () => false);

    expect(toast?.id).not.toBe("upgrade-to-pro");
    expect(toast?.id).not.toBe("pro-requires-login");
    expect(toast).toBeNull();
  });

  it("creates devtools previews with app toast content", () => {
    const languageModelToast = createDevtoolsToastPreview({
      preview: "language-model",
      onOpenLLMSettings: vi.fn(),
      onOpenSTTSettings: vi.fn(),
    });
    const downloadToast = createDevtoolsToastPreview({
      preview: "download",
      onOpenLLMSettings: vi.fn(),
      onOpenSTTSettings: vi.fn(),
    });

    expect(languageModelToast.id).toBe("devtools-missing-llm");
    expect(languageModelToast.description).toBe("Language model needed");
    expect(languageModelToast.primaryAction?.label).toBe("Add");
    expect(downloadToast.id).toBe("devtools-downloading-model");
    expect(downloadToast.loading).toBe(true);
  });
});
