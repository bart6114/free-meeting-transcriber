import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  attachmentSave: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset:${path}`),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: mocks.convertFileSrc,
}));

vi.mock("@hypr/plugin-fs-sync", () => ({
  commands: {
    attachmentSave: mocks.attachmentSave,
  },
}));

import { useFileUpload } from "./useFileUpload";

function renderUploadHook(sessionId: string) {
  const queryClient = new QueryClient();
  return renderHook(() => useFileUpload(sessionId), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    ),
  });
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
      uploaded = await result.current(file);
    });

    expect(uploaded).toEqual({
      path: "/vault/sessions/session-1/attachments/diagram 1.png",
      attachmentId: "diagram 1.png",
      url: "asset:/vault/sessions/session-1/attachments/diagram 1.png",
    });
    expect(mocks.attachmentSave).toHaveBeenCalledWith(
      "session-1",
      Array.from(new TextEncoder().encode("image bytes")),
      "diagram.png",
    );
  });
});
