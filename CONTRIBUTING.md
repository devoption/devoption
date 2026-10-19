# Contributing

Keep changes small enough to understand and verify. Be respectful, explain the
problem clearly, and avoid posting private data.

1. Open or find a repository issue **before starting any work**, including docs,
   maintenance, dependencies, and releases. Agree on the smallest useful scope.
2. Create a topic branch (for example `git switch -c docs/12-setup`) or work in a
   fork. Open a **draft pull request** against `main` when ready for feedback.
3. Make small, atomic [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
   Use types such as `feat`, `fix`, `docs`, `test`, `ci`, or `chore`. Every commit
   needs an issue reference on its own line, for example:

   ```text
   docs: clarify setup prerequisites

   Refs #12
   ```

4. Use a Conventional Commit PR title. Put `Refs #12` or `Closes #12` on its own
   line in the PR body. CI checks that referenced numbers are repository issues,
   not PRs, and checks non-merge commit messages. A reference must name an issue
   in this repository. Reviewers confirm the work actually matches that issue.
5. Update `CHANGELOG.md` under `Unreleased` in every PR, including maintenance
   and docs changes. Explain what changed and link the issue. Keep setup and
   usage documentation consistent with the code. A release preparation PR may
   move those entries to a dated version section instead.
6. Run the build, tests, format, lint, Python and Node checks in
   [the development guide](docs/development.md#checks). Describe
   results and remaining limitations in the PR. Review the diff for credentials,
   private paths/data, unsupported claims, and unrelated changes.
7. Request review once the scope is complete. Preserve the issue reference in
   the final merge/squash message. Never force-push shared history.

The only direct push to `main` is the initial repository bootstrap under
[#1](https://github.com/devoption/devoption/issues/1). All later work uses topic
branches and PRs, including fixes to bootstrap CI. Do not push directly to `main`.

Every commit that changes visible UI must refresh the affected page screenshots
in `docs/screenshots/` in that same commit. Shared layout or theme changes require
all main-page captures. Reuse the existing filenames; non-UI commits keep
the existing images. Add a capture and [gallery](docs/screenshots/gallery.md) entry
when adding a main page.
Follow the [capture instructions](docs/screenshots/README.md).

Keep the root README a concise product introduction, current status, minimal quick
start and documentation links, with at most two representative screenshots. Put
operational detail, technical references and new page captures in `docs/`. Describe
DevOption as a deployable software factory intended for individuals and teams;
keep current owner-only and loopback limits distinct from that direction.

Dependency updates follow this same issue-first process. Keep `Cargo.lock`
committed, use locked builds, and verify action updates against official upstream
release tags before pinning their full commit SHA. No registry publication is
configured; the crate has `publish = false`.

Ordinary GitHub PR CI uses read-only permissions and no stored checkout
credentials. It runs PR code only on GitHub-hosted runners with no repository
secrets. Do not add
`pull_request_target` execution of contributor code, self-hosted runners for
untrusted PRs, or privileged publication jobs to ordinary PR checks.

The separate [release preparation writer](docs/release-prs.md) is an opt-in,
main-only manual job with limited branch/PR write permissions after read-only
checks pass. It never runs on contributor PR events. Default draft generation
grants no merge/publication authority; optional bot-only merging requires the
separate approved policy and preserves the new PR's required checks.

The separate manual [bundle provenance workflow](docs/bundle-provenance.md) has
an explicit signing opt-in after source CI. Its main-only signing job has OIDC
and attestation permissions; ordinary PR checks remain read-only. Adding this
workflow does not authorize its activation, issue credentials, or publish a
product release. Live signing/attestation writes need separate owner approval.

The separate manual [reserved-candidate retention writer](docs/release-reconciliation.md#approved-retention-writer)
can create a reserved tag and unpublished validation draft, upload the unchanged
verified ZIP and recover interrupted retries. It requires explicit tag/draft/upload
approval and passing exact-main CI; implementation alone does not activate it.

`main` requires PRs and passing CI, including for administrators, with no required
approval count. See [the maintainer checklist](docs/maintainers.md) for the verified
settings and remaining owner decisions about Actions permissions.
