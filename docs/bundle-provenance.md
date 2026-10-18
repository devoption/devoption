# Authenticated bundle provenance

This implements the opt-in signing workflow and verifier for
[#261](https://github.com/devoption/devoption/issues/261). It is prepared for
review; **positive live signing acceptance has not run**. It does not introduce
an installer, supported upgrades, product publication or a recommended channel.
The [candidate manifest](release-bundles.md) retains `publication_eligible: false`.

## Authority and activation

Repository policy in [AGENTS.md](../AGENTS.md) requires separate approval before
creating credentials or publishing releases/packages. Obtain explicit owner
approval for the OIDC signing request and GitHub attestation writes before a
live opted-in run. Merging this code is not that approval. No stored secret,
PAT, signing key, repository setting or paid service is created by this PR.

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
visible side effect that requires approval. No archive is uploaded, registry is
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
without cancelling an in-flight operation. Immutable artifact retention, release
publication/retry reconciliation and promotion remain later work. An opted-in
run immediately verifies its proof, but retains no downloadable archive/proof
artifact. Positive live acceptance and a reviewed candidate-retention path are
required before claiming this supplies an installable distribution.

## Verify a retained candidate

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
requires it to reject a forged proof. Those tests do not establish positive live
OIDC issuance, attestation write permissions or signer certificate acceptance.
