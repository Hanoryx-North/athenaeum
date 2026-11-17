---
id: node_e8276c2725d343289c6bfb4994562b60
type: decision
title: Default desktop wikis to the platform user data folder
summary: 'New and Library-cloned desktop wikis default to the user''s local data directory under Athenaeum/Wikis, outside source checkouts; users can choose another parent.'
authority: unverified_observation
scope: project
origin: agent-proposal
proposed_by: codex
accepted: '2026-11-17T14:06:21Z'
accepted_by: user-automation
---

The owner chose this as the default for all wikis to keep knowledge data separate from code repositories. Resolve the platform directory through Tauri localDataDir rather than hard-coding a home path or using Program Files. Existing wikis retain their saved locations; this change is not a migration. Sources: src/commands/fs.ts (defaultWikiDirectory and optional createProject parent), src/components/project/create-project-dialog.tsx, src/components/project/clone-wiki-dialog.tsx, src/hooks/use-desktop-control.ts, docs/wiki-cli.md. Verification: 29 focused tests passed; npm run build and npm run check:boundaries passed. Live desktop resolved ~/Library/Application Support/Athenaeum/Wikis; native creation/open and default fs.createProject duplicate-folder rejection verified with a temporary wiki, then removed. Windows/Linux paths were tested with platform data-directory fixtures, not on those operating systems.
