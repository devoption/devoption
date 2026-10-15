# Read-only reviewed-executable verification

[#200](https://github.com/devoption/devoption/issues/200) adds
`devoption_builds::reviewed_executables::ReviewedExecutables`. It compares the two
Cargo/rustc files against the SHA-256 values in a [typed review context](review-context.md),
retains private read handles and rechecks their bytes and current path bindings.
It never launches either file, runs version commands, installs tools or changes
permissions. Reads can update filesystem access times.

There is no production caller, route, approval/admission integration or process
execution. A matching hash checks supplied byte identity, not provenance, version
claims, a complete toolchain manifest or safe runtime behavior.

## Accepted files

The context execution UID must match the current non-root effective account.
Each file must be nonempty, regular, have exactly one hard link and be owned by
that account or root. Group/other write and setuid/setgid/sticky bits are rejected.
Current-account files need owner execute permission; root-owned files need other
execute permission. Owner writes are allowed: retained handles do not freeze bytes.
Cargo and rustc must have distinct device/inode identities.

Explicit absolute normalized UTF-8 paths retain the context's 4096-byte limit and
have at most 64 components. Traversal starts at `/`, opening each directory with
`O_DIRECTORY`, `O_NOFOLLOW` and `O_CLOEXEC`. The final read-only open also uses
`O_NOFOLLOW`, `O_CLOEXEC` and `O_NONBLOCK`; metadata rejects special entries before
any content read. Nonblocking opens avoid waiting for a FIFO writer. They do not
make arbitrary device opens side-effect free. Paths must come from trusted local
selection, never an arbitrary browser path inspector.

## Bounded hashing and revalidation

Each file is limited to 128 MiB. SHA-256 uses 64 KiB chunks and positioned reads,
so concurrent rechecks do not share a seek cursor. One extra byte can be read to
detect growth past the limit; larger metadata is rejected before hashing.
Device, inode, UID, GID, mode, link count, size, modification time and change time
must remain equal across reads and to the retained initial observation. Actual
bytes read must equal the observed size.

`open(&ReviewContext)` hashes both files, retains their handles, then revalidates
the pair. `revalidate()` rehashes retained bytes, compares their reviewed digests,
and reopens current paths without following symlinks to check their identities.
A final binding pass checks both files again after hashing the second. Replacement,
unlinking, detected metadata/content changes or hash mismatches fail closed; there
is no repair or adoption of replacements. Drop only closes handles.

Errors expose fixed categories without paths, hash values or raw OS details.
The retained capability has no Debug/serialization interface or descriptor getter.
A future caller must revoke approval and stop admission on verification failure.

## Limits

These are point-in-time checks, not an atomic snapshot or an execution boundary.
Same-user/root writers can modify files after checks or restore state between them.
Timestamp precision and filesystem semantics constrain change detection. Retained
handles prevent simple rebinding from replacing the retained object, but cannot
prevent writes or make a later path-based launch safe.

Ancestor ownership/modes, ACLs, mount aliases, network filesystem behavior and
inherited writer handles are not verified. There is no filesystem I/O deadline.
The executable bit is a metadata requirement, not proof of effective access or a
valid executable format. Root-owned and foreign-owner fixtures are not created by
tests, which run without elevated privileges.

The full toolchain manifest, linkers, shared libraries, compiler support files,
version claims, Cargo configuration, source/dependency contents and runner isolation
remain unverified here. The [directory verifier](reviewed-directories.md) is a
separate capability. Neither helper is automatically invoked by context validation
or approval issuance. The [execution trust contract](execution-trust.md) remains
the authority for future admission requirements.

## Validation

Run `cargo test -p devoption-builds --locked --offline reviewed_executables` as an
unprivileged Unix account and the [workspace checks](development.md#checks).
Disposable inert files cover matching/mismatching hashes, repeated concurrent
rechecks, path bounds, metadata/size rejection, symlinks, hard links, replacement,
unlinking and mutation during chunked reads. Linux additionally tests FIFO rejection.
No fixture is launched and no real installed tool is inspected. The implementation
reuses the pinned rustix and sha2 packages without changing dependency versions.
