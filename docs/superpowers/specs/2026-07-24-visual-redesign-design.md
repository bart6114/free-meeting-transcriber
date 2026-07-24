# Visual Redesign — "Field Recorder"

Date: 2026-07-24
Branch: `refactor/field-recorder-redesign`
Status: approved for autonomous implementation (user delegated direction + implementation)

## Brief

Free Meeting Transcriber (fork of fastrepl/anarlog) should stop looking like its
upstream. Upstream look: stock shadcn stone-neutrals, `system-ui` type, 0.5rem
radii, pill buttons, soft minimal chrome. Goal: a distinct, typography-led
identity that fits what the app *is* — a local-first instrument that records
meetings and turns them into readable notes — with the fork's tongue-in-cheek
"free as in beer" streak.

## Direction: "Field Recorder"

The app's world is recording (waveforms, timecode, REC dots, tape labels) and
then *reading* (transcripts, notes). The identity splits along that line:

- **Chrome = instrument.** Compact grotesque UI type, monospace timecode/labels,
  hairline borders, tight radii, flat surfaces.
- **Content = page.** Notes and transcripts set in a real reading serif with
  generous line-height. The note area should feel like paper inside a machine.

### Type system (the core of the redesign)

| Role | Face | Usage |
|---|---|---|
| Display + UI | **Bricolage Grotesque** (variable, opsz) | Session titles, headings, buttons, nav, settings. Characterful at display sizes via optical sizing; quiet at 13px UI sizes. |
| Reading | **Literata** (variable, opsz) | Editor/note body + note headings, enhanced summaries, transcript body text. |
| Data | **IBM Plex Mono** | Timestamps, timecode, speaker labels, kbd, eyebrow/section labels (uppercase, letterspaced). |

- Bundled locally via `@fontsource-variable/bricolage-grotesque`,
  `@fontsource-variable/literata`, `@fontsource/ibm-plex-mono` (400/500/600),
  imported in `apps/desktop/src/main.tsx`. No CSP exists, but bundling keeps the
  app offline-first.
- Tokens in `apps/desktop/src/styles/globals.css` `@theme`:
  - `--font-sans`: `"Bricolage Grotesque Variable", system-ui, sans-serif`
  - `--font-serif` (new): `"Literata Variable", Georgia, serif`
  - `--font-mono` (override): `"IBM Plex Mono", ui-monospace, monospace`
  - `--font-hand`: **retired**; its two call sites move to Bricolage display styling.
- Delete dead `apps/desktop/public/fonts/SF-Pro-Text-*.otf`.
- `title-input.tsx` canvas font-measurement strings must reference the new face.

### Palette

Warm-ink instrument palette, light + dark, expressed as the existing HSL-triplet
tokens (retheme values, not plumbing). Named anchors:

| Name | Hex (approx) | Role |
|---|---|---|
| paper | `#F7F6F2` | light `--background` |
| ink | `#211D19` | light `--foreground` |
| charcoal | `#191613` | dark `--background` |
| chalk | `#F2EFE9` | dark `--foreground` |
| signal red | `#C43D34` | recording states, REC dot, destructive. The one loud color. |
| beer gold | `#D99A26` | brand accent: focus rings, active/selected glints, active waveform. Sparingly. |

- Neutrals re-derived from a warm hue ramp (hue ~35–40, low sat) — close in
  temperament to upstream's stone but deeper, with slightly lower-contrast
  hairline borders and higher-contrast text.
- New semantic tokens: `--recording` (signal red) and `--brand` (beer gold) +
  foregrounds, so sweeps don't hardcode.
- Chart colors re-picked to fit (warm-first ramp).

### Shape & depth

- `--radius: 0.25rem` (from 0.5rem).
- Buttons: `rounded-full` → `rounded-md` (pill → soft rectangle; a visible break
  from upstream).
- Shadows reserved for floating layers (popover, dialog, floating bar); cards
  flat with hairline borders.
- Focus rings: beer gold.

### Signature element

**The timecode strip**: wherever a timestamp, speaker label, or section eyebrow
appears, it is uppercase, letterspaced IBM Plex Mono (`font-mono text-[11px]
uppercase tracking-widest` idiom), with active recording rendered as
`● REC 00:14:32` in signal red. One device, applied consistently.

## Implementation map

All windows share one CSS graph (`index.html` → `main.tsx`), so global changes
reach main, detached-note, floating-bar, and live-caption windows.

1. **Foundation** (sequential, everything else depends on it)
   - `apps/desktop/package.json`: add fontsource deps; import in `main.tsx`.
   - `packages/ui/src/styles/globals.css:184-257`: new `:root`/`.dark` HSL
     values, `--radius`, new `--recording`/`--brand` tokens + `@theme` color
     mappings.
   - `apps/desktop/src/styles/globals.css`: font tokens, `@theme` additions,
     retire `--font-hand`, retheme hardcoded search-highlight yellows.
   - `apps/desktop/src/styles/dark-theme.css`: scrollbar + selection colors.
2. **Editor package** (parallel after foundation)
   - `packages/editor/src/styles/prosemirror/*.css` (~52 hex literals, 13
     files): replace hex with `hsl(var(--…))` tokens; body/headings → Literata;
     adjust heading scale for serif; `dark.css` retheme; caret/link/placeholder
     colors from tokens.
3. **UI primitives** (parallel)
   - De-slate `switch, select, carousel, resizable, dialog` + `neutral-*` leaks;
     button/card radius + shadow changes per Shape & depth.
4. **Desktop app sweep** (parallel, by directory)
   - ~78 hardcoded color occurrences across `session/`, `main/`, `sidebar/`,
     `settings/`, `templates/`, `shared/`, `changelog/`, `contacts/`,
     `onboarding/` → semantic tokens (`blue-*` accents → brand/recording/muted
     as appropriate).
   - Timecode-strip treatment for transcript timestamps + speaker labels and
     recording indicators (`sidebar/timeline/realtime.tsx`, floating bar,
     transcript renderer).
   - `--font-hand` call sites (`settings/page-title.tsx`,
     `onboarding/index.tsx`) → Bricolage display treatment.
   - `title-input.tsx` canvas measurement font strings.
5. **Verify**
   - `pnpm -F desktop typecheck`, `pnpm -r typecheck`, `pnpm -F desktop test`
     (compare against pre-change baseline), `pnpm exec dprint fmt`.
   - Manual visual pass: main window light+dark, editor, transcript, settings,
     onboarding, floating bar.

## What stays

- Semantic token architecture, light/dark support, screen layout/IA, existing
  animation vocabulary (shimmer, wiggle, dancing sticks), traffic-light window
  chrome.

## Alternatives considered

1. **Editorial notebook** (serif display, cream, terracotta) — rejected: the
   generic "AI redesign" look; ignores the recorder half of the app.
2. **Broadcast Swiss** (tight neo-grotesk, sharp, cool grays) — rejected: reads
   as a Linear clone; distinct from upstream but not from the genre.
