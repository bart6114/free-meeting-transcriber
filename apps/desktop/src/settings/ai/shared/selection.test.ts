import { describe, expect, test } from "vitest";

import {
  getConfiguredProviderIds,
  getConfiguredProviders,
  getVisibleModelSelection,
} from "./selection";

describe("getVisibleModelSelection", () => {
  test("hides stale selections for an unconfigured provider", () => {
    expect(getVisibleModelSelection("openai", "gpt-5.5", false)).toEqual({
      provider: "",
      model: "",
    });
  });

  test("keeps a configured provider visible when its model is missing", () => {
    expect(getVisibleModelSelection("acme", undefined, true)).toEqual({
      provider: "acme",
      model: "",
    });
  });

  test("shows a complete configured selection", () => {
    expect(getVisibleModelSelection("openai", "gpt-5.5", true)).toEqual({
      provider: "openai",
      model: "gpt-5.5",
    });
  });
});

describe("getConfiguredProviders", () => {
  test("returns only providers whose configuration is complete", () => {
    const providers = [{ id: "acme" }, { id: "deepgram" }, { id: "openai" }];

    expect(
      getConfiguredProviders(providers, {
        acme: { configured: true },
        deepgram: { configured: true },
        openai: { configured: false },
      }),
    ).toEqual([{ id: "acme" }, { id: "deepgram" }]);
  });
});

describe("getConfiguredProviderIds", () => {
  test("keeps the configured active provider first", () => {
    const providers = [{ id: "acme" }, { id: "deepgram" }, { id: "openai" }];

    expect(
      getConfiguredProviderIds(
        providers,
        {
          acme: { configured: true },
          deepgram: { configured: true },
          openai: { configured: false },
        },
        "deepgram",
      ),
    ).toEqual(["deepgram", "acme"]);
  });

  test("falls back to configured provider order when the active provider is unavailable", () => {
    const providers = [{ id: "acme" }, { id: "deepgram" }, { id: "openai" }];

    expect(
      getConfiguredProviderIds(
        providers,
        {
          acme: { configured: true },
          deepgram: { configured: true },
          openai: { configured: false },
        },
        "openai",
      ),
    ).toEqual(["acme", "deepgram"]);
  });
});
