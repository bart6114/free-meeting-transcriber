import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  search: vi.fn(),
}));

vi.mock("@hypr/plugin-tantivy", () => ({
  commands: {
    search: mocks.search,
  },
}));

import { SearchEngineProvider, useSearchEngine } from "./index";

function renderEngine() {
  return renderHook(() => useSearchEngine(), {
    wrapper: ({ children }) => (
      <SearchEngineProvider>{children}</SearchEngineProvider>
    ),
  });
}

describe("useSearchEngine", () => {
  beforeEach(() => {
    mocks.search.mockReset();
    mocks.search.mockResolvedValue({
      status: "ok",
      data: { hits: [], count: 0 },
    });
  });

  it("normalizes the query and omits options by default", async () => {
    const { result } = renderEngine();
    await result.current.search("  hello   world  ");

    expect(mocks.search).toHaveBeenCalledWith({
      query: "hello world",
      filters: undefined,
    });
  });

  it("passes limit and snippet options through to the plugin", async () => {
    const { result } = renderEngine();
    await result.current.search("zebra", null, {
      limit: 20,
      snippets: true,
      snippetMaxChars: 120,
    });

    expect(mocks.search).toHaveBeenCalledWith({
      query: "zebra",
      filters: undefined,
      limit: 20,
      options: {
        fuzzy: null,
        distance: null,
        phrase_slop: null,
        snippets: true,
        snippet_max_chars: 120,
      },
    });
  });

  it("maps documents and snippets from plugin hits", async () => {
    mocks.search.mockResolvedValue({
      status: "ok",
      data: {
        count: 1,
        hits: [
          {
            score: 2.5,
            document: {
              id: "s1",
              doc_type: "session",
              language: null,
              title: "Planning",
              content: "zebra sighting",
              created_at: 1234,
            },
            title_snippet: null,
            content_snippet: {
              fragment: "zebra sighting",
              highlights: [{ start: 0, end: 5 }],
            },
          },
        ],
      },
    });

    const { result } = renderEngine();
    const hits = await result.current.search("zebra");

    expect(hits).toEqual([
      {
        score: 2.5,
        document: {
          id: "s1",
          type: "session",
          title: "Planning",
          content: "zebra sighting",
          created_at: 1234,
        },
        titleSnippet: null,
        contentSnippet: {
          fragment: "zebra sighting",
          highlights: [{ start: 0, end: 5 }],
        },
      },
    ]);
  });

  it("returns an empty list when the plugin reports an error", async () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    mocks.search.mockResolvedValue({ status: "error", error: "boom" });

    const { result } = renderEngine();
    await expect(result.current.search("zebra")).resolves.toEqual([]);
    consoleError.mockRestore();
  });
});
