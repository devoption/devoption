# Authenticated bundle provenance

This implements the opt-in signing workflow and verifier for
[#261](https://github.com/devoption/devoption/issues/261). The first owner-approved
live signing run passed on 2026-10-18; see the [acceptance evidence](#live-acceptance-evidence).
Live retained-candidate upload and independent readback also passed on
2026-10-19 UTC. This does not introduce an installer, supported upgrades, product
publication or a recommended channel.
The [candidate manifest](release-bundles.md) retains `publication_eligible: false`.

## Authority and activation

Repository policy in [AGENTS.md](../AGENTS.md) requires separate approval before
creating credentials or publishing releases/packages. Obtain explicit owner
approval for the OIDC signing request, GitHub attestation writes and candidate
artifact retention before a
live opted-in run. Merging this code is not that approval. No stored secret,
PAT, signing key, repository setting or paid service is created by this workflow.

After merge, the manual **Bundle provenance** workflow accepts a reviewed full
main `source_commit` and a `sign` checkbox, default **false**. Without the checkbox,
only ordinary read-only source CI runs. Do not select it without that approval.
With the checkbox, its separate main-only job receives `contents: read`,
`id-token: write` and `attestations: write`. It validates the exact repository,
dispatch/main workflow context, source SHA, clean checkout and passing source CI.
Branch/PR dispatches cannot enter that job. Context checks are accidental-invocation
guards, not a sandbox against an operator forging local environment variables.

The signed candidate is built once in that job from fetched locked inputs using
the pinned Rust toolchain and offline release build. Release installation tests,
systemd-unit checks, exact executable version/help checks and complete bundle
comparison run before signing. Source context is rechecked immediately before
the attestation action. No caller-provided binary, artifact download or unsigned
predicate is promoted into the signer. A run may sign the exact reviewed dispatch
revision even if main later advances; consumers must independently pin that SHA.

The action uses GitHub's short-lived OIDC identity and workflow token. For this
public repository, signed proof data may enter GitHub/Sigstore transparency
infrastructure; attestation records are written to GitHub. This is the externally
visible side effect that requires approval. The verified archive and proof are
also uploaded for seven-day test retention as described below. No registry is
pushed, release is created, version tag is changed or branch is merged. The proof
is not a substitute for the separate [#21 release gate](releases.md).

`actions/attest` is pinned to upstream v4.2.2 commit
`1e69f48acb82d1966a394da916b4c1698aa569d6`. It generates its own build-provenance
predicate for the archive produced in that same job. It does not sign the earlier
self-reported assembly predicate as proof of compilation. The downloaded GitHub
CLI 2.102.0 Linux amd64 archive is pinned to SHA-256
`bb766f710eef8ede859c18578c72c327597cd4c8a85b06001b1f3843c6019386`.
Both pins were checked against official upstream release/tag metadata on
2026-10-18. See [GitHub's attestation documentation](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations).

The workflow uses the existing `platform-release-preparation` concurrency group
without cancelling an in-flight operation. Release publication/retry
reconciliation and promotion remain later work. [Reviewed candidate reservations](release-candidates.md)
pin one retained identity per preview version in Git. The candidate-retention path
below has fixture coverage and successful live upload/readback evidence.
The [installer acceptance report](bundle-installer-acceptance.md) records real
installation of these retained signed bytes and the remaining public TLS gate.

## Short-lived candidate retention

After signing, `scripts/bundle_candidate.py stage` verifies private snapshots of
the archive and proof with the existing cryptographic policy before creating a
new packet directory. It checks the source and assembly checksum sidecars and
refuses existing output, including partial output left by a failed attempt.
The packet contains only the unchanged `.tar.gz`, `attestation.json` and
`candidate.json`. The archive already contains its manifest, SBOM and notes;
the adjacent unsigned assembly statement is not substituted for signed proof.

Receipt schema 1 records the source commit, version, archive name/SHA-256, proof
SHA-256, run ID/attempt, artifact name and `publication_eligible: false`.
It is a transport receipt, not a signed release declaration or independent trust
root. Its values must match the identity supplied independently to the checker.

The workflow uploads this packet with checksum-pinned `actions/upload-artifact`
v7.0.2, commit `cf430e030ddbb5b0abf93d22962f4752f3646cd9`, with seven-day retention
and `overwrite: false`. Its name is `bundle-SOURCE_COMMIT-RUN_ID-RUN_ATTEMPT`.
A collision fails instead of deleting or replacing earlier bytes. Rerunning the
signing job creates a distinct attempt; it does not reserve a published version
or prove that separately compiled candidates have identical bytes. Never select
a candidate by a floating name, newest run or embedded version alone.

A separate fresh job with only `contents: read` downloads the exact artifact ID
returned by that successful upload using pinned `actions/download-artifact`
v8.0.2, commit `9000827ccba6bdab643e8b6fd33ac0654aef8333`. These pins were checked
against official upstream release tags on 2026-10-18. The download action fails
on a transport digest mismatch. The checker then requires exactly the three
packet files, compares receipt and both file digests with the signer job's
outputs, and repeats cryptographic and complete archive verification on private
snapshots. It does not compile, extract the platform tarball, execute its binary
or grant signing/attestation permissions. Downloading the outer Actions ZIP is
only transport unpacking. See the upstream [upload](https://github.com/actions/upload-artifact/tree/v7.0.2)
and [download](https://github.com/actions/download-artifact/tree/v8.0.2) contracts.

Only after successful readback does the run summary record the reviewed source,
version, archive/proof digests, original signing attempt and artifact ID/URL.
The separately labelled transport digest hashes the Actions ZIP, not the
platform `.tar.gz`. A failed readback may leave an uploaded artifact; its
existence alone is not acceptance. Retrying only the failed readback job reuses
the original signing attempt and artifact ID, without signing or rebuilding.
Expired, deleted or missing artifacts fail; no fallback or replacement is made.

Actions artifacts require GitHub authentication to download and can expire or be
deleted. This is bounded test retention, not permanent release storage. No
repository retention setting is changed. [Cross-run candidate selection](release-candidates.md)
now reserves identities in Git; remote publication reconciliation, promotion and
channel/installer support remain pending. Each live signing/upload test requires owner approval;
completed acceptance runs do not grant continuing authorization.

## Live acceptance evidence

### Initial signing acceptance

The owner-approved [manual run 39176739455](https://github.com/devoption/devoption/actions/runs/39176739455)
completed successfully on 2026-10-18 at 23:37 UTC with signing explicitly enabled:

- Reviewed source and signer commit: `0de23b6da57aa26ad1b294d37019720cbe3c15b1`.
- Platform version: `0.1.0-preview.1`; target: Ubuntu 24.04 amd64.
- Archive: `devoption-0.1.0-preview.1-ubuntu-24.04-amd64.tar.gz`.
- Archive SHA-256: `4238b375bda5f25030e425c01fc25e9a725c7b4558a70e20498138ba0be029ec`.
- [GitHub attestation record 57213132](https://github.com/devoption/devoption/attestations/57213132).

All reusable source CI checks passed. The signing job then passed its locked
offline release build, installation fixtures, systemd checks, executable smoke
checks and complete bundle comparison. GitHub OIDC signing and the attestation
write succeeded. Immediate verification using the pinned CLI and policy below
accepted the certificate identity, source/signer commit and exact archive, and
reported `authenticated: true` with `publication_eligible: false`.

This records one successful live acceptance run, not continuing authorization
for additional signing runs. The default signing input remains false. The
GitHub attestation record is retained, but the archive and local proof bundle
were not uploaded as downloadable artifacts. Post-run verification of a retained
archive was outside that initial run; the separate retention acceptance below
now covers it. Installation from a distributed bundle, promotion and release
publication remain pending.
No product release, version tag or registry publication was created.

### Retained-candidate acceptance

The owner-approved [manual run 39182230807](https://github.com/devoption/devoption/actions/runs/39182230807)
completed successfully on 2026-10-19 at 00:53 UTC, with signing and seven-day
candidate retention enabled for one run:

- Reviewed source/signer commit: `a11c0619d98c14f43b41e3947e7f4f00a71dc2f0`.
- Platform version: `0.1.0-preview.1`; target: Ubuntu 24.04 amd64.
- Archive: `devoption-0.1.0-preview.1-ubuntu-24.04-amd64.tar.gz`.
- Archive SHA-256: `0ee462ffb9048fbc08108693a5f17167516a5d4c676a71659f1601470aefd0a3`.
- Proof SHA-256: `117ef3546eb26ff914159a99b1f8171709b11f90192ab86992105d2913eafb18`.
- [GitHub attestation record 57224295](https://github.com/devoption/devoption/attestations/57224295).
- [Actions artifact 12157370334](https://github.com/devoption/devoption/actions/runs/39182230807/artifacts/12157370334),
  name `bundle-a11c0619d98c14f43b41e3947e7f4f00a71dc2f0-39182230807-1`.
- Actions ZIP SHA-256: `c3c8ecd8e6d986397243b9be70a6c658eec3bcb5e3992fff20a435e4c3d688c3`.
- Original signing attempt: `1`; service-reported expiry: `2026-10-26T00:53:30Z`.

All source CI, locked offline release build, installation fixtures, executable
checks, complete bundle comparison, OIDC signing, attestation write and private
snapshot verification passed. Upload retained exactly the archive, proof and
receipt. The separate read-only job downloaded the exact artifact ID and passed
transport-digest, receipt, archive/proof digest and cryptographic verification.
The run summary recorded the verified identity with publication eligibility false.

After completion, the same artifact ID was downloaded locally. Its outer ZIP
digest and exact three-file inventory were checked before writing those three
transport files to a new directory. The trusted merged-source checker with pinned
GitHub CLI 2.102.0 then passed against the source, version, archive/proof digests,
run ID and original attempt taken from the successful signer logs. Neither the
platform tarball nor its executable was extracted or executed during readback.

This is one positive retention/readback acceptance, not permanent distribution
or reproducible-compilation proof. Collision/tamper/missing-file/attempt checks
retain fixture coverage; live overwrite, expiry/deletion and failed-job retry
experiments were not performed. The artifact can expire or be deleted; the
recorded identity remains useful evidence after its download URL stops working.
No release, version tag, registry publication or repository setting was changed.

## Verify a retained candidate

Obtain the exact artifact ID and the independently reviewed source, version,
archive/proof digests, run ID and original signing attempt from the successful
run summary. Download that ID from its Actions artifact URL before expiry; do
not run code supplied inside the download. From a separately trusted checkout:

```sh
python3 scripts/bundle_candidate.py check \
  --directory /absolute/path/to/downloaded-candidate \
  --source REVIEWED_40_HEX_COMMIT --version 0.1.0-preview.1 \
  --sha256 REVIEWED_64_HEX_ARCHIVE_SHA256 \
  --proof-sha256 REVIEWED_64_HEX_PROOF_SHA256 \
  --run-id REVIEWED_RUN_ID --attempt ORIGINAL_SIGNING_ATTEMPT \
  --gh /absolute/path/to/trusted/gh
```

Do not derive these expected values from `candidate.json` in the download. The
receipt is checked against them, not used to define them. For direct archive and
proof verification without the transport receipt, use the command below.

Use trusted, checksum-verified GitHub CLI 2.102.0 and this reviewed verifier.
Independently obtain the owner-reviewed commit, version and archive SHA-256;
do not take these trust decisions from a downloaded manifest. Supply the Sigstore
bundle emitted by the approved signer or downloaded through GitHub's attestation
API/CLI. Never substitute `provenance.unsigned.json` or arbitrary verification JSON.

```sh
python3 scripts/verify_bundle_provenance.py \
  --archive /absolute/path/to/devoption-0.1.0-preview.1-ubuntu-24.04-amd64.tar.gz \
  --bundle /absolute/path/to/github-attestation.jsonl \
  --source REVIEWED_40_HEX_COMMIT --sha256 REVIEWED_64_HEX_ARCHIVE_SHA256 \
  --version 0.1.0-preview.1 --gh /absolute/path/to/trusted/gh
```

The verifier snapshots bounded regular inputs into a private temporary directory
and checks the explicit digest before invoking `gh attestation verify`. It does
not inherit caller tokens, GitHub host overrides or CLI configuration. `gh`
validates signatures, certificate chains and signed transparency/timestamp data
with its normal trusted roots; no caller-defined trust root or skip-verification
option is exposed. The pinned policy additionally requires:

- Repository `devoption/devoption` and the exact
  `.github/workflows/bundle-provenance.yml@refs/heads/main` certificate identity.
- GitHub Actions' OIDC issuer and GitHub-hosted runners.
- Both source and signer workflow digests equal to the reviewed full commit,
  source ref `refs/heads/main`, and SLSA v1 build-provenance predicate type.
- A verified subject matching the reviewed archive name and SHA-256.

Only after successful cryptographic verification does the tool inspect the archive.
It never extracts or executes it. It rejects unsafe, duplicate, unexpected or
missing members, unnormalized modes/ownership/timestamps, inconsistent file hashes,
incorrect source/version/target, and mismatched fresh-only release metadata.
Success reports authenticated identity/integrity while keeping publication
eligibility false. It does not establish application correctness, complete SBOM
coverage, vulnerability safety or upgrade compatibility, and makes no SLSA level
claim. Source-controlled build steps still execute trusted dependencies; isolation
from compromised build dependencies is not established by this workflow.

Policy tests simulate verifier output only to exercise argument and payload
checks. Ordinary read-only Ubuntu PR CI separately installs the pinned CLI and
requires it to reject a forged proof. Those tests alone do not establish positive
live OIDC issuance, attestation write permissions or signer certificate acceptance;
the approved live run above supplies that acceptance evidence for its exact commit.
