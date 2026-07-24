import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getStoredAiProvider: vi.fn(),
  getStoredSettingValues: vi.fn(),
  setSettingValues: vi.fn(async () => undefined),
}));

vi.mock("~/settings/providers", () => ({
  getStoredAiProvider: mocks.getStoredAiProvider,
}));

vi.mock("~/settings/queries", () => ({
  getStoredSettingValues: mocks.getStoredSettingValues,
  setSettingValues: mocks.setSettingValues,
}));

import { configurePaidSettings } from "./configure-paid-settings";

describe("configurePaidSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getStoredAiProvider.mockResolvedValue(undefined);
  });

  it("defaults to OpenRouter with no model when no language model is configured", async () => {
    mocks.getStoredSettingValues.mockResolvedValue({
      values: {},
      hasValues: new Set(),
    });

    await configurePaidSettings();

    expect(mocks.setSettingValues).toHaveBeenCalledWith({
      current_stt_provider: "fmtr",
      current_stt_model: "soniqo-parakeet-batch",
      current_llm_provider: "openrouter",
    });
  });

  it("falls back to the on-device default when the stored STT provider id is unknown", async () => {
    mocks.getStoredSettingValues.mockResolvedValue({
      values: {
        current_stt_provider: "stale-provider-id",
        current_stt_model: "soniqo-parakeet-batch",
        current_llm_provider: "ollama",
        current_llm_model: "llama3.2",
      },
      hasValues: new Set(),
    });

    await configurePaidSettings();

    expect(mocks.setSettingValues).toHaveBeenCalledWith({
      current_stt_provider: "fmtr",
      current_stt_model: "soniqo-parakeet-batch",
    });
  });

  it("repairs a selected provider whose required API key is missing", async () => {
    mocks.getStoredSettingValues.mockResolvedValue({
      values: {
        current_stt_provider: "fmtr",
        current_stt_model: "soniqo-parakeet-batch",
        current_llm_provider: "anthropic",
        current_llm_model: "claude-opus-4-5-20251101",
      },
      hasValues: new Set(),
    });

    await configurePaidSettings();

    expect(mocks.setSettingValues).toHaveBeenCalledWith({
      current_llm_provider: "openrouter",
    });
  });

  it("repairs to the OpenRouter default when secure provider lookup fails", async () => {
    mocks.getStoredSettingValues.mockResolvedValue({
      values: {
        current_llm_provider: "anthropic",
        current_llm_model: "claude-opus-4-5-20251101",
      },
      hasValues: new Set(),
    });
    mocks.getStoredAiProvider.mockRejectedValue(
      new Error("secure store unavailable"),
    );

    await configurePaidSettings();

    expect(mocks.setSettingValues).toHaveBeenCalledWith({
      current_stt_provider: "fmtr",
      current_stt_model: "soniqo-parakeet-batch",
      current_llm_provider: "openrouter",
    });
  });

  it("preserves a configured bring-your-own provider", async () => {
    mocks.getStoredSettingValues.mockResolvedValue({
      values: {
        current_stt_provider: "fmtr",
        current_stt_model: "soniqo-parakeet-batch",
        current_llm_provider: "anthropic",
        current_llm_model: "claude-opus-4-5-20251101",
      },
      hasValues: new Set(),
    });
    mocks.getStoredAiProvider.mockResolvedValue({
      type: "llm",
      base_url: "https://api.anthropic.com/v1",
      api_key: "anthropic-key",
    });

    await configurePaidSettings();

    expect(mocks.setSettingValues).toHaveBeenCalledWith({});
  });

  it("preserves local providers that do not require credentials", async () => {
    mocks.getStoredSettingValues.mockResolvedValue({
      values: {
        current_stt_provider: "fmtr",
        current_stt_model: "soniqo-parakeet-batch",
        current_llm_provider: "ollama",
        current_llm_model: "llama3.2",
      },
      hasValues: new Set(),
    });

    await configurePaidSettings();

    expect(mocks.setSettingValues).toHaveBeenCalledWith({});
    expect(mocks.getStoredAiProvider).not.toHaveBeenCalled();
  });
});
