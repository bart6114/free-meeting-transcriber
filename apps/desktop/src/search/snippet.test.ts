import { describe, expect, it } from "vitest";

import { snippetSegments } from "./snippet";

describe("snippetSegments", () => {
  it("splits a fragment into plain and highlighted segments", () => {
    expect(
      snippetSegments({
        fragment: "quarterly zebra review",
        highlights: [{ start: 10, end: 15 }],
      }),
    ).toEqual([
      { text: "quarterly ", highlighted: false },
      { text: "zebra", highlighted: true },
      { text: " review", highlighted: false },
    ]);
  });

  it("returns the whole fragment unhighlighted when there are no highlights", () => {
    expect(snippetSegments({ fragment: "plain text", highlights: [] })).toEqual(
      [{ text: "plain text", highlighted: false }],
    );
  });

  it("returns no segments for an empty fragment", () => {
    expect(snippetSegments({ fragment: "", highlights: [] })).toEqual([]);
  });

  it("treats highlight offsets as byte offsets in multi-byte text", () => {
    // "héllo zebra" -- "é" is two bytes in UTF-8, so "zebra" starts at byte 7.
    expect(
      snippetSegments({
        fragment: "héllo zebra",
        highlights: [{ start: 7, end: 12 }],
      }),
    ).toEqual([
      { text: "héllo ", highlighted: false },
      { text: "zebra", highlighted: true },
    ]);
  });

  it("merges overlapping and adjacent ranges", () => {
    expect(
      snippetSegments({
        fragment: "abcdefgh",
        highlights: [
          { start: 2, end: 5 },
          { start: 4, end: 6 },
          { start: 6, end: 7 },
        ],
      }),
    ).toEqual([
      { text: "ab", highlighted: false },
      { text: "cdefg", highlighted: true },
      { text: "h", highlighted: false },
    ]);
  });

  it("sorts out-of-order ranges", () => {
    expect(
      snippetSegments({
        fragment: "one two three",
        highlights: [
          { start: 8, end: 13 },
          { start: 0, end: 3 },
        ],
      }),
    ).toEqual([
      { text: "one", highlighted: true },
      { text: " two ", highlighted: false },
      { text: "three", highlighted: true },
    ]);
  });

  it("clamps ranges that run past the end of the fragment", () => {
    expect(
      snippetSegments({
        fragment: "short",
        highlights: [{ start: 3, end: 50 }],
      }),
    ).toEqual([
      { text: "sho", highlighted: false },
      { text: "rt", highlighted: true },
    ]);
  });

  it("drops empty and inverted ranges", () => {
    expect(
      snippetSegments({
        fragment: "abc",
        highlights: [
          { start: 1, end: 1 },
          { start: 2, end: 1 },
        ],
      }),
    ).toEqual([{ text: "abc", highlighted: false }]);
  });
});
