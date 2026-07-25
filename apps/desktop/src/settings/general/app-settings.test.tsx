import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppSettingsView } from "./app-settings";

function setting(value = true) {
  return {
    value,
    onChange: vi.fn(),
  };
}

function renderAppSettings({
  floatingBar = true,
  meetingDisclosureAutoPost = setting(),
} = {}) {
  return {
    ...render(
      <AppSettingsView
        autostart={setting()}
        autoStopMeetings={setting()}
        floatingBar={setting(floatingBar)}
        showAppInDock={setting()}
        showTrayIcon={setting()}
        telemetryConsent={setting()}
        meetingDisclosureAutoPost={meetingDisclosureAutoPost}
        audioRetention={{ value: "forever", onChange: vi.fn() }}
      />,
    ),
    meetingDisclosureAutoPost,
  };
}

describe("AppSettingsView", () => {
  afterEach(() => {
    cleanup();
  });

  it("does not expose a separate live transcript overlay setting", () => {
    renderAppSettings();

    expect(screen.queryByText("Show live transcript overlay")).toBeNull();
  });

  it("keeps the floating bar setting available", () => {
    renderAppSettings({ floatingBar: false });

    expect(screen.getByText("Show floating bar")).toBeTruthy();
  });

  it("updates the recording disclosure setting from the meetings switch", () => {
    const meetingDisclosureAutoPost = setting(false);
    renderAppSettings({ meetingDisclosureAutoPost });

    fireEvent.click(
      screen.getByRole("switch", {
        name: "Post recording disclosure in meeting chat",
      }),
    );

    expect(meetingDisclosureAutoPost.onChange).toHaveBeenCalledWith(true);
  });

  it("clarifies that a recording disclosure does not confirm consent", () => {
    renderAppSettings();

    expect(
      screen.getByText(/active meeting chat supports safe posting/),
    ).toBeTruthy();
    expect(
      screen.getByText(/A disclosure does not confirm participant consent/),
    ).toBeTruthy();
  });
});
