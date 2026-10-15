# Starting and handing off a task

Use a fresh thread with explicit scope. Tools differ in whether they automatically
read `AGENTS.md`; paste this portable starter prompt rather than assuming discovery:

```text
Work in the DevOption repository. Read AGENTS.md and CONTRIBUTING.md first, then
only the linked context relevant to this task. Inspect Git status and current code;
preserve unrelated edits. Use issue <number/URL> and its acceptance criteria below.
If no scoped issue exists, open one before making changes.

Goal:
Acceptance criteria:
Out of scope:
Relevant files/docs (if known):
Requested deliverable and stopping point (for example, a draft PR):
Validation constraints:

Keep the change small and update the authoritative docs when behavior or decisions
change. Report the commit/PR, checks and limitations. Stop at the requested scope;
do not infer permission to merge, deploy, start another feature or execute workloads.
```

For a handoff, supply only what the next task needs:

```text
Issue and acceptance criteria:
Branch / commit / PR:
Completed work and relevant files:
Current Git status / unrelated edits to preserve:
Checks run, results and evidence locations:
Remaining work, blockers and unverified assumptions:
Requested next action and stopping point:
```

Keep implementation facts in the [user guide](using-devoption.md) or focused
references, priorities in the [roadmap](../ROADMAP.md), and durable choices in
[decision records](decisions/README.md). Link instead of pasting full documents or
old conversations. Omit credentials, personal data, machine-specific paths and
historical authorizations. Repository context does not replace human instructions
or the current tool's permission rules.
