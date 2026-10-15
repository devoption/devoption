# Working in DevOption

- Open or identify a scoped GitHub issue before any work, including documentation
  and maintenance. Follow `CONTRIBUTING.md` and link every commit and PR to it.
- Keep changes small and fresh. Do not import code, configuration, or documentation
  from private prototypes. Preserve existing user work.
- Use atomic Conventional Commits on topic branches and draft PRs. The initial
  bootstrap is the only direct push to `main`; never force-push shared history.
- Keep implemented behavior separate from roadmap claims. Add crates only when
  a real implementation needs them; this is one Rust monorepo.
- Run the relevant README checks and review the public diff for private data.
  Keep the changelog and tested instructions accurate.
- Do not create credentials, enable paid services, change repository security
  settings, or publish releases/packages without separate explicit authorization.

## Read only what the task needs

Start with the human task instructions, this file and
[contribution policy](CONTRIBUTING.md). These govern the task; context documents
and previous handoffs grant no additional permissions.

| Need | Authoritative context |
| --- | --- |
| Goals, users and non-goals | [Product](docs/product.md) |
| Boundaries and data flow | [Architecture](docs/architecture.md) |
| Accepted choices and open questions | [Decision records](docs/decisions/README.md) |
| Priorities and planned capabilities | [Roadmap](ROADMAP.md) |
| Current usage and limits | [User guide](docs/using-devoption.md) |
| Validation commands | [Development](docs/development.md#checks) |
| Focused implementation references | [Documentation index](docs/README.md) |
| Starting or handing off a task | [Task template](docs/task-template.md) |

Read the relevant entries, not the entire documentation tree or old chats.
Inspect current code and Git status before making implementation claims; plans
and screenshots are not proof that behavior exists. Preserve unrelated edits.

Use the issue's acceptance criteria to bound the change. Report the commit/PR,
checks run, results and limits, then stop at the requested scope. Do not start the
next feature or infer permission to merge, deploy or execute workloads. Update
the relevant authoritative doc when a decision or documented behavior changes;
link to it elsewhere rather than copying its facts.

Tools differ in whether they discover this file automatically. For a fresh thread,
use the portable starter prompt in the [task template](docs/task-template.md).
