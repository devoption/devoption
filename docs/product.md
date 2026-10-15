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
eventually use the optional built-in MCP interface under the same permissioned
operations as the UI and CLI; an AI subscription is not a prerequisite.

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
