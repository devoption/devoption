# Preparing a release

There are no releases yet. `0.1.0-dev` marks development source. The bootstrap
changelog is the record of current changes; it is not a release announcement.

A read-only [release preparation dry run](release-preparation.md) can propose
version/changelog changes with pinned release-plz in a disposable clone. It does
not replace the steps below or authorize publication. It selects numbered preview
proposals and preserves human notes; release PR creation, publication metadata and
publication remain unfinished under #260.

The **Release check** workflow is deliberately read-only. It repeats CI and
validates notes on `main`, showing them in the workflow summary. It does not
create a tag, GitHub release, binary distribution, or package registry upload.
It accepts stable or numbered preview notes and rejects the current unreleased
development version.

When a maintainer intentionally chooses to release:

1. Open a release issue first and make a topic branch/draft PR. Choose a stable
   version such as `0.1.0`; prerelease publication is not automated here.
2. Set `workspace.package.version` in `Cargo.toml`, run `cargo generate-lockfile
   --offline`, and commit the updated lockfile. Update README version/status
   wording as appropriate. Keep `publish = false` unless a separate reviewed
   issue explicitly introduces registry publishing.
3. Move the relevant `Unreleased` entries to a section named exactly
   `## [0.1.0] - YYYY-MM-DD`, using the actual release date. Leave `Unreleased`
   for later changes. Notes need real change bullets and repository issue links;
   describe limitations and any breaking changes honestly.
4. Run all [development checks](development.md#checks) and `python3 scripts/release_notes.py 0.1.0`.
   The script rejects mismatched versions, missing/duplicate sections, invalid
   or future dates, and notes without issue links. Review the output; automation
   cannot decide whether every statement is accurate.
5. Merge the reviewed PR after CI passes. Dispatch **Release check** from `main`
   with the prepared version. Verify the run's commit is the reviewed commit,
   both checks and notes jobs succeed, and the summary contains the right notes.
6. Only after an explicit decision to publish, tag that exact reviewed commit
   and push the new tag (never move an existing tag). For example:

   ```sh
   git tag -a v0.1.0 <reviewed-commit-sha> -m 'DevOption 0.1.0'
   git push origin refs/tags/v0.1.0
   ```

7. From a checkout of that tag, extract the exact notes and use them to create a
   **draft** GitHub release, using existing maintainer authentication:

   ```sh
   python3 scripts/release_notes.py 0.1.0 > /tmp/devoption-release-notes.md
   gh release create v0.1.0 --repo devoption/devoption --verify-tag --draft \
     --title 'DevOption 0.1.0' --notes-file /tmp/devoption-release-notes.md
   ```

8. Review the draft, then deliberately publish it in GitHub. Record the release
   and validation links on the issue. Do not imply that a source-only release
   includes an installer or supported deployment.

No step above has been performed as a release during bootstrap. Actual release
publication, tag pushes, and future distribution mechanisms remain maintainer
actions, outside the automatic checks.
