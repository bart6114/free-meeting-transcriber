import { describe, expect, it } from "vitest";

import { _PROVIDERS } from "./shared";

describe("stt providers", () => {
  it("exposes only the on-device provider", () => {
    expect(_PROVIDERS.map((p) => p.id)).toEqual(["hyprnote"]);
  });
});
