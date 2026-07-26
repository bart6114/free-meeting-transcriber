import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  templateList: vi.fn(),
  templateGet: vi.fn(),
  templateUpsert: vi.fn(),
  templateDelete: vi.fn(),
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: {
    templateList: mocks.templateList,
    templateGet: mocks.templateGet,
    templateUpsert: mocks.templateUpsert,
    templateDelete: mocks.templateDelete,
  },
}));

import {
  getTemplateById,
  useCreateTemplate,
  useUserTemplate,
  useUserTemplates,
} from "./queries";
import { DEFAULT_TEMPLATE_ICON } from "./template-icon";

function templateItem(overrides: Record<string, unknown> = {}) {
  return {
    id: "template-1",
    title: "Standup",
    description: "Daily sync",
    pinned: true,
    pin_order: 2,
    category: "meetings",
    icon: { type: "emoji", value: "☀️" },
    targets: ["engineering"],
    sections: [{ title: "Notes", description: "Capture updates" }],
    created_at: "2026-04-14T00:00:00Z",
    updated_at: "2026-04-14T00:00:00Z",
    ...overrides,
  };
}

describe("template queries", () => {
  function createWrapper() {
    const queryClient = new QueryClient({
      defaultOptions: {
        mutations: { retry: false },
        queries: { retry: false },
      },
    });

    return function Wrapper({ children }: { children: ReactNode }) {
      return (
        <QueryClientProvider client={queryClient}>
          {children}
        </QueryClientProvider>
      );
    };
  }

  afterEach(cleanup);

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.templateList.mockResolvedValue({ status: "ok", data: [] });
    mocks.templateGet.mockResolvedValue({ status: "ok", data: null });
    mocks.templateUpsert.mockResolvedValue({ status: "ok", data: null });
    mocks.templateDelete.mockResolvedValue({ status: "ok", data: null });
  });

  it("maps command items into UserTemplate for list and single reads", async () => {
    mocks.templateList.mockResolvedValue({
      status: "ok",
      data: [templateItem()],
    });
    mocks.templateGet.mockResolvedValue({
      status: "ok",
      data: templateItem(),
    });

    const wrapper = createWrapper();
    const { result: templatesResult } = renderHook(() => useUserTemplates(), {
      wrapper,
    });
    const { result: templateResult } = renderHook(
      () => useUserTemplate("template-1"),
      { wrapper },
    );

    const expected = {
      id: "template-1",
      title: "Standup",
      description: "Daily sync",
      pinned: true,
      pinOrder: 2,
      category: "meetings",
      icon: { type: "emoji", value: "☀️" },
      targets: ["engineering"],
      sections: [{ title: "Notes", description: "Capture updates" }],
    };

    await waitFor(() => {
      expect(templatesResult.current).toEqual([expected]);
      expect(templateResult.current.data).toEqual(expected);
    });
    expect(mocks.templateGet).toHaveBeenCalledWith("template-1");
  });

  it("keeps templates visible when stored template JSON is invalid", async () => {
    mocks.templateList.mockResolvedValue({
      status: "ok",
      data: [
        templateItem({
          id: "template-1",
          title: "Draft Template",
          description: "",
          pinned: false,
          pin_order: null,
          category: null,
          icon: "{",
          targets: "{",
          sections: [{ title: "", description: "" }],
        }),
      ],
    });

    const { result } = renderHook(() => useUserTemplates(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current).toEqual([
        {
          id: "template-1",
          title: "Draft Template",
          description: "",
          pinned: false,
          pinOrder: undefined,
          category: undefined,
          icon: DEFAULT_TEMPLATE_ICON,
          targets: undefined,
          sections: [{ title: "", description: "" }],
        },
      ]);
    });
  });

  it("resolves getTemplateById through the template_get command", async () => {
    mocks.templateGet.mockResolvedValue({
      status: "ok",
      data: templateItem({
        pinned: false,
        pin_order: null,
        category: null,
        icon: { type: "icon", value: "target", color: "#5b67d8" },
      }),
    });

    await expect(getTemplateById("template-1")).resolves.toEqual({
      id: "template-1",
      title: "Standup",
      description: "Daily sync",
      pinned: false,
      pinOrder: undefined,
      category: undefined,
      icon: { type: "icon", value: "target", color: "#5b67d8" },
      targets: ["engineering"],
      sections: [{ title: "Notes", description: "Capture updates" }],
    });
    expect(mocks.templateGet).toHaveBeenCalledWith("template-1");
  });

  it("returns null without a command round-trip for an empty id", async () => {
    await expect(getTemplateById("")).resolves.toBeNull();
    expect(mocks.templateGet).not.toHaveBeenCalled();
  });

  it("creates a template through the template_upsert command", async () => {
    const { result } = renderHook(() => useCreateTemplate(), {
      wrapper: createWrapper(),
    });

    let createdId: string | undefined;
    await act(async () => {
      createdId = await result.current({
        title: "New Template",
        description: "",
        sections: [],
      });
    });

    expect(createdId).toEqual(expect.any(String));
    expect(mocks.templateUpsert).toHaveBeenCalledWith({
      id: createdId,
      title: "New Template",
      description: "",
      pinned: false,
      pin_order: null,
      category: null,
      icon: DEFAULT_TEMPLATE_ICON,
      targets: null,
      sections: [],
    });
  });

  it("surfaces command errors from mutations", async () => {
    mocks.templateUpsert.mockResolvedValue({
      status: "error",
      error: "invalid template id",
    });
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});

    const { result } = renderHook(() => useCreateTemplate(), {
      wrapper: createWrapper(),
    });

    await expect(
      act(() =>
        result.current({ title: "Broken", description: "", sections: [] }),
      ),
    ).rejects.toThrow("invalid template id");
    consoleError.mockRestore();
  });
});
