import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SessionDate } from "./session-date";

const mocks = vi.hoisted(() => ({
  createdAt: "2026-07-02T03:53:00.000Z" as unknown,
  updateSession: vi.fn(),
}));

const lingui = vi.hoisted(() => {
  const t = (input: TemplateStringsArray | string, ...values: unknown[]) => {
    if (typeof input === "string") {
      return input;
    }

    return Array.from(input).reduce(
      (text, part, index) => `${text}${part}${values[index] ?? ""}`,
      "",
    );
  };

  return { t };
});

vi.mock("@lingui/react/macro", () => ({
  useLingui: () => ({
    t: lingui.t,
  }),
}));

vi.mock("~/session/queries", () => ({
  useSession: () => ({ created_at: mocks.createdAt }),
  useUpdateSession: () => mocks.updateSession,
}));

describe("SessionDate", () => {
  beforeEach(() => {
    mocks.createdAt = "2026-07-02T03:53:00.000Z";
    mocks.updateSession.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders the date as a clickable label that opens the inline editor", () => {
    render(<SessionDate sessionId="session-1" />);

    fireEvent.click(screen.getByRole("button", { name: "Edit date" }));

    expect(
      document.querySelector("input[type='datetime-local']"),
    ).not.toBeNull();
    expect(
      screen.getByRole("button", { name: "Cancel date edit" }),
    ).not.toBeNull();
    expect(screen.getByRole("button", { name: "Save date" })).not.toBeNull();
  });

  it("persists the picked date and closes the editor", async () => {
    render(<SessionDate sessionId="session-1" />);

    fireEvent.click(screen.getByRole("button", { name: "Edit date" }));

    const input = document.querySelector("input[type='datetime-local']");
    fireEvent.change(input!, { target: { value: "2026-08-01T10:30" } });
    fireEvent.click(screen.getByRole("button", { name: "Save date" }));

    await waitFor(() => {
      expect(mocks.updateSession).toHaveBeenCalledTimes(1);
    });
    expect(mocks.updateSession.mock.calls[0]?.[0]).toMatchObject({
      created_at: expect.stringMatching(/^\d{4}-\d{2}-\d{2}T/),
    });
    expect(document.querySelector("input[type='datetime-local']")).toBeNull();
  });

  it("closes the editor without saving on cancel", () => {
    render(<SessionDate sessionId="session-1" />);

    fireEvent.click(screen.getByRole("button", { name: "Edit date" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel date edit" }));

    expect(mocks.updateSession).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Edit date" })).not.toBeNull();
  });
});
