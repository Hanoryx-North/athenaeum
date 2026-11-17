# Athenaeum Desktop

The product owner requested restoration of the desktop from `ffbf092` on
2026-10-29, superseding the desktop appearance introduced by `abf1f7d`
(“visual redo”). This applies to this desktop application.

Mode: Operate; wiki content is Read. Preserve the compact knowledge workspace:
an icon sidebar, resizable file tree and research panes, a small project header,
and a centered project picker with recent projects. First use shows a single sign-in form. Successful sign-in advances automatically; project selection follows it. Coding-agent connections use direct actions, with external integrations and background memory under disclosure controls.

The implementation restored from `ffbf092` is the visual authority. Theme roles
live in `src/index.css`, native window colors in `src/lib/theme.ts`, and controls
in `src/components/ui/`. Do not sync the Library's blue/green, Roboto-based shell
back into this desktop without a new request.

Use Geist Variable, compact 32px default buttons and inputs, a 10px base radius,
quiet separators, and neutral gray surfaces. The dark canvas uses
`oklch(0.16 0.005 260)`, panels `oklch(0.205 0.005 260)`, and the sidebar
`oklch(0.18 0.005 260)`. Light mode remains supported; existing theme choices
are preserved. Primary actions in dark mode use light neutral fills.

Keep visible keyboard focus, disabled and loading states, the existing icon
labels/tooltips, and pane scrolling. Chat document typography and its 3px
blockquote rule are intentionally restored from the pinned commit; that rule
is document quotation styling, not a card accent.

The previous `.impeccable` sidecar and surface brief describe the superseded
visual redo and are historical metadata, not authority for this restoration.
