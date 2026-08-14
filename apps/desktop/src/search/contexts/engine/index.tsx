import { createContext, useCallback, useContext } from "react";

import { commands as tantivy } from "@hypr/plugin-tantivy";

import { buildTantivyFilters } from "./filters";
import type {
  SearchEntityType,
  SearchFilters,
  SearchHit,
  SearchOptions,
} from "./types";
import { normalizeQuery } from "./utils";

export type {
  SearchDocument,
  SearchEntityType,
  SearchFilters,
  SearchHit,
  SearchOptions,
  SearchSnippet,
} from "./types";

const SearchEngineContext = createContext<{
  search: (
    query: string,
    filters?: SearchFilters | null,
    options?: SearchOptions,
  ) => Promise<SearchHit[]>;
} | null>(null);

export function SearchEngineProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const search = useCallback(
    async (
      query: string,
      filters: SearchFilters | null = null,
      options: SearchOptions = {},
    ): Promise<SearchHit[]> => {
      const normalizedQuery = normalizeQuery(query);
      const tantivyFilters = buildTantivyFilters(filters);

      try {
        const result = await tantivy.search({
          query: normalizedQuery,
          filters: tantivyFilters,
          ...(options.limit !== undefined && { limit: options.limit }),
          ...(options.snippets && {
            options: {
              fuzzy: null,
              distance: null,
              phrase_slop: null,
              snippets: true,
              snippet_max_chars: options.snippetMaxChars ?? null,
            },
          }),
        });

        if (result.status === "error") {
          console.error("Search failed:", result.error);
          return [];
        }

        return result.data.hits.map((hit) => ({
          score: hit.score,
          document: {
            id: hit.document.id,
            type: hit.document.doc_type as SearchEntityType,
            title: hit.document.title,
            content: hit.document.content,
            created_at: hit.document.created_at,
          },
          titleSnippet: hit.title_snippet,
          contentSnippet: hit.content_snippet,
        }));
      } catch (error) {
        console.error("Search failed:", error);
        return [];
      }
    },
    [],
  );

  const value = {
    search,
  };

  return (
    <SearchEngineContext.Provider value={value}>
      {children}
    </SearchEngineContext.Provider>
  );
}

export function useSearchEngine() {
  const context = useContext(SearchEngineContext);
  if (!context) {
    throw new Error("useSearchEngine must be used within SearchEngineProvider");
  }
  return context;
}
