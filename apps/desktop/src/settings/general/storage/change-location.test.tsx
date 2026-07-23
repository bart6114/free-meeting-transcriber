import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  vaultBase: vi.fn(),
  copyVault: vi.fn(),
  setVaultBase: vi.fn(),
  isEmptyOrMissingDir: vi.fn(),
  obsidianVaults: vi.fn(),
  selectFolder: vi.fn(),
  message: vi.fn(),
  openPath: vi.fn(),
  scheduleAutomaticRelaunch: vi.fn(),
  toastError: vi.fn(),
  homeDir: vi.fn(),
}));

vi.mock("@hypr/plugin-settings", () => ({
  commands: {
    vaultBase: mocks.vaultBase,
    copyVault: mocks.copyVault,
    setVaultBase: mocks.setVaultBase,
    isEmptyOrMissingDir: mocks.isEmptyOrMissingDir,
    obsidianVaults: mocks.obsidianVaults,
  },
}));

vi.mock("@hypr/plugin-opener2", () => ({
  commands: { openPath: mocks.openPath },
}));

vi.mock("@hypr/ui/components/ui/toast", () => ({
  sonnerToast: { error: mocks.toastError },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: mocks.homeDir,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: mocks.selectFolder,
  message: mocks.message,
}));

vi.mock("~/shared/relaunch", () => ({
  scheduleAutomaticRelaunch: mocks.scheduleAutomaticRelaunch,
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

import { ChangeLocationRow } from "./change-location";

function renderRow() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });

  return {
    queryClient,
    ...render(
      <QueryClientProvider client={queryClient}>
        <ChangeLocationRow />
      </QueryClientProvider>,
    ),
  };
}

describe("ChangeLocationRow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.vaultBase.mockResolvedValue({
      status: "ok",
      data: "/Users/x/Drive/vault",
    });
    mocks.copyVault.mockResolvedValue({ status: "ok", data: null });
    mocks.setVaultBase.mockResolvedValue({ status: "ok", data: null });
    mocks.isEmptyOrMissingDir.mockResolvedValue({ status: "ok", data: true });
    mocks.obsidianVaults.mockResolvedValue({ status: "ok", data: [] });
    mocks.homeDir.mockResolvedValue("/Users/x");
    mocks.scheduleAutomaticRelaunch.mockResolvedValue("scheduled");
  });

  afterEach(() => {
    cleanup();
  });

  it("shows the current vault path and a change button", async () => {
    renderRow();
    expect(await screen.findByText(/Drive\/vault/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /change/i })).toBeTruthy();
  });

  it("opens a confirm dialog after picking a new folder and applies the change", async () => {
    mocks.selectFolder.mockResolvedValue("/Users/x/Desktop/new-vault");
    renderRow();
    await screen.findByText(/Drive\/vault/);

    fireEvent.click(screen.getByRole("button", { name: /change/i }));

    await waitFor(() => expect(mocks.selectFolder).toHaveBeenCalledTimes(1));

    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(
        /App restarts to apply\. Existing files are copied, not moved\./,
      ),
    ).toBeTruthy();

    fireEvent.click(
      within(dialog).getByRole("button", { name: /^change$/i }),
    );

    await waitFor(() =>
      expect(mocks.copyVault).toHaveBeenCalledWith(
        "/Users/x/Desktop/new-vault",
      ),
    );
    await waitFor(() =>
      expect(mocks.setVaultBase).toHaveBeenCalledWith(
        "/Users/x/Desktop/new-vault",
      ),
    );
    await waitFor(() =>
      expect(mocks.scheduleAutomaticRelaunch).toHaveBeenCalledTimes(1),
    );
  });

  it("surfaces a validate_vault_base_change failure as a toast and inline error", async () => {
    mocks.selectFolder.mockResolvedValue("/Users/x/Drive/vault/nested");
    mocks.copyVault.mockResolvedValue({
      status: "error",
      error: "New location is a subdirectory of the current vault",
    });
    renderRow();
    await screen.findByText(/Drive\/vault/);

    fireEvent.click(screen.getByRole("button", { name: /change/i }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: /^change$/i }),
    );

    await waitFor(() =>
      expect(
        within(dialog).getByText(
          "New location is a subdirectory of the current vault",
        ),
      ).toBeTruthy(),
    );
    expect(mocks.toastError).toHaveBeenCalledWith(
      "New location is a subdirectory of the current vault",
    );
    expect(mocks.setVaultBase).not.toHaveBeenCalled();
    expect(mocks.scheduleAutomaticRelaunch).not.toHaveBeenCalled();
  });

  it("offers detected Obsidian vaults as quick picks", async () => {
    mocks.obsidianVaults.mockResolvedValue({
      status: "ok",
      data: [{ path: "/Users/x/Documents/MyVault" }],
    });
    renderRow();
    await screen.findByText(/Drive\/vault/);

    const quickPick = await screen.findByRole("button", {
      name: /MyVault/,
    });
    fireEvent.click(quickPick);

    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(/Documents\/MyVault/),
    ).toBeTruthy();
  });
});
