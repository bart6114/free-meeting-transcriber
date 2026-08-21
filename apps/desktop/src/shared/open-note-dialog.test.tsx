import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SearchHit } from "~/search/contexts/engine";

const mocks = vi.hoisted(() => ({
  engineSearch: vi.fn(),
  openCurrent: vi.fn(),
  onOpenChange: vi.fn(),
  sessions: [] as { id: string; title: string; created_at: string }[],
  recentlyOpenedSessionIds: [] as string[],
}));

vi.mock("~/search/contexts/engine", () => ({
  useSearchEngine: () => ({ search: mocks.engineSearch }),
}));

vi.mock("~/session/queries", () => ({
  useSessionSummaries: () => mocks.sessions,
}));

vi.mock("~/store/zustand/tabs", () => ({
  useTabs: (
    selector: (state: {
      openCurrent: typeof mocks.openCurrent;
      recentlyOpenedSessionIds: string[];
    }) => unknown,
  ) =>
    selector({
      openCurrent: mocks.openCurrent,
      recentlyOpenedSessionIds: mocks.recentlyOpenedSessionIds,
    }),
}));

import { OpenNoteDialog } from "./open-note-dialog";

function sessionHit(
  id: string,
  title: string,
  fragment: string,
  highlights: { start: number; end: number }[] = [],
  type = "session",
): SearchHit {
  return {
    score: 1,
    document: {
      id,
      type: type as never,
      title,
      content: fragment,
      created_at: 0,
    },
    titleSnippet: null,
    contentSnippet: { fragment, highlights },
  };
}

function renderDialog() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <OpenNoteDialog open onOpenChange={mocks.onOpenChange} />
    </QueryClientProvider>,
  );
}

async function typeQuery(value: string) {
  fireEvent.change(screen.getByPlaceholderText("Find a note..."), {
    target: { value },
  });
}

describe("OpenNoteDialog", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
    mocks.engineSearch.mockReset().mockResolvedValue([]);
    mocks.openCurrent.mockReset();
    mocks.onOpenChange.mockReset();
    mocks.sessions = [
      {
        id: "s1",
        title: "Planning session",
        created_at: "2026-01-02T00:00:00Z",
      },
      { id: "s2", title: "Weekly sync", created_at: "2026-01-01T00:00:00Z" },
    ];
    mocks.recentlyOpenedSessionIds = ["s2"];
  });

  afterEach(() => {
    cleanup();
  });

  it("shows recent and all notes when the query is empty", () => {
    renderDialog();

    expect(screen.getByText("Recent")).toBeTruthy();
    expect(screen.getByText("All Notes")).toBeTruthy();
    expect(screen.getByText("Weekly sync")).toBeTruthy();
    expect(screen.getByText("Planning session")).toBeTruthy();
    expect(mocks.engineSearch).not.toHaveBeenCalled();
  });

  it("opens a session note from All Notes", () => {
    renderDialog();

    fireEvent.click(screen.getByRole("option", { name: "Planning session" }));

    expect(mocks.onOpenChange).toHaveBeenCalledWith(false);
    expect(mocks.openCurrent).toHaveBeenCalledWith({
      type: "sessions",
      id: "s1",
    });
  });

  it("runs a full-text search with snippets for the typed query", async () => {
    renderDialog();
    await typeQuery("zebra");

    expect(mocks.engineSearch).toHaveBeenCalledWith("zebra", null, {
      limit: 20,
      snippets: true,
      snippetMaxChars: 120,
    });
  });

  it("shows sessions matched only by content, with a highlighted snippet", async () => {
    mocks.engineSearch.mockResolvedValue([
      sessionHit("s2", "Weekly sync", "spotted a zebra today", [
        { start: 10, end: 15 },
      ]),
    ]);

    renderDialog();
    await typeQuery("zebra");

    expect(await screen.findByText("Weekly sync")).toBeTruthy();
    expect(screen.queryByText("Planning session")).toBeNull();

    const mark = await screen.findByText("zebra");
    expect(mark.tagName).toBe("MARK");
    expect(screen.getByText("spotted a")).toBeTruthy();
  });

  it("keeps title substring matches even when the engine returns nothing", async () => {
    mocks.engineSearch.mockResolvedValue([]);

    renderDialog();
    await typeQuery("plan");

    expect(await screen.findByText("Planning session")).toBeTruthy();
    expect(screen.queryByText("Weekly sync")).toBeNull();
    expect(screen.queryByText("Recent")).toBeNull();
  });

  it("dedupes sessions matching both title and content, keeping the snippet", async () => {
    mocks.engineSearch.mockResolvedValue([
      sessionHit("s2", "Weekly sync", "sync notes from standup", [
        { start: 0, end: 4 },
      ]),
    ]);

    renderDialog();
    await typeQuery("sync");

    expect(await screen.findByText("notes from standup")).toBeTruthy();
    expect(screen.getAllByText("Weekly sync")).toHaveLength(1);
  });

  it("ignores hits for unknown sessions and non-session documents", async () => {
    mocks.engineSearch.mockResolvedValue([
      sessionHit("ghost", "Deleted note", "zebra"),
      sessionHit("h1", "Some Human", "zebra", [], "human"),
    ]);

    renderDialog();
    await typeQuery("zebra");

    expect(await screen.findByText("No notes found.")).toBeTruthy();
    expect(screen.queryByText("Deleted note")).toBeNull();
    expect(screen.queryByText("Some Human")).toBeNull();
  });

  it("opens the selected session and closes the dialog", async () => {
    mocks.engineSearch.mockResolvedValue([
      sessionHit("s2", "Weekly sync", "spotted a zebra today"),
    ]);

    renderDialog();
    await typeQuery("zebra");

    fireEvent.click(await screen.findByText("Weekly sync"));

    expect(mocks.openCurrent).toHaveBeenCalledWith({
      type: "sessions",
      id: "s2",
    });
    expect(mocks.onOpenChange).toHaveBeenCalledWith(false);
  });

  it("shows the empty state when nothing matches", async () => {
    mocks.engineSearch.mockResolvedValue([]);

    renderDialog();
    await typeQuery("xyzzy");

    expect(await screen.findByText("No notes found.")).toBeTruthy();
  });

  it("caps the empty-query All Notes list to the most recent entries", () => {
    mocks.sessions = Array.from({ length: 60 }, (_, index) => ({
      id: `s-${index}`,
      title: `Note ${index}`,
      created_at: new Date(Date.UTC(2026, 0, 1, 0, index)).toISOString(),
    }));

    renderDialog();

    // Newest-first: the 50 most recent survive the cap, the 10 oldest don't.
    expect(screen.getByText("Note 59")).toBeTruthy();
    expect(screen.getByText("Note 10")).toBeTruthy();
    expect(screen.queryByText("Note 9")).toBeNull();
  });
});
