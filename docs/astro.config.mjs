import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import starlightLinksValidator from "starlight-links-validator";
import starlightLlmsTxt from "starlight-llms-txt";

import { remarkChangelogBanner } from "./src/remark-changelog-banner.mjs";

export default defineConfig({
  site: "https://loofah.io",
  markdown: {
    remarkPlugins: [remarkChangelogBanner],
  },
  integrations: [
    starlight({
      title: "Loofah",
      description:
        "A local-first knowledge vault for notes, meetings, transcripts, and agent-created research.",
      logo: {
        src: "../apps/desktop/src-tauri/icons/src/loofah-mark-1024.png",
      },
      favicon: "/favicon.ico",
      customCss: ["./src/styles/custom.css"],
      plugins: [
        starlightLinksValidator(),
        starlightLlmsTxt({
          promote: ["index*", "agents/**", "reference/**", "installation"],
        }),
      ],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/bart6114/loofah",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/bart6114/loofah/edit/main/docs/",
      },
      sidebar: [
        {
          label: "Getting started",
          items: [{ label: "Loofah", link: "/" }, "background", "quickstart"],
        },
        {
          label: "Using Loofah",
          items: [
            "meetings",
            "automatic-capture",
            "import-recordings",
            "notes",
            "syncing",
            "customize-summaries",
            "languages",
          ],
        },
        {
          label: "AI and privacy",
          items: ["ai-setup", "offline", "data-and-privacy"],
        },
        {
          label: "CLI and agents",
          items: [
            "installation",
            "agents/overview",
            "agents/vault",
            "agents/cli",
            "agents/mcp",
            "agents/skills",
          ],
        },
        {
          label: "Reference",
          items: [
            "reference/cli",
            "reference/mcp",
            "reference/errors",
            "artwork",
          ],
        },
        {
          label: "Help",
          items: ["help", "troubleshooting", "changelog"],
        },
      ],
    }),
  ],
});
