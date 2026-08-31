import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  attachmentImportPath: vi.fn(),
  attachmentSave: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset:${path}`),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: mocks.convertFileSrc,
}));

vi.mock("@hypr/plugin-fs-sync", () => ({
  commands: {
    attachmentImportPath: mocks.attachmentImportPath,
    attachmentSave: mocks.attachmentSave,
  },
}));

import { useFileUpload } from "./useFileUpload";

function renderUploadHook(sessionId: string) {
  const queryClient = new QueryClient();
  const hook = renderHook(() => useFileUpload(sessionId), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    ),
  });
  return { ...hook, queryClient };
}

function uploadFile() {
  const bytes = new TextEncoder().encode("image bytes").buffer;
  return {
    name: "diagram.png",
    type: "image/png",
    size: bytes.byteLength,
    arrayBuffer: vi.fn().mockResolvedValue(bytes),
  } as unknown as File;
}

describe("useFileUpload", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.attachmentSave.mockResolvedValue({
      status: "ok",
      data: {
        path: "/vault/sessions/session-1/attachments/diagram 1.png",
        attachmentId: "diagram 1.png",
      },
    });
  });

  it("saves the attachment and returns its asset URL", async () => {
    const file = uploadFile();
    const { result } = renderUploadHook("session-1");
    let uploaded: Awaited<ReturnType<typeof result.current>> | undefined;

    await act(async () => {
      uploaded = await result.current({
        kind: "file",
        file,
        name: file.name,
      });
    });

    expect(uploaded).toEqual({
      path: "/vault/sessions/session-1/attachments/diagram 1.png",
      attachmentId: "diagram 1.png",
      url: "asset:/vault/sessions/session-1/attachments/diagram 1.png",
      attachment: {
        path: "/vault/sessions/session-1/attachments/diagram 1.png",
        attachmentId: "diagram 1.png",
        extension: "png",
        size: 11,
        modifiedAt: expect.any(String),
      },
    });
    expect(mocks.attachmentSave).toHaveBeenCalledWith(
      "session-1",
      Array.from(new TextEncoder().encode("image bytes")),
      "diagram.png",
    );
  });

  it("imports native paths without reading a File and updates the cache", async () => {
    mocks.attachmentImportPath.mockResolvedValue({
      status: "ok",
      data: {
        path: "/vault/sessions/session-1/attachments/archive.zip",
        attachmentId: "archive.zip",
        extension: "zip",
        size: 4_000_000_000,
        modifiedAt: "2026-08-31T10:00:00.000Z",
      },
    });
    const { result, queryClient } = renderUploadHook("session-1");

    await act(async () => {
      await result.current({
        kind: "path",
        path: "/Users/me/archive.zip",
        name: "archive.zip",
      });
    });

    expect(mocks.attachmentImportPath).toHaveBeenCalledWith(
      "session-1",
      "/Users/me/archive.zip",
    );
    expect(mocks.attachmentSave).not.toHaveBeenCalled();
    expect(
      queryClient.getQueryData(["session", "session-1", "attachment-paths"]),
    ).toEqual([
      expect.objectContaining({
        attachmentId: "archive.zip",
        size: 4_000_000_000,
      }),
    ]);
  });

  it("runs imports sequentially in submission order", async () => {
    let resolveFirst!: (value: any) => void;
    mocks.attachmentImportPath
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockResolvedValueOnce({
        status: "ok",
        data: attachment("second.txt"),
      });
    const { result } = renderUploadHook("session-1");

    let first!: Promise<unknown>;
    let second!: Promise<unknown>;
    act(() => {
      first = result.current({
        kind: "path",
        path: "/tmp/first.txt",
        name: "first.txt",
      });
      second = result.current({
        kind: "path",
        path: "/tmp/second.txt",
        name: "second.txt",
      });
    });
    await waitFor(() =>
      expect(mocks.attachmentImportPath).toHaveBeenCalledTimes(1),
    );

    await act(async () => {
      resolveFirst({ status: "ok", data: attachment("first.txt") });
      await Promise.all([first, second]);
    });
    expect(
      mocks.attachmentImportPath.mock.calls.map((call) => call[1]),
    ).toEqual(["/tmp/first.txt", "/tmp/second.txt"]);
  });
});

function attachment(name: string) {
  return {
    path: `/vault/attachments/${name}`,
    attachmentId: name,
    extension: "txt",
    size: 1,
    modifiedAt: "2026-08-31T10:00:00.000Z",
  };
}
