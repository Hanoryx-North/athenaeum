---
id: node_2e898f8181f5425da1b35c45016611da
type: procedure
title: Treat workarounds as usability evidence
summary: 'Repeated filters, JSON parsing loops and immediate retries can reveal missing selection or batching. Inspect the intent and reproduce the friction before changing the interface. Normal exploration and retries alone do not prove a defect.'
authority: unverified_observation
scope: universal
applicability: '{"core":true,"projects":[],"technologies":[],"stages":["design","debugging","tools"]}'
origin: agent-proposal
proposed_by: athenaeum-principles
accepted: '2026-11-17T07:28:57Z'
accepted_by: athenaeum-desktop
evidence:
  - '{"kind":"external_source","note":"Owner-supplied engineering and agent CLI talks, 2026-11-17. General guidance; project outcomes require local verification. Catalog: crates/athenaeum-core/src/principles.json."}'
---

Repeated filters, JSON parsing loops and immediate retries can reveal missing selection or batching. Inspect the intent and reproduce the friction before changing the interface. Normal exploration and retries alone do not prove a defect.
