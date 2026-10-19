# Read-only candidate retention plan

The [#261](https://github.com/devoption/devoption/issues/261) planner compares a
[committed candidate reservation](release-candidates.md) with GitHub tag, release
and asset state. It proposes the missing steps for a validation draft. It has no
writer, upload, deletion, tag creation, signing or publication operation. Existing
Actions retention still expires; this planner does not preserve remote bytes.

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

The reserved real ZIP was reverified locally and the first read-only inspection
produced the missing-tag/draft/asset plan. Fixtures cover interrupted states,
metadata conflicts, draft visibility, annotated tags and pagination failures.
Positive remote draft/asset reuse and publication have not run. Permanent byte
retention, the authorized writer and installation from the distributed bundle
remain pending. The [#21 release gate](releases.md) still governs publication.
