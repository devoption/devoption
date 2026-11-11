# First-release evidence and owner decision

This is the acceptance map for [#21](https://github.com/devoption/devoption/issues/21),
reconciled on 2026-11-11 against main `d52c36d` and the linked reports. It records
existing evidence and remaining decisions; it is not a new security audit,
installer rehearsal or release approval. The [release procedure](releases.md)
owns publication steps; [candidate readiness](release-readiness.md) owns the
machine-checkable canary gates.

## Candidate boundary

The retained [preview.3 reservation](../release-candidates/0.1.0-preview.3.json)
identifies source `dbe5a5c890d03806fbe6bd4cbe0e02e34ca4caf0`, its exact archive,
proof and transport digests, and `publication_eligible: false`.
[Signing/readback](bundle-provenance.md#preview3-signing-and-independent-readback)
and [unpublished retention](release-reconciliation.md#preview3-retention) are
recorded for that packet. They do not cover subsequent main changes, even while
the workspace version still reads `0.1.0-preview.3`.

Choose the source revision before collecting release acceptance. Either assess
the unchanged retained packet using its own source, notes and support contract,
or prepare a new numbered candidate for later source. Changed bytes require a
new version and reservation; do not replace preview.3 or reuse its reports for a
different digest. Current [metadata](../release-metadata.toml) declares fresh-only
installation, no supported predecessor and no cross-version binary rollback.
Earlier base-preview upgrade evidence cannot extend that declaration.

## Acceptance map

| #21 requirement | Available evidence | What still blocks closure |
| --- | --- | --- |
| Clean supported host, two revisions, failed deployment, recovery and restart/restore | [Single-node reconciliation](single-node-acceptance-status.md) maps the source-built WASIp2 lifecycle and recovery reports. [Bundle installer acceptance](bundle-installer-acceptance.md) covers an earlier retained packet. | Complete the selected candidate's exact signed-packet installer/lifecycle acceptance; preview.3's report explicitly leaves it pending. Record host profile, source and packet identity, two app revisions, failure/recovery, persistent state and reboot/restore results. |
| Execution/request, filesystem, approval, process, environment/log, dependency/action and provenance review | [Execution trust contract](execution-trust.md), [authentication decisions](authentication-review.md), [deployment identity evidence](deployment-identity-validation.md) and provenance reports describe bounded controls and tradeoffs. | Review the selected release source across every listed boundary, record findings and dispositions, resolve blockers and obtain owner acceptance of residual nonblocking risks. Existing tests and #99's authentication decisions do not complete this review. |
| Private reporting route and repository settings decisions | Private reporting and the documented protection/permission settings were read back on 2026-11-11; see the [maintainer snapshot](maintainers.md#verified-repository-settings) and [security policy](../SECURITY.md). | Owner decision on the write-default token, PR-approval permission, unrestricted actions and external-contributor workflow approval policy. Recheck availability before release; an enabled private form is not proof that a report was submitted or triaged. |
| Version, date, notes, README, locked dependencies and exact-main Release check | Prepared metadata, historical notes and the retained candidate's [exact-source Release check](bundle-provenance.md#preview3-signing-and-independent-readback) exist. | Review the selected candidate's actual shipped contents and support claims. Later main changes need a new versioned proposal and exact-reviewed-main checks; an old successful run cannot validate them. |
| Concrete owner release decision | Tags and retained drafts identify validation packets. No published product release or continuing publication authority is recorded. | Present the complete evidence packet and obtain separate approval for the exact commit, version, artifact digests and publication actions. Record the resulting authorized publication before closing #21. |

## Next bounded work

1. Select the candidate source and supported installation profile. Preserve
   existing candidate bytes and their historical metadata/notes.
2. Complete the release-boundary review and record each blocking finding in its
   appropriate tracking channel; vulnerability details stay private. Record
   owner decisions on repository settings without treating this checklist as
   authority to change them.
3. Prepare any new candidate through the existing issue-linked PR process. Run
   the documented checks and read-only Release check on the exact reviewed main
   commit. Signing, retention and disposable-host acceptance need their own
   explicit authorization before those actions run.
4. Validate the selected authenticated packet on the supported disposable host,
   retain identity-bound lifecycle/recovery reports, and collect any required
   canary evidence under the existing policy. Source-built fixtures and simulated
   clocks cannot substitute for these observations.
5. Present the owner with candidate identity, compatibility limits, report/run
   links, security dispositions, residual risks and the precise proposed writes.
   Missing, failed or inconclusive evidence keeps the release blocked.

The operator supplies host evidence, the reviewer supplies review dispositions,
and the owner supplies settings/publication decisions. No person or deadline is
assigned by this reconciliation. [#263](https://github.com/devoption/devoption/issues/263)
still owns coordinated hosted-app upgrade/recovery and
[#264](https://github.com/devoption/devoption/issues/264) owns image distribution.
They are not implemented by this checklist; any narrower first-preview scope
must be explicitly reconciled with #21 and the provisional milestone before
claiming it complete. The promotion stop remains enabled and automatic
publication remains disabled in [policy](../release-promotion.toml).
