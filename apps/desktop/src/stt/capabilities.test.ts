import { beforeEach, describe, expect, test, vi } from "vitest";

const { isSupportedLanguagesBatchMock, isSupportedLanguagesLiveMock } =
  vi.hoisted(() => ({
    isSupportedLanguagesBatchMock: vi.fn(),
    isSupportedLanguagesLiveMock: vi.fn(),
  }));

vi.mock("@hypr/plugin-transcription", () => ({
  commands: {
    isSupportedLanguagesBatch: isSupportedLanguagesBatchMock,
    isSupportedLanguagesLive: isSupportedLanguagesLiveMock,
  },
}));

import {
  getLiveTranscriptionConfig,
  getOnDeviceTranscriptionConfig,
  getOnDeviceTranscriptionMode,
  getTranscriptionLanguages,
  isConfiguredSttModel,
  isSupportedLanguagesBatch,
  isSupportedLanguagesLive,
  isSupportedLocalSttModel,
} from "./capabilities";

beforeEach(() => {
  vi.clearAllMocks();
  isSupportedLanguagesLiveMock.mockResolvedValue({
    status: "ok",
    data: true,
  });
  isSupportedLanguagesBatchMock.mockResolvedValue({
    status: "ok",
    data: true,
  });
});

describe("getOnDeviceTranscriptionMode", () => {
  test("uses live mode for realtime local models", () => {
    expect(getOnDeviceTranscriptionMode("soniqo-parakeet-streaming")).toBe(
      "live",
    );
  });

  test("uses batch mode for non-realtime local models", () => {
    expect(getOnDeviceTranscriptionMode("soniqo-qwen3-small")).toBe("batch");
  });

  test("keeps live mode when realtime local model has no Soniqo-supported language", () => {
    expect(
      getOnDeviceTranscriptionMode("soniqo-parakeet-streaming", ["ko"]),
    ).toBe("live");
  });

  test("keeps European Soniqo streaming languages live", () => {
    expect(
      getOnDeviceTranscriptionMode("soniqo-parakeet-streaming", ["de"]),
    ).toBe("live");
  });
});

describe("isSupportedLocalSttModel", () => {
  test("accepts shipped local STT model families", () => {
    expect(isSupportedLocalSttModel("soniqo-parakeet-streaming")).toBe(true);
    expect(isSupportedLocalSttModel("am-parakeet-v3")).toBe(true);
    expect(isSupportedLocalSttModel("QuantizedSmallEn")).toBe(true);
  });

  test("rejects cloud, local LLM, and removed local model ids", () => {
    expect(isSupportedLocalSttModel("cloud")).toBe(false);
    expect(isSupportedLocalSttModel("Llama3p2_3bQ4")).toBe(false);
    expect(isSupportedLocalSttModel("removed-local-model")).toBe(false);
  });
});

describe("isConfiguredSttModel", () => {
  test("requires an on-device model id for the on-device provider — no cloud model exists anymore", () => {
    expect(isConfiguredSttModel("fmtr", "cloud")).toBe(false);
    expect(isConfiguredSttModel("fmtr", "soniqo-qwen3-small")).toBe(true);
    expect(isConfiguredSttModel("fmtr", "removed-local-model")).toBe(false);
  });

  test("treats any other provider string as configured (defensive default for unknown/legacy providers)", () => {
    expect(
      isConfiguredSttModel("some-legacy-provider", "whisper-large-v3"),
    ).toBe(true);
  });
});

describe("getOnDeviceTranscriptionConfig", () => {
  test("uses the first supported language for realtime local models", () => {
    expect(
      getOnDeviceTranscriptionConfig("soniqo-parakeet-streaming", ["en", "ko"]),
    ).toEqual({
      languages: ["en"],
      transcriptionMode: "live",
    });
  });

  test("keeps German live even when English is an additional language", () => {
    expect(
      getOnDeviceTranscriptionConfig("soniqo-parakeet-streaming", ["de", "en"]),
    ).toEqual({
      languages: ["de"],
      transcriptionMode: "live",
    });
  });

  test("drops unsupported Soniqo language hints instead of forcing batch", () => {
    expect(
      getOnDeviceTranscriptionConfig("soniqo-parakeet-streaming", ["ko"]),
    ).toEqual({
      languages: [],
      transcriptionMode: "live",
    });
  });
});

describe("getLiveTranscriptionConfig", () => {
  // These use a non-on-device provider string to exercise the fallback branch
  // that runs when `provider`/`model` are not a recognized on-device pair
  // (e.g. a stale config from before STT went on-device only).
  test("keeps all languages when the selected provider supports them live", async () => {
    const config = await getLiveTranscriptionConfig({
      provider: "deepgram",
      model: "nova-3-general",
      languages: ["en", "es"],
    });

    expect(config).toEqual({
      languages: ["en", "es"],
      transcriptionMode: undefined,
    });
    expect(isSupportedLanguagesLiveMock).toHaveBeenCalledTimes(1);
  });

  test("falls back to the main language when additional languages are unsupported live", async () => {
    isSupportedLanguagesLiveMock.mockImplementation(
      (_provider, _model, languages) =>
        Promise.resolve({
          status: "ok",
          data: languages.length === 1 && languages[0] === "en",
        }),
    );

    await expect(
      getLiveTranscriptionConfig({
        provider: "deepgram",
        model: "nova-3-general",
        languages: ["en", "ko"],
      }),
    ).resolves.toEqual({
      languages: ["en"],
      transcriptionMode: undefined,
    });
  });

  test("passes the provider through untouched — STT is on-device only, no Deepgram-compatibility mapping left", async () => {
    await isSupportedLanguagesLive("fmtr", "am-parakeet-v3", ["en"]);

    expect(isSupportedLanguagesLiveMock.mock.calls[0]).toEqual([
      "fmtr",
      "am-parakeet-v3",
      ["en"],
    ]);

    await isSupportedLanguagesBatch("fmtr", "am-parakeet-v3", ["en"]);

    expect(isSupportedLanguagesBatchMock.mock.calls[0]).toEqual([
      "fmtr",
      "am-parakeet-v3",
      ["en"],
    ]);
  });
});

describe("getTranscriptionLanguages", () => {
  test("prefers the main language before additional spoken languages", () => {
    expect(getTranscriptionLanguages("en", ["ko"])).toEqual(["en", "ko"]);
  });

  test("deduplicates regional variants by base language", () => {
    expect(getTranscriptionLanguages("en-US", ["en", "ko"])).toEqual([
      "en-US",
      "ko",
    ]);
  });
});
