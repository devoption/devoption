# Release metadata and validation

[`release-metadata.toml`](../release-metadata.toml) records the reviewed support,
compatibility, migration and recovery statements for one exact proposed preview
version under [#260](https://github.com/devoption/devoption/issues/260). It is a
candidate-specific snapshot of the documented implementation, not a roadmap,
installer configuration, upgrade mechanism or publication approval.

## Required record

The validator rejects unknown/missing keys and unsupported schema versions.

| Section | Required fields and current policy |
| --- | --- |
| Root | Integer `schema_version = 1`, exact numbered preview `version`, `channel = "preview"`, and the single supported platform identifier in `platforms` |
| `compatibility` | `installation = "fresh-only"`, empty `upgrade_from`, and nonempty `data` and `configuration` statements |
| `migration` | Boolean `required = false` and nonempty `instructions` explaining the current migration limitation |
| `recovery` | Boolean `binary_rollback = false` and nonempty `instructions` explaining recovery limits |
| `support` | `status = "development-preview"` and 1–10 distinct, nonempty `warnings` |

Statements must be trimmed, single-line text with no control characters, at most
2,000 characters each. The UTF-8 TOML file must be a regular file of at most
32 KiB. Numeric values cannot substitute for boolean fields.

The current schema policy deliberately rejects upgrade source versions, required
migrations, binary rollback support, additional platform profiles and stable
support claims. Those capabilities need implementation/evidence and a reviewed
policy change; editing metadata cannot enable them. In particular, the bounded
[same-installation recovery procedure](same-node-recovery.md) does not establish
cross-version platform rollback. Human review must still assess whether prose
matches the actual change: structural validation cannot prove English claims.

## Prepare and verify

Before committing a release-affecting change, review the record for the next
version selected by the [preview policy](release-preparation.md). Update its
`version` and all affected statements explicitly. Do not automatically carry
forward a record for a different version. The initial record targets
`0.1.0-preview.1` while the real checkout remains at `0.1.0-dev`.

Validate the record without generating or publishing anything:

```sh
python3 scripts/release_metadata.py 0.1.0-preview.1
```

The committed source supplies the metadata. Preview preparation validates it
against the selected next version **before** invoking release-plz to change the
temporary workspace version. A missing, stale or inconsistent record blocks the
proposal and produces no successful output directory. A genuine no-release run
creates no candidate and does not require next-version metadata; removing the
metadata from a tagged baseline counts as a shipped change and therefore blocks
the resulting proposal. Raw `--channel analysis` remains a diagnostic and is
explicitly reported as not metadata-validated.

The proposal appends a marked compatibility/support block to the new dated
changelog section. It preserves the human change notes and prior release
sections. Its SHA-256 identifies the exact metadata file bytes, including
comments; it is not a signature, artifact checksum or build provenance. The report
records that digest, version and repository-relative path. If the record changes,
regenerate the notes rather than editing the generated block by hand.

After applying a proposal in its reviewed branch, the existing read-only command
checks version/date/change notes and requires exactly the current metadata block:

```sh
python3 scripts/release_notes.py 0.1.0-preview.1
```

Missing, modified, duplicated or stale blocks fail before notes are printed.
The **Release check** workflow uses this same command. Stable note syntax remains
recognized by the low-level extractor, but the current metadata policy rejects
stable candidates; stable support/promotion requires its separate reviewed gates.

Metadata validation does not satisfy [#21's release approval gate](releases.md),
prove artifact provenance or authorize tags, publishing, merges or installed-server
upgrades. Bundle inventory/provenance and upgrade/recovery implementation remain
in #261 and #263.
