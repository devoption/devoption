# Draft release PR generation

The manual **Release preparation** workflow can generate a narrowly scoped,
bot-owned draft after source-revision CI and preview/metadata validation pass.
This is part of [#260](https://github.com/devoption/devoption/issues/260).
It creates no tags or releases and does not approve or merge PRs.

## Run from main

After this implementation is merged, dispatch the workflow on `main` with an
explicit `proposal_date` (`YYYY-MM-DD`). Leave `create_draft` unchecked for a
read-only proposal. Select it to authorize that run's separate draft writer.
Use the same date for a retry; a later date produces different changelog bytes
and is not an identical candidate. Review
[version-bound metadata](release-metadata.md) for the proposed version first.

The writer is restricted to this repository's main-only manual workflow and
requires successful checks and plan jobs. It regenerates the proposal from the
same source revision rather than accepting a caller-supplied patch or artifact.
It builds the expected Git tree locally, verifies the remote source and generated
tree, and creates one issue-linked commit changing only the root `Cargo.toml`,
`Cargo.lock` and `CHANGELOG.md`. All files retain mode `100644`. A no-release
result creates no branch or PR.

The immutable branch name is `codex/release-plz-VERSION`. PR titles and commit
messages are Conventional Commits and reference #260. The PR body identifies the
source, expected tree, proposal date and metadata digest, and distinguishes
source CI from the new commit's required checks. Existing contribution checks
remain applicable. All version-changing commits still require their own CI.

## Authentication and review

Ordinary PR CI and the proposal job remain read-only. Only the opted-in writer
job receives `contents: write`, `pull-requests: write` and `issues: read`, using
GitHub's automatically issued short-lived token. Checkout credentials remain
unpersisted. No PAT, GitHub App private key or new stored credential is used.
The writer calls only the fixed GitHub repository API, refuses redirects, and
never prints its authorization header or API error body. Its workflow-context
assertions guard accidental invocation; they are not a sandbox against a trusted
operator who deliberately forges environment variables.

The repository must allow Actions to create PRs. A read-only settings check on
2026-10-18 observed `can_approve_pull_request_reviews = true`; no setting was
changed. If GitHub rejects creation, stop and review that failure rather than
changing settings or substituting a more powerful token automatically.

GitHub's current [GITHUB_TOKEN behavior](https://docs.github.com/en/actions/concepts/security/github_token)
places `opened`/`synchronize`/`reopened` PR workflow runs created by this token in
an approval-required state. A maintainer must select **Approve workflows to run**
on the PR when prompted. The writer does not approve runs, manufacture statuses,
or substitute the source workflow's passing CI for the new commit's checks.
Other token-triggered events generally do not launch workflows; this workflow
uses only manual dispatch, so merging a release PR does not trigger another
preparation run. Keep required branch-protection checks and up-to-date rules.

Auto-merge remains disabled and unimplemented. Publication, stable promotion and
upgrades remain separately gated by [the release process](releases.md), #21 and
the remaining distribution tasks. Merging this implementation does not dispatch
the writer or approve a particular release candidate.

## Retry and interruption

- An identical bot-owned commit and open draft are reused without mutation.
- If a branch was created but PR creation failed or its response was lost, a
  retry verifies the commit's single parent, tree, message and bot author before
  creating or recovering the draft. A lost PR response is reconciled by listing
  the existing PR; no duplicate is created.
- A changed main commit, existing version tag, another open release candidate,
  closed/ready/edited/human-owned PR, or nonidentical branch stops the writer.
  It never overwrites, force-updates, deletes, reopens or merges them.
- Main and tag state are checked again before branch/PR creation. External
  administrators can still race these reads. Up-to-date protection and the new
  PR's required checks remain essential; the writer does not lock GitHub main.
- Interrupted runs can leave an unreferenced Git object or candidate branch.
  These are not a published release. Reconcile remote state before retrying;
  obsolete/closed or changed candidates require an explicit maintainer decision,
  not automatic cleanup or version reuse.

The existing `platform-release-preparation` concurrency group serializes workflow
runs without cancelling an in-flight writer. A future publication workflow must
share/reconcile this serialization and immutable identity contract before
activation. Partial *publication* recovery is not implemented by PR retry logic.

Local/API fixtures exercise these transitions without GitHub writes. The live
writer remains unexercised until a maintainer deliberately dispatches an opted-in
main run and reviews its result. Passing fixtures do not establish live token
permissions, PR workflow approval or publication readiness.
