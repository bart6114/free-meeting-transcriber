/**
 * The changelog entries in packages/changelog/content use a custom <banner>
 * tag rendered by the desktop app's Streamdown renderer. In CommonMark the
 * whole block becomes one raw-HTML node, so the markdown inside it would not
 * be processed. This plugin re-parses the inner text as markdown and wraps it
 * in a styled <aside>, keeping the title/variant attributes.
 */
export function remarkChangelogBanner() {
  const self = this;
  return (tree) => {
    tree.children = tree.children.flatMap((node) => {
      if (
        node.type !== "html" ||
        !node.value.trimStart().startsWith("<banner")
      ) {
        return [node];
      }
      const match = node.value.match(
        /^\s*<banner([^>]*)>\n?([\s\S]*?)<\/banner>\s*$/,
      );
      if (!match) return [node];
      const [, attrs, inner] = match;
      const title = attrs.match(/title="([^"]*)"/)?.[1];
      const variant = attrs.match(/variant="([^"]*)"/)?.[1] ?? "info";
      return [
        {
          type: "html",
          value:
            `<aside class="release-banner release-banner--${variant}">` +
            (title ? `<p class="release-banner-title">${title}</p>` : ""),
        },
        ...self.parse(inner).children,
        { type: "html", value: "</aside>" },
      ];
    });
  };
}
