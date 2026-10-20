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
| Root | Integer `schema_version = 1` or `2`, exact numbered preview `version`, `channel = "preview"`, and the single supported platform identifier in `platforms` |
| `compatibility` | Schema 1: `installation = "fresh-only"`, empty `upgrade_from`; schema 2: the fresh-only or reviewed upgrade policy below. Both require nonempty `data` and `configuration` statements. |
| `migration` | Boolean `required = false` and nonempty `instructions` explaining the current migration limitation |
| `recovery` | Boolean `binary_rollback = false` and nonempty `instructions` explaining recovery limits |
| `support` | `status = "development-preview"` and 1–10 distinct, nonempty `warnings` |

Statements must be trimmed, single-line text with no control characters, at most
2,000 characters each. The UTF-8 TOML file must be a regular file of at most
32 KiB. Numeric values cannot substitute for boolean fields.

Schema 1 retains the original fresh-only policy and bytes for historical packets.
Schema 2 adds integer `data_schema = 1` and `configuration_schema = 1` to
`compatibility`. It permits either fresh-only with no source versions, or
`installation = "fresh-and-upgrade"` with one explicitly reviewed earlier preview
of the same base version in `upgrade_from`. The [operator upgrade path](platform-upgrades.md)
implements that unchanged-schema base-installation profile with full offline
backup/restore. It rejects source/custom/app-capable installations and migrations.
Changing a prose statement does not bypass these executable checks.

Required migrations, binary-only rollback, additional platform profiles and
stable support claims remain rejected. In particular, the bounded
[same-installation recovery procedure](same-node-recovery.md) does not establish
cross-version platform rollback. Human review must still assess whether prose
matches the actual change: structural validation cannot prove English claims.

## Prepare and verify

Before committing a release-affecting change, review all affected statements.
The record may describe the current workspace version or the next preview chosen
by the [preview policy](release-preparation.md). For current-version policy,
preparation changes only the top-level `version` inside its disposable clone and
includes that binding in the generated PR. Other statements and comments retain
their exact bytes. An unrelated old/future version is rejected. This removes
manual version bookkeeping without inventing compatibility or migration claims.
Schema-2 upgrade declarations must be reviewed against the proposed version:
preparation never silently rewrites `upgrade_from`. Supply the reviewed next-version
record when its supported predecessor changes. The retained preview's schema-1
metadata and historical release notes are not rewritten by upgrade implementation.

Validate the record without generating or publishing anything:

```sh
python3 scripts/release_metadata.py 0.1.0-preview.2
```

The committed source supplies the metadata. Preview preparation validates it
against the selected next version after the permitted version-only binding and
**before** invoking release-plz to change the temporary workspace version.
Unsupported statements, missing metadata or unrelated stale versions block the
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
python3 scripts/release_notes.py 0.1.0-preview.2
```

Missing, modified, duplicated or stale blocks fail before notes are printed.
The **Release check** workflow uses this same command. Stable note syntax remains
recognized by the low-level extractor, but the current metadata policy rejects
stable candidates; stable support/promotion requires its separate reviewed gates.

Metadata validation does not satisfy [#21's release approval gate](releases.md),
prove artifact provenance or authorize tags, publishing, merges or installed-server
upgrades. Bundle inventory/provenance and upgrade/recovery implementation remain
in #261 and #263.
