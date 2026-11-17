---
id: node_4b832bd8d04647e3a185dc84475ae152
type: procedure
title: Preserve failures through output reduction
summary: 'Keep the actual command exit status, timeout and truncation state. Preserve a useful failure tail and a full log when available. Do not infer success from the last filtering process in a pipeline; use pipeline failure propagation or run validation directly.'
authority: unverified_observation
scope: universal
applicability: '{"core":true,"projects":[],"technologies":[],"stages":["debugging","verification","tools"]}'
origin: agent-proposal
proposed_by: athenaeum-principles
accepted: '2026-11-17T07:28:59Z'
accepted_by: athenaeum-desktop
evidence:
  - '{"kind":"external_source","note":"Owner-supplied engineering and agent CLI talks, 2026-11-17. General guidance; project outcomes require local verification. Catalog: crates/athenaeum-core/src/principles.json."}'
---

Keep the actual command exit status, timeout and truncation state. Preserve a useful failure tail and a full log when available. Do not infer success from the last filtering process in a pipeline; use pipeline failure propagation or run validation directly.
