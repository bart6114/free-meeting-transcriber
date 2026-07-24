import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  exportVaultNow: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: { exportVaultNow: mocks.exportVaultNow },
}));

vi.mock("@hypr/ui/components/ui/toast", () => ({
  sonnerToast: { success: mocks.toastSuccess, error: mocks.toastError },
}));

vi.mock("@lingui/react/macro", () => ({
  Trans: ({ children }: { children?: ReactNode }) => <>{children}</>,
  useLingui: () => ({
    t: (strings: TemplateStringsArray, ...values: unknown[]) =>
      strings.reduce(
        (message, part, index) =>
          `${message}${part}${index < values.length ? String(values[index]) : ""}`,
        "",
      ),
  }),
}));

import { ReExportAllFilesRow } from "./reexport-all";

function renderRow() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <ReExportAllFilesRow />
    </QueryClientProvider>,
  );
}

describe("ReExportAllFilesRow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("enqueues a full vault export and toasts success", async () => {
    mocks.exportVaultNow.mockResolvedValue({ status: "ok", data: null });
    renderRow();

    fireEvent.click(
      screen.getByRole("button", { name: /re-export all files/i }),
    );

    await waitFor(() => expect(mocks.exportVaultNow).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalledTimes(1));
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it("surfaces a command failure as an error toast", async () => {
    mocks.exportVaultNow.mockResolvedValue({
      status: "error",
      error: "vault base unavailable",
    });
    renderRow();

    fireEvent.click(
      screen.getByRole("button", { name: /re-export all files/i }),
    );

    await waitFor(() =>
      expect(mocks.toastError).toHaveBeenCalledWith("vault base unavailable"),
    );
    expect(mocks.toastSuccess).not.toHaveBeenCalled();
  });
});
