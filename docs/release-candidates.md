# Reviewed candidate reservations

Under [#261](https://github.com/devoption/devoption/issues/261), the Git catalog in
[`release-candidates/`](../release-candidates/) reserves one exact signed candidate
per numbered preview version through a normal issue-linked draft PR. A reservation
pins its source, archive/proof digests and original run/attempt, plus the reviewed
Actions artifact ID and transport ZIP digest. Existing records cannot change.
Archives still expire under [seven-day test retention](bundle-provenance.md#short-lived-candidate-retention).
This catalog does not provide permanent binary storage or authorize publication.

The first reservation selects the already accepted `0.1.0-preview.1` candidate
from [run 39182230807](https://github.com/devoption/devoption/actions/runs/39182230807).
See its [exact record](../release-candidates/0.1.0-preview.1.json) and
[live acceptance evidence](bundle-provenance.md#retained-candidate-acceptance).
Earlier or later builds with the same embedded version are not automatically
selected. Once this record is merged, a different candidate needs a new version.

The [preview.2 record](../release-candidates/0.1.0-preview.2.json) selects the
separately approved [run 39369686068](https://github.com/devoption/devoption/actions/runs/39369686068)
at merged main commit `cdc2df021bc518b5314ccf6e47f7a3cc89bf7c8f`. Its signature,
independent Actions readback and local ZIP/proof verification passed; see
[provenance evidence](bundle-provenance.md#preview2-signing-and-independent-readback).
The same original ZIP now has [verified long-lived draft retention](release-reconciliation.md#live-retention-evidence),
including independent downloaded-byte verification. Preview.1's existing record
is unchanged.

## Reserve and retry

Independently review the source commit, version, archive/proof digests, original
run/attempt and exact artifact ID from the successful signing/readback logs.
Review that artifact's ZIP digest through GitHub's artifact metadata. Obtain the
ZIP for that exact artifact ID before it expires. Do not take these expected
values from the download. No additional signing is needed to reserve these bytes.

Prepare a separate canonical JSON record using the checked-in record as the
schema example. Schema 1 contains exactly `schema_version`, `candidate` and
`artifact`. `candidate` is the complete existing schema-1 transport receipt;
`artifact` contains only `id` and `transport_sha256`. Preserve the false publication
eligibility. This is a reviewed selection, not another attestation or release
manifest. Use a trusted checkout and the pinned verifier:

```sh
python3 scripts/release_candidates.py reserve \
  --record /absolute/path/to/reviewed-reservation.json \
  --transport /absolute/path/to/exact-actions-artifact.zip \
  --catalog release-candidates --gh /absolute/path/to/trusted/gh
```

The command checks the outer ZIP digest and exactly three bounded regular packet
members. It writes only those known transport filenames into private temporary
storage and repeats the existing receipt, archive/proof digest, cryptographic
identity and complete archive checks. It never extracts the platform tarball or
executes its binary. No URL, shell command or downloaded script is executed.

Only after verification does an atomic exclusive link create the canonical
`VERSION.json` record. A crash before linking leaves no reservation. Existing
identical bytes return `already_reserved` without replacing the file; conflicting,
malformed, partial or linked records fail. Concurrent identical attempts reconcile
to the same file. Every retry still requires the original verified transport;
missing, expired or changed bytes do not trigger a rebuild or replacement.
Inspect interrupted temporary files instead of treating them as reservations.

Commit the record, changelog and issue reference on a topic branch. Check the
committed catalog and compare it with the trusted full PR base commit:

```sh
python3 scripts/release_candidates.py audit
python3 scripts/release_candidates.py audit --base REVIEWED_40_HEX_BASE_COMMIT
```

The audit reads committed Git blobs, not unstaged or untracked files. It requires
canonical bounded records at their exact version filenames, regular nonexecutable
Git modes, an available source commit that is an ancestor of the audited HEAD,
and agreement with that source's embedded workspace version. Base comparison
rejects edits, removals and renames of any existing reservation, including
repointing its artifact locator. Ordinary CI audits structure/source identity;
the existing required contribution check enforces append-only comparison with
the PR base. These are review policy checks, not protection against an authorized
change to the validator itself or repository history.

## Limits and next gates

New records require human review of the independently obtained identities and
successful cryptographic verification. CI does not download artifacts, request
OIDC credentials or verify the proof again; expired artifacts must not make every
future PR depend on a permanently available download service. The catalog does
not itself prove artifact availability, authenticate arbitrary new record values
or alter the embedded `publication_eligible: false` manifest.

The initial real candidate was reverified locally and an identical retry returned
`already_reserved` without changing its record. Fixtures cover conflicts,
concurrent retries, interrupted records, tampering and Git audit rejection paths.
No new signing run, artifact upload, tag or GitHub release is created here.

The [read-only remote reconciliation planner](release-reconciliation.md) now
checks tag/draft/asset state and reports missing work without writes. Its separately
approved companion writer now implements long-lived draft retention and retries;
live retention evidence is recorded in that document. Signed-bundle installation
and base-upgrade acceptance are recorded in
[delivery status](releases.md#delivery-status-against-the-six-issues). Channel
promotion remains pending and must use the reserved archive digest without
recompiling or silently renaming an embedded preview version. Stable manifests remain unsupported. The separate
[#21 publication decision](releases.md) is still required before product publication;
validation-draft retention has its own explicit approval.
