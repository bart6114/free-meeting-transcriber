import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import starlightLinksValidator from "starlight-links-validator";
import starlightLlmsTxt from "starlight-llms-txt";

import { remarkChangelogBanner } from "./src/remark-changelog-banner.mjs";

export default defineConfig({
  site: "https://freemeetingtranscriber.com",
  markdown: {
    remarkPlugins: [remarkChangelogBanner],
  },
  integrations: [
    starlight({
      title: "Free Meeting Transcriber",
      description:
        "A local-first knowledge vault for notes, meetings, transcripts, and agent-created research.",
      logo: {
        light: "./src/assets/logo-light.svg",
        dark: "./src/assets/logo-dark.svg",
        replacesTitle: true,
      },
      favicon: "/favicon.svg",
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
          href: "https://github.com/bart6114/free-meeting-transcriber",
        },
      ],
      editLink: {
        baseUrl:
          "https://github.com/bart6114/free-meeting-transcriber/edit/main/docs/",
      },
      sidebar: [
        {
          label: "Getting started",
          items: [
            { label: "Free Meeting Transcriber", link: "/" },
            "quickstart",
          ],
        },
        {
          label: "Using Free Meeting Transcriber",
          items: [
            "meetings",
            "automatic-capture",
            "import-recordings",
            "notes",
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
          items: ["reference/cli", "reference/mcp", "reference/errors"],
        },
        {
          label: "Help",
          items: ["help", "troubleshooting", "changelog"],
        },
      ],
    }),
  ],
});
