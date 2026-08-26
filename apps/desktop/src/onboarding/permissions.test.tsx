import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const createPermission = () => ({
    status: "denied" as "authorized" | "denied" | "neverRequested",
    isPending: false,
    open: vi.fn(),
    request: vi.fn(),
    reset: vi.fn(),
  });
  const permissions = {
    microphone: createPermission(),
    systemAudio: createPermission(),
  };

  return {
    permissions,
    usePermission: vi.fn((type: keyof typeof permissions) => permissions[type]),
  };
});

const lingui = vi.hoisted(() => ({
  t: (input: TemplateStringsArray, ...values: unknown[]) =>
    input.reduce(
      (message, part, index) =>
        `${message}${part}${index < values.length ? String(values[index]) : ""}`,
      "",
    ),
}));

vi.mock("@lingui/react/macro", () => ({
  useLingui: () => ({ t: lingui.t }),
}));

vi.mock("~/shared/hooks/usePermissions", () => ({
  usePermission: mocks.usePermission,
}));

import { PermissionsSection } from "./permissions";

afterEach(cleanup);

describe("PermissionsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    Object.values(mocks.permissions).forEach((permission) => {
      permission.status = "denied";
      permission.isPending = false;
    });
  });

  it("collects microphone and system audio permissions", () => {
    const { container } = render(<PermissionsSection />);

    expect(screen.getByText("Help Loofah listen to you")).toBeTruthy();
    expect(screen.getByText("Help Loofah listen to others")).toBeTruthy();
    expect(container.querySelectorAll(".lucide-arrow-right")).toHaveLength(2);
  });

  it("waits for both audio permissions before continuing", () => {
    const onContinue = vi.fn();
    mocks.permissions.microphone.status = "authorized";

    const view = render(<PermissionsSection onContinue={onContinue} />);

    expect(onContinue).not.toHaveBeenCalled();

    mocks.permissions.systemAudio.status = "authorized";
    view.rerender(<PermissionsSection onContinue={onContinue} />);

    expect(onContinue).toHaveBeenCalledTimes(1);

    view.rerender(<PermissionsSection onContinue={onContinue} />);

    expect(onContinue).toHaveBeenCalledTimes(1);
  });
});
