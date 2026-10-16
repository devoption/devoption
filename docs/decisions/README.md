# Decision records

These short records capture accepted choices and their consequences, not chat
history or permission grants. Change a record when its decision changes; mark it
superseded and link its replacement if preserving the distinction is useful.
Keep proposals visibly separate until accepted. Priorities belong in the
[roadmap](../../ROADMAP.md), not in this index.

## Accepted

- [0001: Rust domains and hosting direction](0001-platform-boundaries.md)
- [0002: Cohesive product interface](0002-product-interface.md)
- [0003: Focused engineering and validation](0003-engineering-workflow.md)

## First application contract

- [0004: First single-node Wasm web app](0004-first-wasm-web-app.md) selects the
  runtime, ABI, app subset and validation plan for #229; see its review status.
  Implementation remains pending.

## Open questions

- Broader application subsets and host capabilities beyond decision 0004.
- Concrete build isolation, resource enforcement and deployment topology beyond
  the documented contracts and bounded fixtures.
- Package registry/proxy protocols and rollout details; no hosted service is implied.

Resolve these through scoped issues and explicit decisions. Existing proposals,
fixture evidence and historical task authorizations do not settle them.
