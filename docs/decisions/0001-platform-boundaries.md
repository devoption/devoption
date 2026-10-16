# 0001: Rust domains and hosting direction

Status: Accepted direction; planned capabilities remain subject to implementation.

## Decision

Keep one Rust monorepo with cohesive domain crates and reusable modular interfaces.
CLI and server delivery code should stay thin as domain behavior grows. Add a
crate when real implementation needs a boundary, not to reserve roadmap names.
The [architecture](../architecture.md) describes today's boundaries, including
authentication and storage still owned by the server crate.

WebAssembly-first application hosting is the selected direction. Existing native
Rust fixture/build contracts are groundwork, not an implemented Wasm service or
a deployed runtime. [Decision 0004](0004-first-wasm-web-app.md) selects the first
runtime/ABI contract, subject to its review status and validation plan.
Start with an opinionated single server; expand to
multiple servers after that path is usable.

Registry direction includes hosted private/public packages and controlled proxy
services, beyond the current dependency contracts. See the [roadmap](../../ROADMAP.md)
for sequencing and the [open questions](README.md#open-questions) for unresolved choices.

## Consequences

Evolve and reuse domain contracts across delivery surfaces. Do not treat a UI route,
CLI command, dependency descriptor or fixture as permission to execute a workload.
Validate target and host-interface changes before claiming support or isolation.
