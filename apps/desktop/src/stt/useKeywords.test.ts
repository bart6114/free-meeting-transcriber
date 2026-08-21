import { beforeEach, describe, expect, it, vi } from "vitest";

const sessionGet = vi.hoisted(() => vi.fn());

vi.mock("~/types/tauri.gen", () => ({
  commands: { sessionGet },
}));

vi.mock("~/session/queries", () => ({
  useSession: vi.fn(() => null),
}));

import {
  formatDictionaryTerms,
  normalizeKeywordList,
  parseDictionaryTermsText,
} from "./keywords";
import {
  buildKeywords,
  buildKeywordSourceText,
  extractKeywordsFromMarkdown,
  getSessionKeywords,
} from "./useKeywords";

beforeEach(() => {
  sessionGet.mockReset();
  sessionGet.mockResolvedValue({ status: "ok", data: null });
});

describe("extractKeywordsFromMarkdown", () => {
  const cases: Array<{
    description: string;
    input: string;
    keywords: string[];
    keyphrases: string[];
  }> = [
    {
      description: "extracts hashtags from markdown",
      input: "This is #awesome and #cool stuff",
      keywords: ["awesome", "cool"],
      keyphrases: [],
    },
    {
      description: "handles unicode hashtags",
      input: "#日本語 #한글 #Español",
      keywords: ["日本語", "한글", "Español"],
      keyphrases: [],
    },
    {
      description: "excludes code blocks and inline code",
      input:
        "Use the `useState` hook\n```js\nconst value = 1;\n```\nKeep reading for keywords",
      keywords: ["hook", "keywords"],
      keyphrases: [],
    },
    {
      description: "extracts hashtags and keywords from markdown",
      input:
        "# Hello World\n\nThis is a #test with some #keywords and important content",
      keywords: ["test", "keywords", "World", "content"],
      keyphrases: [],
    },
    {
      description: "removes code blocks before processing",
      input:
        "Text before\n```js\nconst x = 1;\n```\nText after with important keywords",
      keywords: ["Text", "keywords"],
      keyphrases: [],
    },
    {
      description: "extracts keywords from natural language",
      input:
        "Artificial intelligence and machine learning are transforming technology",
      keywords: ["technology"],
      keyphrases: ["Artificial intelligence"],
    },
    {
      description: "extracts keyphrases",
      input: "I'm learning about machine learning and data science in depth",
      keywords: [],
      keyphrases: ["data science"],
    },
    {
      description: "handles complex real-world markdown",
      input: `
# Meeting Notes

We discussed machine learning and its applications in production systems.

\`\`\`python
def hello():
    print("hello")
\`\`\`

Key points:
- Review the #projectA deliverables
- Implement data science solutions
- Team collaboration is essential

Next steps: testing and validation of the algorithms
    `,
      keywords: ["projectA"],
      keyphrases: ["production systems"],
    },
    {
      description: "handles empty text gracefully",
      input: "",
      keywords: [],
      keyphrases: [],
    },
    {
      description: "filters single-character tokens",
      input: "a b c",
      keywords: [],
      keyphrases: [],
    },
    {
      description: "comma & space",
      input: "yujonglee,john, atila",
      keywords: ["yujonglee", "john", "atila"],
      keyphrases: [],
    },
  ];

  it.each(cases)("$description", ({ input, keywords, keyphrases }) => {
    const result = extractKeywordsFromMarkdown(input);
    expect(result.keywords).toEqual(expect.arrayContaining(keywords));
    expect(result.keyphrases).toEqual(expect.arrayContaining(keyphrases));
  });
});

describe("buildKeywordSourceText", () => {
  it("includes session note and title", () => {
    expect(
      buildKeywordSourceText({
        rawMd: "Discuss product launch",
        title: "Erebor sync",
      }),
    ).toBe(["Discuss product launch", "Erebor sync"].join("\n"));
  });
});

describe("getSessionKeywords", () => {
  it("builds keywords from the current session snapshot", async () => {
    sessionGet.mockResolvedValue({
      status: "ok",
      data: {
        meta: {
          id: "session-1",
          title: "Erebor sync",
        },
        note_markdown: "Discuss #Launch and production systems",
      },
    });

    await expect(
      getSessionKeywords({
        sessionId: "session-1",
        dictionaryTerms: ["Vertex"],
      }),
    ).resolves.toEqual(expect.arrayContaining(["Vertex", "Launch"]));
    expect(sessionGet).toHaveBeenCalledWith("session-1");
  });
});

describe("buildKeywords", () => {
  it("dedupes higher-priority hints and caps the result", () => {
    const result = buildKeywords({
      rawMd: "",
      title: "",
      dictionaryTerms: Array.from(
        { length: 60 },
        (_, index) => `Term ${index}`,
      ),
    });

    expect(result).toHaveLength(50);
    expect(result.slice(0, 2)).toEqual(["Term 0", "Term 1"]);
  });
});

describe("dictionary term helpers", () => {
  it("parses newline and comma separated terms", () => {
    expect(
      parseDictionaryTermsText("Vertex\nFastConformer, Parakeet TDT"),
    ).toEqual(["Vertex", "FastConformer", "Parakeet TDT"]);
  });

  it("normalizes duplicate terms while preserving first spelling", () => {
    expect(normalizeKeywordList(["Vertex", " vertex ", "Parakeet"])).toEqual([
      "Vertex",
      "Parakeet",
    ]);
  });

  it("formats stored terms one per line", () => {
    expect(formatDictionaryTerms(["Vertex", "Parakeet TDT"])).toBe(
      "Vertex\nParakeet TDT",
    );
  });
});
