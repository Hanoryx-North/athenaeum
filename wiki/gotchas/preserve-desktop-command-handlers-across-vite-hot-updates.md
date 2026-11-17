---
id: node_e644d5b412244333b1856f622b02899c
type: gotcha
title: Preserve desktop command handlers across Vite hot updates
summary: 'The desktop CLI event listener can retain a replaced module''s dispatch closure during React Fast Refresh; keep its dispatcher and mounted action registry in import.meta.hot.data.'
authority: unverified_observation
scope: project
applicability: '{"core":false,"projects":["athenaeum"],"technologies":["Vite","React","Tauri"],"stages":["coding","debugging","verification"]}'
origin: agent-proposal
proposed_by: codex
accepted: '2026-11-17T13:05:00Z'
accepted_by: user-automation
---

Confirmed while enabling persistent live source sessions on 2026-11-17. Changing src/lib/graph-label.ts from 32 to 33 characters hot-updated React but wiki desktop status kept reporting 32 and mounted CLI actions were lost. src/lib/desktop-control.ts now preserves the registration map and a mutable dispatch holder in import.meta.hot.data, so existing native event listeners call the current module. Repeating the temporary edit reached the running desktop in 0.12 seconds with all 471 actions retained and target/debug/athenaeum-app's modification time unchanged; restored the source and confirmed status returned 32. npm start now reuses the same managed session as wiki start via scripts/live-desktop.mjs. Keep that session and the Cargo incremental cache for source work; Rust changes still require compilation.
