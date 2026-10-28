---
name: Athenaeum Desktop
description: "A shared Roboto, slate, blue and green workspace with dedicated account screens."
colors:
  bg: "#f8fafc"
  rail: "#eef2f7"
  surface: "#ffffff"
  inset: "#f0f4f8"
  hover: "#e7edf5"
  line: "#d8e0ea"
  control-line: "#7b8ba1"
  text: "#202c3e"
  muted: "#52627a"
  link: "#1466a8"
  selection: "#e0eefb"
  primary: "#14783e"
  primary-hover: "#0f6332"
  primary-ink: "#ffffff"
  danger: "#b02d3f"
  danger-soft: "#ffedf0"
  ok: "#14783e"
  ok-soft: "#e7f5ec"
  focus: "#2473c5"
  dark-bg: "#111722"
  dark-rail: "#141d2b"
  dark-surface: "#182230"
  dark-inset: "#0f1620"
  dark-hover: "#243246"
  dark-line: "#2e3d50"
  dark-control-line: "#64758c"
  dark-text: "#edf2f8"
  dark-muted: "#a4b2c5"
  dark-link: "#8dc7f6"
  dark-selection: "#223d58"
  dark-primary: "#25864b"
  dark-primary-hover: "#217e45"
  dark-danger: "#ff9da8"
  dark-danger-soft: "#39232c"
  dark-ok: "#84d6a1"
  dark-ok-soft: "#183427"
  dark-focus: "#81bdff"
typography:
  headline:
    fontFamily: "Roboto, 'Segoe UI', Arial, sans-serif"
    fontSize: "30px"
    fontWeight: 500
    lineHeight: 1.25
    letterSpacing: "-0.025em"
  auth-story:
    fontFamily: "Roboto, 'Segoe UI', Arial, sans-serif"
    fontSize: "clamp(32px, 3.2vw, 44px)"
    fontWeight: 500
    lineHeight: 1.15
    letterSpacing: "-0.025em"
  body:
    fontFamily: "Roboto, 'Segoe UI', Arial, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.55
  label:
    fontFamily: "Roboto, 'Segoe UI', Arial, sans-serif"
    fontSize: "14px"
    fontWeight: 500
    lineHeight: 1.4
  detail:
    fontFamily: "Roboto, 'Segoe UI', Arial, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  auth-description:
    fontFamily: "Roboto, 'Segoe UI', Arial, sans-serif"
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.7
  welcome:
    fontFamily: "Roboto, 'Segoe UI', Arial, sans-serif"
    fontSize: "clamp(32px, 3.6vw, 48px)"
    fontWeight: 500
    lineHeight: 1.2
    letterSpacing: "-0.025em"
rounded:
  panel: "12px"
  control: "7px"
  avatar: "50%"
  base-ui-control: "8px"
  base-ui-container: "10px"
spacing:
  xs: "4px"
  sm: "8px"
  control-gap: "12px"
  md: "16px"
  lg: "24px"
  section: "32px"
  page: "40px"
  auth: "64px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-ink}"
    typography: "{typography.label}"
    rounded: "{rounded.base-ui-control}"
    padding: "0 16px"
    height: "40px"
  button-secondary:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.text}"
    rounded: "{rounded.base-ui-control}"
    padding: "0 16px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.text}"
    rounded: "{rounded.base-ui-control}"
  input:
    backgroundColor: "transparent"
    textColor: "{colors.text}"
    rounded: "{rounded.base-ui-control}"
    padding: "4px 10px"
    height: "40px"
  navigation:
    backgroundColor: "{colors.rail}"
    textColor: "{colors.muted}"
    width: "200px"
  account-identity:
    textColor: "{colors.text}"
    typography: "{typography.label}"
  page-heading:
    textColor: "{colors.text}"
    typography: "{typography.headline}"
  auth-frame:
    textColor: "{colors.text}"
    padding: "64px 40px"
    width: "1080px"
  recent-projects:
    textColor: "{colors.text}"
    padding: "20px 0"
---

# Design System: Athenaeum Desktop

## Overview

**Creative North Star: "One recognizable knowledge workspace"**

Athenaeum Desktop belongs to one visual family with the Library, operator console and desktop application. Cool slate canvases, quiet separators and self-hosted Roboto give navigation, content and account identity a consistent voice. Blue identifies navigation and selection; green identifies productive actions.

The shared signature is the book mark, initial avatar, measured page heading and full-page authentication composition. Panels remain calm and readable in light and dark themes. This is the approved modernization of the existing Library baseline, documented from the implemented system on 2026-10-28.

The canonical MIT implementation is [`athenaeum_library/design-system/`](../athenaeum_library/design-system/), especially `tokens.css`, `components.tsx`, `sync.mjs` and `README.md`. Each app imports an exact vendored `src/design-system/` copy and builds independently. Change the canonical source, then run `node design-system/sync.mjs` from the Library checkout. This document records the implemented system; the frontmatter owns primitives, and `.impeccable/design.json` extends it with renderable specimens and non-token-schema metadata.

**Key Characteristics:**

- Self-hosted Roboto with real 400, 500 and 700 weights.
- Cool slate surfaces, blue navigation and green productive actions.
- Quiet borders, compact controls and clear account identity.
- Dedicated authentication and account screens with responsive layouts.

## Colors

The palette combines cool slate neutrals with clear blue and green signals. Unprefixed frontmatter colors are the shared light values; `dark-` entries are their dark-theme replacements. CSS resolves them through `--ath-*` roles using system preference or an explicit light/dark choice. White primary ink is shared by both themes.

### Primary

- **Knowledge blue** (`link`, `selection`, `focus`): links, selected navigation, selection surfaces and keyboard focus.

### Secondary

- **Productive green** (`primary`, `primary-hover`): create, save, search and sign-in actions. **Success green** (`ok`, `ok-soft`) communicates successful state with text or icons.

### Tertiary

- **Danger rose** (`danger`, `danger-soft`): errors and destructive action context. Preserve explicit labels and confirmations.

### Neutral

- **Slate canvas** (`bg`), **workspace rail** (`rail`) and **content surface** (`surface`) establish large regions.
- **Inset and hover layers** (`inset`, `hover`) organize fields, table headers and pointer feedback.
- **Fine line and control line** (`line`, `control-line`) distinguish separators from interactive boundaries.
- **Content ink and supporting ink** (`text`, `muted`) separate primary content from descriptions and metadata.

**The Role Rule.** Blue carries navigation, focus and selection; green carries productive actions. Status colors retain their established meanings.

## Typography

**Display and Body Font:** self-hosted Roboto with Segoe UI, Arial and sans-serif fallbacks. The canonical token stylesheet loads normal weights (400), (500) and (700) with `font-display: swap`; font files and the Roboto license travel with each vendored copy.

Page headings use the measured headline role; shared authentication adds a larger balanced story heading and comfortable supporting prose. Labels and account names use medium weight; metadata uses regular weight and smaller sizes. Identity detail truncates rather than forcing the header wider. Existing application content sizes remain appropriate to the density of each task; the shared identity and headings provide consistency.

**The Roboto Rule.** Use the shipped Roboto family and real static weights for application identity; reserve monospace for code and identifiers.

## Layout

The workspace has a (64px) header and (200px) rail. At (1100px), the rail becomes a (60px) icon strip with labels hidden while accessible names remain. The welcome header is (68px); its two-column introduction and recent-project list cap at (1160px), then stack at (760px). The work area continues to use the existing split panes, trees, editor and review controls. Account screens fill the app content area and retain an explicit return to the workspace. Shared authentication caps at (1080px), with equal context/form columns, a form width of (400px), (64px 40px) padding and a vertical divider. At (760px), it becomes one column with (36px 24px) padding, a horizontal divider and a shorter story; supporting fact rows are hidden. Page-heading actions wrap at this same breakpoint. Use the observed spacing scale rather than imposing identical density on every screen.

## Elevation & Depth

Hierarchy comes from tonal layering and fine borders. Shared floating surfaces may use the light and dark ath-shadow values recorded in the sidecar; resting panels remain flat.

**The Surface Rule.** Use canvas, surface and separators to establish hierarchy before adding a shadow.

## Shapes

Shared panels have softly squared corners (12px) and shared controls use compact corners (7px). Identity avatars are circular. Fine separators distinguish content regions; stronger control borders distinguish fields from the surrounding surface. Existing BaseUI controls retain their Tailwind radius mapping: a (10px) base yields (8px) medium controls. The shared header/account controls use the shared compact geometry. This is an intentional retained implementation detail, not a separate palette.

## Components

### Buttons

Existing BaseUI variants remain default, outline, secondary, ghost, destructive and link. Default controls are (40px) high and use (14px) medium text. Their primary hover uses the existing 90% primary-color treatment, and focus uses the existing ring/border mapping. Account inputs and submit actions are (46px) high.

### Inputs / Fields

BaseUI inputs retain their current (40px) default height and Tailwind focus, invalid and disabled states, mapped to the shared semantic colors. Full-page account forms enlarge fields to (46px).

### Navigation

Navigation uses labelled rows, muted resting ink and blue selected ink on the selection surface. Keep hover, selected and keyboard-focus states distinct. The desktop rail contracts to icons at its existing breakpoint; retain accessible names and tooltips.

### Content containers and status

Recent projects use a flat divided list with a name, muted path, open action and remove action. Hover emphasizes the project name; removal uses danger ink. Existing local panes, trees and review surfaces retain their behavior and BaseUI building blocks.

### Shared identity, page heading and authentication

`Brand` renders the inline outlined book mark, Athenaeum name and optional muted section label. `AccountIdentity` pairs a circular initial avatar (36px default) with a medium name and smaller muted detail; header and rail contexts may use the existing smaller avatar variants. `PageHeading` pairs a headline and description with wrapping actions above a fine divider. `AuthFrame` combines brand/context and a focused form in a full-page destination; account identity and return navigation stay reachable.

The four shared components contain presentation only. Existing local CLI/bridge operations own account behavior; BaseUI/Tailwind controls continue to consume the shared roles. No raster imagery is required: the mark and fact icons are inline SVG, and fonts include their license.

Control transitions remain short (160ms). Shared reduced-motion CSS suppresses movement when the system asks; loading, retryable failure and empty states remain explicit. Shared keyboard focus is a (2px) outline with (3px) offset; existing component focus rings retain their application behavior.

Documentation evidence: current source and `.impeccable/review/` captures. The final review resolved light control-boundary and dark primary-hover contrast findings. Browser evidence used synthetic API data and simulated Tauri; it does not establish native-webview or live-account behavior. No deployment or native app restart is implied.

## Do's and Don'ts

### Do:

- **Do** reuse Brand, AccountIdentity, AuthFrame and PageHeading from the vendored design system.
- **Do** use semantic tokens and verify both themes and keyboard focus.
- **Do** preserve explicit loading, error, empty, selected and disabled states.
- **Do** let account forms stack and keep long content inside its own scroll boundary.

### Don't:

- **Don't** edit generated src/design-system files directly; change the canonical MIT source and sync it.
- **Don't** replace Roboto or the blue/green role assignment with a separate application identity.
- **Don't** move sign-in or account management back into a cramped floating dialog.
- **Don't** imply backend permissions or local processing through presentation alone.
