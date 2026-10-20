# Preparing a release

There is no published product release. The prepared source version is
`0.1.0-preview.2`, with reviewed schema-2 metadata for the preview.1 base-bundle
upgrade path. Both packets are reserved in the [candidate catalog](release-candidates.md),
and [signed base-upgrade/reboot acceptance passed](bundle-upgrade-acceptance.md).
Both packets have long-lived unpublished validation drafts with
[verified retention/readback](release-reconciliation.md#live-retention-evidence).
Preparation and acceptance are not publication approval.

## Delivery status against the six issues

[#259](https://github.com/devoption/devoption/issues/259) is the umbrella, with
five implementation workstreams. They are not six equivalent release-workflow
tasks. The status below separates completed implementation from live activation
and work that has not been built.

| Issue | Implemented and demonstrated | Remaining before the overall release effort is complete |
| --- | --- | --- |
| [#260: Versioning and release PRs](https://github.com/devoption/devoption/issues/260) | Pinned release-plz; one workspace version; preview selection; metadata-bound notes; exact bot PR creation/retry; guarded optional squash auto-merge. [Live writer run](https://github.com/devoption/devoption/actions/runs/39156408201) generated [PR #270](https://github.com/devoption/devoption/pull/270), which merged; its [exact-main release check](https://github.com/devoption/devoption/actions/runs/39158246952) passed. Auto-merge has fixture coverage. | Implementation is complete with explicit activation gates: manual dispatch, checked-in auto-merge policy, repository setting and GitHub's bot-PR workflow approval. Auto-merge remains disabled; no setting or authority is changed by implementation. |
| [#261: Bundles and provenance](https://github.com/devoption/devoption/issues/261) | Complete local bundle inventory, manifests, SBOM, signing, cryptographic verification, seven-day transport and append-only candidate reservation. [Live signing, long-lived retention and independent readback](bundle-provenance.md#retained-candidate-acceptance) passed. | Publish only eligible complete candidates after #21. A validation draft is not a recommended release; installer acceptance is tracked separately below. |
| [#262: Bundle installer](https://github.com/devoption/devoption/issues/262) | Verified fresh-host bundle installer, immutable versioned files, dedicated accounts/units, pinned proxy, SSH enrollment and safe retries. [Real signed-packet installation, interruption/resume, identical retry, public DNS/HTTPS/ACME renewal and systemd restart/reboot passed](bundle-installer-acceptance.md). Temporary public infrastructure was removed and DNS restored. | Installer acceptance is complete for the retained Ubuntu/IPv4 validation packet. Publication eligibility and platform upgrade support remain separate gates. |
| [#263: Platform upgrades](https://github.com/devoption/devoption/issues/263) | [Operator-triggered same-schema base-preview upgrades](platform-upgrades.md), independent packet verification, locked offline backup, versioned activation, health checks, interrupted-state handling and explicit full-state recovery. [Actual signed preview.1-to-preview.2 upgrade/recovery and four kernel boots passed](bundle-upgrade-acceptance.md), including production CLI verification, owner/project/profile/Git persistence and corrupt-input refusal. Ordinary CI retains source-built fault/reboot gates. The owner Overview now displays root-written upgrade observations and SSH action/recovery guidance in builds containing the interface. | Coordinated hosted-app upgrades, migration recovery and channel/canary/stop gates remain pending. The notification flow requires a new versioned candidate for signed-packet acceptance. Preview.2 retention is verified; no automatic stable promotion exists. |
| [#264: DigitalOcean image](https://github.com/devoption/devoption/issues/264) | Existing public-node acceptance used a source build. | Reusable bundle-based image build, image hygiene, fresh-instance/upgrade tests, Marketplace review and a working launch destination are not implemented. |
| [#259: End-to-end distribution](https://github.com/devoption/devoption/issues/259) | Tracks the shared version/bundle contract and the five workstreams above. | Remains open until the child outcomes and published-candidate install/upgrade/recovery path are demonstrated. |

Completing version/PR automation and installer acceptance does not authorize
unattended merging or publication. The next delivery boundary is platform upgrade
and recovery support under #263, followed by image distribution under #264.
These are feature outcomes, not additional release-workflow planning slices.

A read-only [release preparation dry run](release-preparation.md) can propose
version/changelog changes with pinned release-plz in a disposable clone. It does
not replace the steps below or authorize publication. It selects numbered preview
proposals, preserves human notes and checks
[version-bound release metadata](release-metadata.md). Its opt-in
[draft PR writer](release-prs.md) retains the new commit's required CI and human
merge control. The [auto-merge policy](release-prs.md#optional-guarded-auto-merge)
is disabled until separately approved. Publication eligibility and execution
remain under #261/#263 and #21, not release-PR generation.

The **Release check** workflow is deliberately read-only. It repeats CI and
validates notes on `main`, showing them in the workflow summary. It does not
create a tag, GitHub release, binary distribution, or package registry upload.
The current metadata policy permits numbered preview candidates only. It rejects
the unreleased development version and stable candidates until a separate stable
policy and its gates are reviewed.

The separately opted-in [retention workflow](release-reconciliation.md#approved-retention-writer)
can create the reserved version tag and an **unpublished validation draft** with
the exact ZIP. It requires explicit tag/draft/upload approval and does not close
#21 or publish a release. All release-related workflows share serialization.

When a maintainer intentionally chooses to release:

1. Open a release issue first and make a topic branch/draft PR. Choose a stable
   numbered preview such as `0.1.0-preview.2`; publication is not automated here.
   Review and commit its support/migration metadata before generating the proposal;
   the generator binds current-version statements to the selected preview.
2. Apply the exact generated preview proposal to its reviewed topic branch. It
   updates the shared workspace/lockfile version without changing dependency
   inputs, binds metadata and generates the new dated notes. Keep `publish = false`;
   version preparation does not enable registry publishing.
3. Move the relevant `Unreleased` entries to a section named exactly
   `## [0.1.0-preview.2] - YYYY-MM-DD`, using the actual release date. Leave `Unreleased`
   for later changes. Notes need real change bullets and repository issue links;
   describe limitations and any breaking changes honestly. Prefer applying the
   generated proposal, which retains the human notes and adds the required
   metadata block, over manually constructing that block.
4. Run all [development checks](development.md#checks) and `python3 scripts/release_notes.py 0.1.0-preview.2`.
   The script rejects mismatched versions, missing/duplicate sections, invalid
   or future dates, notes without issue links and missing/stale metadata blocks.
   Review the output; automation
   cannot decide whether every statement is accurate.
5. Merge the reviewed PR after CI passes. Dispatch **Release check** from `main`
   with the prepared version. Verify the run's commit is the reviewed commit,
   both checks and notes jobs succeed, and the summary contains the right notes.
6. Only after an explicit decision to publish, tag that exact reviewed commit
   and push the new tag (never move an existing tag). For example:

   ```sh
   git tag -a v0.1.0-preview.2 <reviewed-commit-sha> -m 'DevOption 0.1.0-preview.2'
   git push origin refs/tags/v0.1.0-preview.2
   ```

7. From a checkout of that tag, extract the exact notes and use them to create a
   **draft** GitHub release, using existing maintainer authentication:

   ```sh
   python3 scripts/release_notes.py 0.1.0-preview.2 > /tmp/devoption-release-notes.md
   gh release create v0.1.0-preview.2 --repo devoption/devoption --verify-tag --draft --prerelease \
     --title 'DevOption 0.1.0-preview.2' --notes-file /tmp/devoption-release-notes.md
   ```

8. Review the draft, then deliberately publish it in GitHub. Record the release
   and validation links on the issue. Do not imply that a source-only release
   includes an installer or supported deployment.

No step above has been performed as a release during bootstrap. Actual release
publication, tag pushes, and future distribution mechanisms remain maintainer
actions, outside the automatic checks.
