import { describe, expect, it } from "vitest";

import { shouldShowSessionTopAudioPlayer } from "./top-audio-player";

describe("shouldShowSessionTopAudioPlayer", () => {
  it("shows playback whenever a recording is ready", () => {
    expect(
      shouldShowSessionTopAudioPlayer({
        audioExists: true,
        audioUrlReady: true,
        sessionMode: "inactive",
      }),
    ).toBe(true);

    expect(
      shouldShowSessionTopAudioPlayer({
        audioExists: false,
        audioUrlReady: true,
        sessionMode: "inactive",
      }),
    ).toBe(false);

    expect(
      shouldShowSessionTopAudioPlayer({
        audioExists: true,
        audioUrlReady: false,
        sessionMode: "inactive",
      }),
    ).toBe(false);
  });

  it("keeps playback hidden while recording or finalizing", () => {
    expect(
      shouldShowSessionTopAudioPlayer({
        audioExists: true,
        audioUrlReady: true,
        sessionMode: "active",
      }),
    ).toBe(false);

    expect(
      shouldShowSessionTopAudioPlayer({
        audioExists: true,
        audioUrlReady: true,
        sessionMode: "finalizing",
      }),
    ).toBe(false);
  });
});
