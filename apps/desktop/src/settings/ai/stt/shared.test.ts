import { describe, expect, test } from "vitest";

import { displayModelLabel, displayModelTitle } from "./shared";

describe("STT model display labels", () => {
  test("collapses local model names to on-device labels", () => {
    expect(
      displayModelLabel(
        "soniqo-parakeet-streaming",
        "Soniqo Parakeet Streaming",
      ),
    ).toBe("On device");
    expect(
      displayModelTitle(
        "soniqo-parakeet-streaming",
        "Soniqo Parakeet Streaming",
      ),
    ).toBe("Soniqo Parakeet Streaming");
  });

  test("falls back to the raw model id when there is no display name", () => {
    expect(displayModelLabel("some-unknown-model")).toBe("some-unknown-model");
    expect(displayModelTitle("some-unknown-model")).toBeUndefined();
  });
});
