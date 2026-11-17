---
id: node_63e26d9248044975a4998b27af18d7c9
type: gotcha
title: Keep local Dock launches on the managed live desktop
summary: 'The owner''s local Athenaeum Dock launcher should use the managed live session; a Relay-installed snapshot can run alongside it and overwrite the desktop control descriptor.'
authority: unverified_observation
scope: project
applicability: '{"core":false,"projects":["athenaeum"],"technologies":["Tauri","macOS"],"stages":["tools","debugging"]}'
origin: agent-proposal
proposed_by: codex
accepted: '2026-11-17T13:36:50Z'
accepted_by: user-automation
---

Confirmed 2026-11-17: a Relay snapshot and target/debug/athenaeum-app were both running under com.hanoryx.athenaeum. The Dock pointed to the Relay snapshot and wiki desktop status reported live:false even though a live supervisor existed. src-tauri/src/desktop_control.rs::start writes the same app-data desktop-control.json for each instance. Removed the obsolete Relay application bundle and aliases at the owner's request. ~/Applications/Athenaeum.app now links to this checkout's target/debug bundle; its Contents/MacOS launcher runs wiki start before desktop show. This avoids the previous launcher short-circuiting to any available desktop. Verified repeated open plus wiki start reused the managed session, NSWorkspace reported one Athenaeum instance, and wiki desktop status reported live:true and frontendReady:true. Preserve source, user data, installed CLI/MCP support, and Cargo incremental caches when removing old desktop builds.
