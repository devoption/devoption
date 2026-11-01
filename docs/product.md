# Product context

DevOption is an open-source, server-deployable software factory for individuals
and teams. Its goal is one usable interface connecting source, reviews, pipelines,
packages, deployment and observability, so users can take software from a reviewed
change to an observable running application.

## Users and goals

- Individuals who want to develop and operate their own applications without
  assembling a separate interface for every step.
- Teams that need shared project context, review and operational visibility as
  collaboration capabilities become available.
- Operators who want an opinionated, simple single-server starting point and a
  path to broader operations later.

Human-only workflows must remain complete. Compatible external AI clients can
use the optional [authenticated read-only MCP interface](mcp.md) under the same permissioned
operations as the UI and CLI; an AI subscription is not a prerequisite.

DevOption is not an agent host or a replacement for an agent harness. The intended
integration exposes an MCP server, platform tools, and skills to a user's chosen
external harness. The harness owns the agent loop, model, memory, and scheduling;
DevOption owns its permissioned operations, approval boundaries, and execution
evidence. Overview reports platform health, work, outcomes, and required decisions,
not the internal state of externally hosted agents. Authenticated read-only MCP
transport and observation tools are implemented; skills and permissioned writes
remain product direction.

## Non-goals and boundaries

- A local-only workspace is not the product boundary; current preview restrictions
  are documented in the [user guide](using-devoption.md).
- DevOption is not a universal replacement for Linux containers or every
  third-party development/operations tool. Integrate useful tools rather than
  rebuilding them all immediately.
- The business rollout of devoption.io is separate from the product roadmap.
- Direction is not evidence of production readiness, compliance certification or
  a release commitment.

Use the [roadmap](../ROADMAP.md) for priorities and sequence, the
[architecture](architecture.md) for implementation boundaries, and
[accepted decisions](decisions/README.md) for durable constraints. Verify current
behavior in code and the linked implementation guides before describing it as
available.
