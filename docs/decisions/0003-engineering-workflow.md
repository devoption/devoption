# 0003: Focused engineering and validation

Status: Accepted workflow guidance; human task instructions and repository policy govern.

## Decision

Use scoped issues, reviewable PRs, atomic Conventional Commits and the validation
required by [contribution policy](../../CONTRIBUTING.md). Let acceptance criteria
define completion; report results and stop there. A handoff is context, not
authorization for another action.

Match review effort to risk. Reserve Copilot review for pre-release reviews rather
than every PR. Use independent security review when warranted, especially for
changes affecting authentication, storage or execution boundaries; it is not an
automatic requirement for every prose edit. Preserve authentication, isolation
and data safety while prioritizing deeper hardening by risk; deferral does not
relax existing guards or release gates.

Use only disposable repositories for local testing, never real personal checkouts.
Choose deliberate gitignored locations for runtime/test artifacts and bounded
logs; avoid scattered large files. Use portable relative paths in documentation,
and verify ignore coverage before recording artifacts. Never commit credentials,
private data, chat transcripts or internal assistant notes.

## Consequences

Fresh threads read only task-relevant context, inspect current code and preserve
unrelated work. Use the [task/handoff template](../task-template.md) to record scope,
evidence and limits without turning past permissions into standing authority.
