import type { SearchSnippet } from "~/search/contexts/engine";

export type SnippetSegment = {
  text: string;
  highlighted: boolean;
};

// Tantivy highlight ranges are byte offsets into the UTF-8 fragment, so the
// fragment must be sliced at byte boundaries before decoding back to a string.
export function snippetSegments(snippet: SearchSnippet): SnippetSegment[] {
  const bytes = new TextEncoder().encode(snippet.fragment);
  const decoder = new TextDecoder();

  const ranges = [...snippet.highlights]
    .map(({ start, end }) => ({
      start: Math.max(0, Math.min(start, bytes.length)),
      end: Math.max(0, Math.min(end, bytes.length)),
    }))
    .filter(({ start, end }) => end > start)
    .sort((a, b) => a.start - b.start);

  const merged: { start: number; end: number }[] = [];
  for (const range of ranges) {
    const last = merged[merged.length - 1];
    if (last && range.start <= last.end) {
      last.end = Math.max(last.end, range.end);
    } else {
      merged.push({ ...range });
    }
  }

  const segments: SnippetSegment[] = [];
  let cursor = 0;
  const push = (start: number, end: number, highlighted: boolean) => {
    if (end <= start) {
      return;
    }
    const text = decoder.decode(bytes.subarray(start, end));
    if (text) {
      segments.push({ text, highlighted });
    }
  };

  for (const range of merged) {
    push(cursor, range.start, false);
    push(range.start, range.end, true);
    cursor = range.end;
  }
  push(cursor, bytes.length, false);

  return segments;
}
