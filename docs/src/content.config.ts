import { docsLoader } from "@astrojs/starlight/loaders";
import { docsSchema } from "@astrojs/starlight/schema";
import { glob } from "astro/loaders";
import { defineCollection, z } from "astro:content";

export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
  changelog: defineCollection({
    loader: glob({
      base: "../packages/changelog/content",
      pattern: "[0-9]*.md",
      generateId: ({ entry }) => entry.replace(/\.md$/, ""),
    }),
    schema: z.object({
      date: z.string(),
      summary: z.string(),
    }),
  }),
};
