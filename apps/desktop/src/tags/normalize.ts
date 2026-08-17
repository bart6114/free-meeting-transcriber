export const TAG_NAME_RE = /^[\p{L}_][\p{L}\p{N}_-]*$/u;

// Lowercase-dedupe, dropping anything outside the tag charset. The Rust side's
// `ensure_tag` only guarantees trim/strip-#/lowercase; the strict charset filter
// lives here, applied before any command call.
export function normalizeTagNames(tagNames: string[]): string[] {
  const result = new Map<string, string>();

  for (const rawTagName of tagNames) {
    const tagName = rawTagName.replace(/^#/, "").trim().toLowerCase();
    if (!TAG_NAME_RE.test(tagName)) {
      continue;
    }

    result.set(tagName, tagName);
  }

  return [...result.values()];
}
