# Typed build review context

[#194](https://github.com/devoption/devoption/issues/194) replaces the arbitrary
external digest accepted by [approval records](build-approvals.md) with immutable
`devoption_builds::review_context::ReviewContext` metadata. This is a non-executing
prerequisite of #17, not a verified review issuer or a production approval flow.

## Contents and validation

`ReviewContext::validate(&BuildPlan, Input)` retains:

- The explicit `InitialBuildV1` policy from the [execution trust contract](execution-trust.md).
  This identifies its fixed network, resource, output and retention requirements;
  changing those requirements needs a new policy version before this binding is reused.
- The pinned Rust version, absolute Cargo/rustc paths and claimed executable hashes,
  plus a digest of the complete toolchain manifest (compiler, linker and support files).
- Digests for the fresh catalog binding, exact source snapshot, dependency bundle
  and controlled configuration manifest.
- The unprivileged execution UID and seven distinct owned-directory observations:
  Work, Source, Home, CargoHome, Target, Temporary and Artifact. Each includes path,
  device, inode, owner UID and mode.

Validation requires the Rust version, Cargo path, Work path, source snapshot hash
and dependency bundle hash to match the supplied plan. The approval registry checks
that binding again on issuance; consumption compares the complete plan and complete
normalized context by value. A raw SHA-256 no longer satisfies either API. The
context's fingerprint is available for later display/provenance, not used as a
substitute for value equality in this process.

Under [#196](https://github.com/devoption/devoption/issues/196), context validation
also applies the InitialBuildV1 proposed-settings gate below. Issuance repeats that
gate against the actual queued plan, so a context created for a compliant plan
cannot be reused to approve different, out-of-policy environment/limit settings.

Paths must be absolute normalized Unix strings of at most 4096 UTF-8 bytes, without
empty/dot/parent components or controls; `/` is rejected. Cargo is bound to the
validated plan's executable; rustc must end in `/rustc`, and the two paths differ.
Each of the seven directory roles appears exactly once. Directory paths and
(device, inode) pairs must be distinct; inodes and owner/execution UIDs are nonzero.
Device zero is allowed because it can be a valid filesystem device number.
Directory mode is exactly `0700` or `0500`, with no special or group/other bits.
Different unprivileged supervisor and runner owners can be represented. These
checks do not decide which process can write a directory or enforce read-only mounts.

Input ordering is normalized by role. `details()` provides an immutable reference
to the complete typed input for later review. Caller edits to its original copy
cannot change the retained value. Types containing paths and digests deliberately
omit Debug; fixed validation errors contain none of the input values. No credentials,
passwords, tokens, filesystem descriptors or executable byte buffers are stored.

## InitialBuildV1 proposed-settings gate

The plan must explicitly contain all twelve environment entries in the
[trust-contract allowlist](execution-trust.md#environment-resources-and-output),
with no additional, duplicate or case-substituted names. Fixed values match exactly,
including empty compiler wrappers. `RUSTC` equals the reviewed compiler path;
`HOME`, `CARGO_HOME`, `CARGO_TARGET_DIR` and `TMPDIR` equal the corresponding
reviewed directory paths. No value is synthesized from the service environment.
Input ordering does not affect policy validation, although exact plan equality
continues to include environment ordering and therefore invalidates a changed plan.

`PATH` is at most 1024 bytes and consists of distinct, absolute normalized components.
It includes the exact parent directories of both reviewed executable paths. Empty,
relative, dot/parent and duplicate components are rejected. A component cannot be
an owned directory, its descendant or its ancestor; comparisons use path-component
boundaries so a disjoint sibling such as `one-tools` is not confused with `one`.
This conservative lexical rule does not inspect symlinks or case/filesystem aliases,
prove a search directory trusted, or exclude the original user checkout (which is
not represented by these job-directory observations). Those remain external checks.

Proposed elapsed time must be 1–60 seconds and retained log bytes 1–1,048,576,
inclusive. Lower limits are allowed and remain visible in the plan. Other fixed
policy requirements such as CPU, memory, process/filesystem caps, network denial
and child cleanup still need an enforcing runner. Passing this gate is never proof
that OS limits or environment isolation have been applied. BuildPlan itself retains
its broader structural validation; only reviewed/approved settings pass this gate.

## Encoding version 1

SHA-256 is computed over these explicit bytes, never Rust Debug or a platform-native
structure layout. The header is ASCII `devoption.review-context`, one NUL byte,
then version byte `01`. Every field is a one-byte tag, an unsigned 64-bit big-endian
payload byte length, then the exact payload. Strings use UTF-8 with no terminator.
Digest payloads are the raw 32 bytes. Integers use fixed-width big-endian bytes.

| Tag | Payload |
| --- | --- |
| 1 | InitialBuildV1 policy byte `01` |
| 2 | Rust version |
| 3–6 | Cargo path, Cargo hash, rustc path, rustc hash |
| 7–11 | Toolchain manifest, catalog binding, source snapshot, dependency bundle, configuration hashes |
| 12 | Execution UID, u32 |
| 13–19 | Work, Source, Home, CargoHome, Target, Temporary, Artifact directory records |

Each directory payload is another ordered TLV sequence: tag 1 path, tag 2 device
(u64), tag 3 inode (u64), tag 4 owner UID (u32), tag 5 mode (u32). Role is encoded
by its outer tag. There are no optional fields, locale-dependent values or implicit
map ordering. Length prefixes prevent concatenation ambiguity. No decoder or
persisted-format compatibility promise is introduced in this slice; future schema
changes must increment the encoding version rather than silently reusing it.

The encoder-only synthetic vector in `review_context/tests.rs` has fingerprint
`14476c542c423164edc0956cb8ba3f2c026874ac38c4f982b5cf5bb02bde511b`,
independently reproduced with Python hashlib and big-endian struct encoding.
It is a fixture fingerprint, not a real verified build context.

## Verification still required

All filesystem identities and executable/manifest hashes remain supplied claims.
Validation never opens files, resolves symlinks, hashes an installed executable,
checks ACLs or current ownership, or verifies dependency archives. A structurally
valid record can still lie. Its digest establishes only deterministic metadata
identity. Cross-role ancestry, mount containment, directory freshness, actual
permissions and writable access need separate verification. The
[read-only directory verifier](reviewed-directories.md) now checks retained handles
and path bindings for one private same-account Work tree. It is not called by
context validation or approval issuance, and it does not verify ACLs, mounts,
content or the other context fields. The separate
[executable verifier](reviewed-executables.md) hashes retained Cargo/rustc handles
and checks current path bindings; it also has no context-validation or approval
integration and does not verify the complete toolchain manifest.

The future producer must define and verify the underlying toolchain, catalog and
configuration manifests; their internal byte formats are not specified here.
Configuration must cover the controlled Cargo/dependency layout and effective
allowlisted environment, not an unexplained caller-chosen constant. The plan
already retains environment entries and source/dependency metadata. Its syntax
validation is broader than this context's initial-policy gate; neither verifies
installed binaries or controlled Cargo configuration.
Policy tagging describes required limits, not evidence that a runner enforces them.

A human review UI must show the relevant manifest contents and effective settings,
not only these fingerprints. There is no UI or production producer here. A future
adapter must authenticate the explicit owner action, verify the full record before
issuance and again before admission, revoke on failure, and serialize cancellation
with consumption/admission. No process can start or emit a RunnerEvent through this
API. Existing approval expiry, replay, capacity and restart behavior is unchanged.

Run `cargo test -p devoption-builds --locked --offline` and the
[workspace checks](development.md#checks). Tests cover canonical ordering, immutable
copies, path/role/identity/permission rejection, plan binding, field sensitivity,
TLV ambiguity, the independent golden vector and changed-context invalidation.
