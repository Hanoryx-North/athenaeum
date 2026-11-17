---
id: node_5a5e9630bc024f32965a7814756bad09
type: procedure
title: Make agent tools return useful slices
summary: 'Offer limits, offsets, ranges, field selection and batch reads where repeated usage needs them. Keep defaults compact, state what was omitted and give the next request. Preserve relevant matches rather than relying on blind truncation.'
authority: unverified_observation
scope: universal
applicability: '{"core":true,"projects":[],"technologies":[],"stages":["design","tools"]}'
origin: agent-proposal
proposed_by: athenaeum-principles
accepted: '2026-11-17T07:28:59Z'
accepted_by: athenaeum-desktop
evidence:
  - '{"kind":"external_source","note":"Owner-supplied engineering and agent CLI talks, 2026-11-17. General guidance; project outcomes require local verification. Catalog: crates/athenaeum-core/src/principles.json."}'
---

Offer limits, offsets, ranges, field selection and batch reads where repeated usage needs them. Keep defaults compact, state what was omitted and give the next request. Preserve relevant matches rather than relying on blind truncation.
