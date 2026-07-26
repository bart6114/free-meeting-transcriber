import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getConfig: vi.fn(),
  setConfigValues: vi.fn(),
  getSecret: vi.fn(async () => ({
    status: "ok",
    data: null as string | null,
  })),
  setSecret: vi.fn(async () => ({ status: "ok", data: null })),
  deleteSecret: vi.fn(async () => ({ status: "ok", data: null })),
  repairKeychainAccess: vi.fn<
    () => Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    >
  >(async () => ({ status: "ok", data: null })),
}));

vi.mock("@hypr/plugin-settings", () => ({
  commands: {
    getConfig: mocks.getConfig,
    setConfigValues: mocks.setConfigValues,
  },
}));

vi.mock("@hypr/plugin-store2", () => ({
  commands: {
    getSecret: mocks.getSecret,
    setSecret: mocks.setSecret,
    deleteSecret: mocks.deleteSecret,
    repairKeychainAccess: mocks.repairKeychainAccess,
  },
}));

import {
  getStoredAiProvider,
  isKeychainAccessError,
  loadSecureAiProviderApiKeys,
  parseAiProviders,
  repairKeychainAccess,
  setAiProvider,
  useAiProvidersState,
} from "./providers";

import { resetConfigStoreForTests } from "~/shared/config/store";

function configWithProviders(
  aiProviders: Record<string, { type: string; base_url: string }>,
) {
  return { status: "ok", data: { ai_providers: aiProviders } };
}

describe("config-backed AI providers", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetConfigStoreForTests();
    mocks.getConfig.mockResolvedValue(configWithProviders({}));
    mocks.setConfigValues.mockResolvedValue({ status: "ok", data: null });
    mocks.getSecret.mockResolvedValue({ status: "ok", data: null });
    mocks.setSecret.mockResolvedValue({ status: "ok", data: null });
    mocks.deleteSecret.mockResolvedValue({ status: "ok", data: null });
    mocks.repairKeychainAccess.mockResolvedValue({
      status: "ok",
      data: null,
    });
  });

  it("waits for secure provider keys before reporting provider state as ready", async () => {
    let resolveSecret!: (value: { status: "ok"; data: string | null }) => void;
    mocks.getConfig.mockResolvedValue(
      configWithProviders({
        "stt:deepgram": {
          type: "stt",
          base_url: "https://api.deepgram.com/v1",
        },
      }),
    );
    mocks.getSecret.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSecret = resolve;
        }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const { result } = renderHook(() => useAiProvidersState("stt"), {
      wrapper,
    });

    expect(result.current.isReady).toBe(false);

    await waitFor(() => expect(mocks.getSecret).toHaveBeenCalledTimes(1));
    resolveSecret({ status: "ok", data: "deepgram-key" });

    await waitFor(() => expect(result.current.isReady).toBe(true));
    expect(result.current.providers["stt:deepgram"]?.api_key).toBe(
      "deepgram-key",
    );
  });

  it("returns only entries of the requested type with sane row ids", () => {
    const providers = parseAiProviders(
      {
        "llm:openai": { type: "llm", base_url: "https://direct.example" },
        "stt:deepgram": { type: "stt", base_url: "https://stt.example" },
        "llm:": { type: "llm", base_url: "https://nameless.example" },
        "llm:mismatched": { type: "stt", base_url: "https://wrong.example" },
      },
      "llm",
    );

    expect(providers).toEqual({
      "llm:openai": {
        type: "llm",
        base_url: "https://direct.example",
        api_key: "",
      },
    });
  });

  it("merges a partial write with the stored entry and other providers", async () => {
    mocks.getConfig.mockResolvedValue(
      configWithProviders({
        "llm:openai": { type: "llm", base_url: "https://old.example" },
        "stt:deepgram": { type: "stt", base_url: "https://stt.example" },
      }),
    );

    await setAiProvider("llm", "openai", { api_key: "new-key" });

    expect(mocks.setSecret).toHaveBeenCalledWith(
      "ai-provider-api-keys",
      "llm:openai",
      "new-key",
    );
    expect(mocks.setConfigValues).toHaveBeenCalledWith({
      ai_providers: {
        "llm:openai": { type: "llm", base_url: "https://old.example" },
        "stt:deepgram": { type: "stt", base_url: "https://stt.example" },
      },
    });
  });

  it("creates a new provider entry on the first write", async () => {
    await setAiProvider("stt", "deepgram", {
      base_url: "https://api.deepgram.com/v1",
    });

    expect(mocks.setConfigValues).toHaveBeenCalledWith({
      ai_providers: {
        "stt:deepgram": {
          type: "stt",
          base_url: "https://api.deepgram.com/v1",
        },
      },
    });
    // No API key was provided or previously stored, so nothing is written to
    // the keychain.
    expect(mocks.setSecret).not.toHaveBeenCalled();
    expect(mocks.deleteSecret).toHaveBeenCalledWith(
      "ai-provider-api-keys",
      "stt:deepgram",
    );
  });

  it("restores secure storage when the config write fails", async () => {
    mocks.setConfigValues.mockResolvedValue({
      status: "error",
      error: "config locked",
    });

    await expect(
      setAiProvider("llm", "openai", { api_key: "new-key" }),
    ).rejects.toThrow("config locked");

    expect(mocks.setSecret).toHaveBeenCalledWith(
      "ai-provider-api-keys",
      "llm:openai",
      "new-key",
    );
    expect(mocks.deleteSecret).toHaveBeenCalledWith(
      "ai-provider-api-keys",
      "llm:openai",
    );
  });

  it("loads secure API keys by provider ID", async () => {
    mocks.getSecret.mockResolvedValueOnce({
      status: "ok",
      data: "deepgram-key",
    });

    const apiKeys = await loadSecureAiProviderApiKeys(["stt:deepgram"], "stt");

    expect(apiKeys).toEqual({ "stt:deepgram": "deepgram-key" });
    expect(mocks.getSecret).toHaveBeenCalledWith(
      "ai-provider-api-keys",
      "stt:deepgram",
    );
    expect(mocks.setSecret).not.toHaveBeenCalled();
  });

  it("loads one stored provider with its secure API key", async () => {
    mocks.getConfig.mockResolvedValue(
      configWithProviders({
        "llm:anthropic": {
          type: "llm",
          base_url: "https://api.anthropic.com/v1",
        },
      }),
    );
    mocks.getSecret.mockResolvedValueOnce({
      status: "ok",
      data: "anthropic-key",
    });

    await expect(getStoredAiProvider("llm", "anthropic")).resolves.toEqual({
      type: "llm",
      base_url: "https://api.anthropic.com/v1",
      api_key: "anthropic-key",
    });
  });

  it("returns undefined for a provider that was never configured", async () => {
    await expect(getStoredAiProvider("llm", "openai")).resolves.toBeUndefined();
  });

  it("repairs macOS Keychain access through the secure store", async () => {
    await expect(repairKeychainAccess()).resolves.toBeUndefined();
    expect(mocks.repairKeychainAccess).toHaveBeenCalledOnce();
  });

  it("surfaces Keychain repair failures", async () => {
    mocks.repairKeychainAccess.mockResolvedValueOnce({
      status: "error",
      error: "unlock cancelled",
    });

    await expect(repairKeychainAccess()).rejects.toThrow("unlock cancelled");
  });

  it("recognizes only the recoverable macOS Keychain error", () => {
    expect(
      isKeychainAccessError(
        new Error(
          "macOS couldn't access your login Keychain. Use repair below.",
        ),
      ),
    ).toBe(true);
    expect(
      isKeychainAccessError(new Error("Platform failure: missing entitlement")),
    ).toBe(false);
  });
});
