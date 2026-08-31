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
  attachmentDir: vi.fn(),
  attachmentRemove: vi.fn(),
  invalidateQueries: vi.fn(),
  onFocusChanged: vi.fn(),
  openPath: vi.fn(),
  refetch: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
  uploadFile: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onFocusChanged: mocks.onFocusChanged }),
}));

vi.mock("@hypr/plugin-fs-sync", () => ({
  commands: {
    attachmentDir: mocks.attachmentDir,
    attachmentRemove: mocks.attachmentRemove,
  },
}));

vi.mock("@hypr/plugin-opener2", () => ({
  commands: { openPath: mocks.openPath },
}));

vi.mock("@hypr/ui/components/ui/toast", () => ({
  sonnerToast: {
    error: mocks.toastError,
    success: mocks.toastSuccess,
  },
}));

vi.mock("~/session/hooks/useAttachmentResolver", () => ({
  sessionAttachmentPathsQueryKey: (sessionId: string) => [
    "session",
    sessionId,
    "attachment-paths",
  ],
  useSessionAttachments: () => ({
    data: [
      {
        attachmentId: "zeta",
        path: "/vault/sessions/s1/attachments/zeta",
        extension: "",
        size: 5,
        modifiedAt: "2026-08-31T00:00:00Z",
      },
      {
        attachmentId: "Alpha.pdf",
        path: "/vault/sessions/s1/attachments/Alpha.pdf",
        extension: "pdf",
        size: 1536,
        modifiedAt: "2026-08-31T00:00:00Z",
      },
    ],
    isError: false,
    isLoading: false,
    refetch: mocks.refetch,
  }),
}));

vi.mock("~/shared/hooks/useFileUpload", () => ({
  useFileUpload: () => mocks.uploadFile,
}));

import { Attachments, formatFileSize, formatFileType } from "./attachments";

function renderAttachments(children?: ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  queryClient.invalidateQueries = mocks.invalidateQueries;
  return render(
    <QueryClientProvider client={queryClient}>
      <Attachments sessionId="s1">{children}</Attachments>
    </QueryClientProvider>,
  );
}

describe("Attachments", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.attachmentDir.mockResolvedValue({
      status: "ok",
      data: "/vault/sessions/s1/attachments",
    });
    mocks.attachmentRemove.mockResolvedValue({ status: "ok", data: null });
    mocks.onFocusChanged.mockResolvedValue(() => {});
    mocks.openPath.mockResolvedValue({ status: "ok", data: null });
    mocks.uploadFile.mockResolvedValue({
      attachmentId: "new.txt",
      path: "/vault/sessions/s1/attachments/new.txt",
      url: "asset:new.txt",
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("sorts files and shows their type and size", () => {
    renderAttachments();

    const openButtons = screen.getAllByTitle(/^Open /);
    expect(openButtons.map((button) => button.getAttribute("title"))).toEqual([
      "Open Alpha.pdf",
      "Open zeta",
    ]);
    expect(screen.getByText("PDF")).toBeTruthy();
    expect(screen.getByText("1.5 KB")).toBeTruthy();
    expect(screen.getByText("File")).toBeTruthy();
    expect(screen.getByText("5 B")).toBeTruthy();
  });

  it("opens a file and the resolved attachment directory", async () => {
    renderAttachments();

    fireEvent.click(screen.getByTitle("Open Alpha.pdf"));
    await waitFor(() =>
      expect(mocks.openPath).toHaveBeenCalledWith(
        "/vault/sessions/s1/attachments/Alpha.pdf",
        null,
      ),
    );

    fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));
    await waitFor(() =>
      expect(mocks.openPath).toHaveBeenCalledWith(
        "/vault/sessions/s1/attachments",
        null,
      ),
    );
  });

  it("adds selected and dropped files", async () => {
    const { container } = renderAttachments();
    const input = container.querySelector('input[type="file"]');
    const dropTarget = container.firstElementChild;
    const selected = new File(["selected"], "selected.txt");
    const dropped = new File(["dropped"], "dropped.csv");

    expect(dropTarget?.getAttribute("data-allow-file-drop")).toBe("true");

    fireEvent.change(input!, { target: { files: [selected] } });
    await waitFor(() =>
      expect(mocks.uploadFile).toHaveBeenCalledWith(selected),
    );

    const shouldRunBrowserDefault = fireEvent.drop(dropTarget!, {
      dataTransfer: { files: [dropped], types: ["Files"] },
    });
    expect(shouldRunBrowserDefault).toBe(false);
    await waitFor(() => expect(mocks.uploadFile).toHaveBeenCalledWith(dropped));
  });

  it("uses the full attachment view for the drop target and visual hint", () => {
    const { container } = renderAttachments(
      <div data-testid="session-metadata">Session metadata</div>,
    );
    const dropTarget = container.firstElementChild!;
    const metadata = screen.getByTestId("session-metadata");

    expect(metadata.closest("[data-allow-file-drop='true']")).toBe(dropTarget);

    fireEvent.dragEnter(metadata, {
      dataTransfer: { files: [], items: [], types: ["Files"] },
    });
    expect(screen.getByText("Drop files to attach them")).toBeTruthy();
  });

  it("confirms removal before moving an attachment to trash", async () => {
    renderAttachments();

    fireEvent.click(screen.getByRole("button", { name: "Remove Alpha.pdf" }));
    expect(screen.getByText(/Links to this file/)).toBeTruthy();
    expect(mocks.attachmentRemove).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Move to Trash" }));
    await waitFor(() =>
      expect(mocks.attachmentRemove).toHaveBeenCalledWith("s1", "Alpha.pdf"),
    );
    expect(mocks.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["session", "s1", "attachment-paths"],
    });
  });
});

describe("attachment formatting", () => {
  it("formats binary file sizes and extensionless types", () => {
    expect(formatFileSize(1024 * 1024)).toBe("1.0 MB");
    expect(formatFileSize(1024 * 1024 * 1024)).toBe("1.0 GB");
    expect(formatFileType("")).toBe("File");
    expect(formatFileType("docx")).toBe("DOCX");
  });
});
