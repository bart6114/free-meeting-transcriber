import { Effect, Option, pipe } from "effect";
import type { UnknownException } from "effect/Cause";
import { toString } from "nlcst-to-string";
import { useMemo } from "react";
import retextEnglish from "retext-english";
import type { Keyphrase, Keyword } from "retext-keywords";
import retextKeywords from "retext-keywords";
import retextPos from "retext-pos";
import retextStringify from "retext-stringify";
import { unified } from "unified";
import type { VFile } from "vfile";

import { useSession } from "~/session/queries";
import { useConfigValue } from "~/shared/config";
import { normalizeKeywordList } from "~/stt/keywords";
import { commands } from "~/types/tauri.gen";

const MAX_TRANSCRIPTION_HINTS = 50;

export function useKeywords(sessionId: string) {
  const session = useSession(sessionId);
  const dictionaryTerms = useConfigValue("personalization_dictionary_terms");

  return useMemo(
    () =>
      buildKeywords({
        rawMd: session?.raw_md,
        title: session?.title,
        dictionaryTerms,
      }),
    [dictionaryTerms, session],
  );
}

export async function getSessionKeywords({
  sessionId,
  dictionaryTerms,
}: {
  sessionId: string;
  dictionaryTerms: string[];
}): Promise<string[]> {
  const result = await commands.sessionGet(sessionId);
  if (result.status === "error") {
    throw new Error(result.error);
  }

  const session = result.data;
  return buildKeywords({
    rawMd: session?.note_markdown ?? undefined,
    title: session?.meta.title,
    dictionaryTerms,
  });
}

export function buildKeywords({
  rawMd,
  title,
  dictionaryTerms,
}: {
  rawMd: unknown;
  title: unknown;
  dictionaryTerms: string[];
}) {
  const sourceText = buildKeywordSourceText({
    rawMd,
    title,
  });
  const { keywords, keyphrases } =
    sourceText.length > 0
      ? extractKeywordsFromMarkdown(sourceText)
      : { keywords: [], keyphrases: [] };

  return normalizeKeywordList([
    ...dictionaryTerms,
    ...keywords,
    ...keyphrases,
  ]).slice(0, MAX_TRANSCRIPTION_HINTS);
}

export function buildKeywordSourceText({
  rawMd,
  title,
}: {
  rawMd: unknown;
  title: unknown;
}): string {
  return [stringValue(rawMd), stringValue(title)]
    .filter((value) => value.length > 0)
    .join("\n");
}

export const extractKeywordsFromMarkdown = (
  markdown: string,
): { keywords: string[]; keyphrases: string[] } =>
  pipe(
    Effect.succeed(markdown),
    Effect.map(removeCodeBlocks),
    Effect.map((text) => ({
      hashtags: extractHashtags(text),
      cleaned: stripMarkdownFormatting(text),
    })),
    Effect.flatMap(({ cleaned, hashtags }) =>
      cleaned.trim().length === 0
        ? Effect.succeed({ keywords: hashtags, keyphrases: [] })
        : pipe(
            processMarkdown(cleaned),
            Effect.map((file) => gatherKeywords(file, hashtags)),
            Effect.orElse(() =>
              Effect.succeed({
                keywords: hashtags,
                keyphrases: [],
              }),
            ),
          ),
    ),
    Effect.runSync,
  );

const processMarkdown = (
  markdown: string,
): Effect.Effect<VFile, UnknownException, never> =>
  Effect.try(() =>
    unified()
      .use(retextEnglish)
      .use(retextPos)
      .use(retextKeywords, { maximum: 50 })
      .use(retextStringify)
      .processSync(markdown),
  );

const gatherKeywords = (
  file: VFile,
  hashtags: string[],
): { keywords: string[]; keyphrases: string[] } => {
  const keywords = pipe(
    Option.fromNullable(file.data.keywords),
    Option.map((entries) => entries.flatMap(extractKeywordMatches)),
    Option.getOrElse(() => [] as string[]),
  );

  const keyphrases = pipe(
    Option.fromNullable(file.data.keyphrases),
    Option.map((entries) => entries.flatMap(extractKeyphraseMatches)),
    Option.getOrElse(() => [] as string[]),
  );

  return {
    keywords: [...hashtags, ...keywords].filter(
      (keyword) => keyword.length >= 2,
    ),
    keyphrases: keyphrases.filter((phrase) => phrase.length >= 2),
  };
};

const extractKeywordMatches = (keyword: Keyword): string[] =>
  keyword.matches.flatMap((match) => {
    const text = toString(match.node).trim();
    return text.length > 0 ? [text] : [];
  });

const extractKeyphraseMatches = (phrase: Keyphrase): string[] =>
  phrase.matches.flatMap((match) => {
    const text = toString(match.nodes).trim();
    return text.length > 0 ? [text] : [];
  });

const stringValue = (value: unknown): string =>
  typeof value === "string" ? value.trim() : "";

const removeCodeBlocks = (text: string): string =>
  text.replace(/```[\s\S]*?```/g, "").replace(/`[^`]+`/g, "");

const extractHashtags = (text: string): string[] =>
  Array.from(text.matchAll(/#([\p{L}\p{N}_]+)/gu), (match) => match[1]).filter(
    Boolean,
  );

const stripMarkdownFormatting = (text: string): string =>
  text.replace(/[#*_~`[\]()]/g, " ");
