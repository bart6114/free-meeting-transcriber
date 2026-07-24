import { describe, expect, test } from "vitest";

import { sortProviders } from "./sort-providers";

describe("sortProviders", () => {
  test("keeps the on-device provider first and Custom last", () => {
    const sorted = sortProviders([
      { id: "custom", displayName: "Custom" },
      { id: "fireworks", displayName: "Fireworks", disabled: true },
      { id: "openai", displayName: "OpenAI" },
      { id: "fmtr", displayName: "On-device" },
    ]);

    expect(sorted.map((provider) => provider.id)).toEqual([
      "fmtr",
      "openai",
      "fireworks",
      "custom",
    ]);
  });
});
