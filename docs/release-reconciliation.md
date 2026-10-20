# Read-only candidate retention plan

The [#261](https://github.com/devoption/devoption/issues/261) planner compares a
[committed candidate reservation](release-candidates.md) with GitHub tag, release
and asset state. It proposes the missing steps for a validation draft. It has no
writer, upload, deletion, tag creation, signing or publication operation. Its
separate opted-in writer is described below. Actions retention still expires;
planning alone does not preserve remote bytes.

## Inspect a reserved candidate

Use a trusted checkout containing the merged reservation, its exact original
Actions ZIP and the pinned GitHub CLI. The planner audits the committed catalog
and re-verifies the ZIP, packet receipt, signed proof and complete platform archive
before reading remote state. It does not extract or execute the platform binary.

```sh
python3 scripts/reconcile_candidate_release.py \
  --version 0.1.0-preview.1 \
  --transport /absolute/path/to/exact-actions-artifact.zip \
  --gh /absolute/path/to/trusted/gh
```

The API reader uses the caller's existing authentication and explicit GET requests
to `github.com` for `devoption/devoption` only. It requires reported push access
because an incomplete inventory that hides drafts must not look like an absent
release. No new credential or permission is requested. Permission/API failures
stop planning. Release and asset lists are paginated with duplicate-ID checks and
a bounded complete scan; reaching the bound fails instead of assuming absence.

The proposed retention layout uses `vVERSION` at the reservation's original source
commit, a draft prerelease named `DevOption VERSION validation candidate`, and one
asset named `devoption-VERSION-ubuntu-24.04-amd64.candidate.zip`. That asset is the
unchanged original Actions ZIP, including the platform tarball, signed proof and
transport receipt. Its planned digest is the reserved transport digest, not the
inner archive digest. There is no rebuild, repackaging or silent embedded-version
change. A deterministic draft body binds the reservation, source and digests and
states `publication_eligible: false`. This layout is a reviewed planning contract;
creating even its draft and tag requires separate authorization.

## Retry observations

- Missing tag/release/asset: list only the absent operations. A matching tag or
  validation draft can be reused in a future authorized writer.
- Existing tag: resolve the actual ref, including bounded annotated-tag chains,
  to the reserved commit. A release's `target_commitish` alone is not tag proof.
  The exact tag is distinguished from longer tags sharing its prefix.
- Existing release: require the exact validation title/body, source SHA target,
  draft/prerelease flags, unpublished status and mutable state. Published,
  immutable, edited or duplicate releases require manual review.
- Existing asset: require exactly the expected name, size, SHA-256, content type
  and `uploaded` state. Extra, duplicate, unfinished or conflicting assets stop
  planning; no delete, clobber, retag or replacement is proposed.
- Matching metadata: return `download_and_verify_existing_zip`, never an
  already-complete claim. The output explicitly leaves `remote_bytes_verified`
  false. A future writer must download and cryptographically reverify those bytes
  before accepting an interrupted upload as complete.

The [GitHub release API](https://docs.github.com/en/rest/releases/releases) and
[asset API](https://docs.github.com/en/rest/releases/assets) supply the draft,
immutability, upload-state and digest metadata used by these checks. Metadata
agreement does not replace archive/proof verification.

Output schema 1 includes the expected identity, observed release/asset IDs and
pending operations. It always reports `writes_performed: false`,
`requires_separate_approval_for_writes: true` and `publication_eligible: false`.
The JSON is an observation for human review, not an executable transaction or
authorization. Remote state can change between requests and after the plan;
future write logic must serialize work, re-read state and verify returned IDs
and bytes. Do not execute operations from a saved stale plan.

The reserved packets have real signing/readback evidence and installer/base-upgrade
acceptance; see [delivery status](releases.md#delivery-status-against-the-six-issues).
The planner's fixtures cover interrupted states, metadata conflicts, draft visibility,
annotated tags and pagination failures. The [#21 release gate](releases.md) still
governs publication.

## Approved retention writer

**Retain reserved candidate** now connects the entire path: required source CI,
original artifact download, cryptographic verification, exact tag, unpublished
validation draft, unchanged ZIP upload, and independent downloaded-byte verification.
It does not rebuild, sign, publish the draft or change publication eligibility.
Draft assets have no Actions seven-day expiry, but remain deletable by authorized
repository users. This is long-lived retention, not a guarantee against deletion.

Before dispatch, obtain explicit owner approval for the tag, draft and asset write.
Select the reviewed full **current main workflow commit**, reserved version and
SHA-256 of its committed reservation JSON. The artifact source commit inside that
reservation can be older; it is never substituted with current main. The `retain`
input defaults false. Without it, only read-only source CI runs. No live write is
authorized by merging the implementation.

The main-only writer job uses `contents: write` and `actions: read` with the
short-lived workflow token; it needs no signing, attestation, PR or settings rights.
Checkout credentials remain unpersisted. Context and clean-checkout checks bind
the writer to the exact reviewed main dispatch and passing source CI. The reviewed
reservation digest must match before constructing any client or write request.
Unlike the local read-only planner, this writer does not require the REST
repository response's user-oriented `permissions.push` field: GitHub App
installation tokens can omit it. Its draft visibility follows from the reviewed
job's `contents: write` grant and the enforced workflow context (see
[GitHub's release access rules](https://docs.github.com/en/rest/releases/releases#list-releases)). It still checks
the returned repository identity and fails on API errors; the local planner
continues to require reported push access.

Each invocation re-reads tag/draft/asset state instead of executing a saved plan.
It verifies the original Actions artifact ID/name/digest/run/source, or downloads
an already matching retained asset when the original artifact has expired. No
remote objects are created until the ZIP, proof and complete archive pass checks.
The only write operations create the reserved ref, validation draft and absent ZIP
asset. Upload uses the fixed GitHub upload host without redirects; no delete,
clobber, force-update or publish endpoint exists. Every returned object ID is
checked against fresh state. Final readback re-verifies signatures and packet
bytes and confirms the same tag, draft and asset identities before reporting success.
Downloads select the documented media type for each API: the
[Actions archive redirect](https://docs.github.com/en/rest/actions/artifacts#download-an-artifact)
uses `application/vnd.github+json`, while
[release asset bytes](https://docs.github.com/en/rest/releases/assets#get-a-release-asset)
use `application/octet-stream`. Failures report a sanitized HTTP status without
printing captured stderr, tokens or signed download URLs.

Lost responses after tag, draft or asset creation are recovered by the next
identical run. A matching complete retry performs no writes and verifies retained
bytes even after Actions expiry. Wrong tags, modified/published drafts, extra or
unfinished assets, missing original bytes and failed verification stop without
repairing or replacing remote state. Administrators can still change state after
the final read; the workflow is not a sandbox against repository owners.

## Live retention evidence

End-to-end API-model fixtures cover full completion, identical retry, lost responses
at all three write boundaries, original-artifact expiry, corrupt readback and
conflicts. These fixtures make no remote writes.

The preview.1 [retention run](https://github.com/devoption/devoption/actions/runs/39291171277)
passed upload and cryptographic readback on attempt 5, then verified the same bytes
without writes on attempt 6. Its historical tag/draft required operator setup;
that run alone did not prove workflow-token creation of those objects.

Earlier attempts exposed two real adapter bugs: the
[user-oriented permissions check](https://github.com/devoption/devoption/actions/runs/39252471232)
rejected the workflow token, and the
[Actions download request](https://github.com/devoption/devoption/actions/runs/39254096971)
used the release-asset media type (HTTP 415). Both stopped before writes and were
corrected. They are historical failures, not outstanding acceptance gates.

The owner-approved preview.2 [retention run](https://github.com/devoption/devoption/actions/runs/39374411243)
used workflow commit `ebe9ae3c9599f4d3cc7ed4a8fefc03a9f62a3f4d` and reservation
SHA-256 `4deb180355e25e6bbe5072d4ba70c203dbe75dfd051e1852a6ff3cf993d65a94`.
All exact-main checks passed. Attempt 1 created the correct tag and validation
draft, then stopped because the immediate inventory did not return that draft.
Attempt 2 reused those objects, uploaded the unchanged original ZIP and passed
independent cryptographic readback. No operator-seeded object was needed for
this candidate; the existing interrupted-run recovery path completed it. Attempt 3
passed an identical retry using the same retained release/asset. A separate local
download also passed the ZIP digest, signed proof and complete archive verifier.

- Tag: `v0.1.0-preview.2`, source `cdc2df021bc518b5314ccf6e47f7a3cc89bf7c8f`.
- Unpublished draft ID: `418039335`; asset ID: `658376378`.
- ZIP: `devoption-0.1.0-preview.2-ubuntu-24.04-amd64.candidate.zip`, 1,926,009 bytes.
- Transport SHA-256: `22e2659de42eeba2d00f1dbaf33742fa0b9fe7a4b0bd18e02aa92e94adc1e74a`.

The [committed reservation](../release-candidates/0.1.0-preview.2.json) still binds
the original archive/proof/source and signing run. The release remains a draft
prerelease with no publication timestamp; the packet remains
`publication_eligible: false`. Tag/draft creation, upload and readback are now live
workflow evidence, not publication or proof of fully unattended operation.
