import type { SearchFilters, SearchHit } from "~/search/contexts/engine/types";

export type ContactSearchResult = {
  id: string;
  name: string;
  email: string | null;
  phone: string | null;
  jobTitle: string | null;
  organization: string | null;
  memo: string | null;
};

export type WebSearchResult = {
  title: string;
  url: string;
  snippet: string;
  publishedDate?: string | null;
  author?: string | null;
};

export type WebSearchResponse = {
  status: "ok" | "error";
  message?: string;
  query: string;
  results: WebSearchResult[];
};

export interface ToolDependencies {
  search: (
    query: string,
    filters?: SearchFilters | null,
  ) => Promise<SearchHit[]>;
  getContactSearchResults: (
    query: string,
    limit: number,
  ) => Promise<ContactSearchResult[]>;
  getSessionId: () => string | undefined;
  getEnhancedNoteId: () => string | undefined;
  openEditTab: (requestId: string) => void;
  getAuthHeaders: () => Record<string, string> | null | undefined;
  fetch?: typeof fetch;
}
