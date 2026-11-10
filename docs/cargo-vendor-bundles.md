# Verified Cargo vendor bundles

`Publications::vendor_inputs` constructs a deterministic `VendorInputs` value from
an [authorized locked archive set](package-publication-policy.md#exact-locked-archive-acquisition)
and the consumer's fresh verified source under
[#385](https://github.com/devoption/devoption/issues/385). It revalidates current
owner/project policy, source, catalog and exact archived inputs before parsing.
The caller must authenticate and serialize that registered-source selection as
for locked acquisition. This native trusted-adapter API performs no network,
filesystem publication, Cargo execution, build approval or production changes.

## Archive and manifest verification

The reader accepts a single complete gzip member containing regular files and
optional directory headers in POSIX ustar or simple GNU-header format. It checks
gzip integrity, tar header checksums, exact `name-version/` roots, octal sizes,
padding and complete zero-block termination. Concatenated/trailing gzip data,
truncation, links, devices, FIFOs, sparse files, GNU extension fields/long-name
records and PAX extensions are refused. No general-purpose extraction occurs.

Relative paths are bounded ASCII names with letters, digits, `.`, `_`, `-`, `+`
and `/`, at most 240 bytes and 24 components. Absolute, empty, dot/parent,
backslash and control paths, duplicate/case-colliding names and file/directory
collisions are refused. `.git`, `.cargo` and `.cargo-checksum.json` components are
reserved, including case aliases. An archive cannot supply a checksum manifest
that the projection would overwrite. File ownership, timestamps and executable
bits are not carried into the output; every projected file has mode `0644`.

Each package's actual `Cargo.toml` name/version must equal the locked identity.
The initial manifest profile supports a conventional library (or a safe explicit
library path), editions 2015/2018/2021/2024, disabled/absent build scripts and
normal registry dependencies. Cargo SemVer requirements are matched against the
exact outgoing lock edges, preserving public/hosted origins and explicit
`registry-index` bindings. Matching an unrelated node elsewhere in the package
set is insufficient. This uses pinned `semver` 1.0.28 already in the workspace lock.

Build dependencies/scripts, native `links`, proc macros, nonempty feature groups,
requested dependency features, active optional edges, target-specific normal/build
dependencies, workspace/path/Git dependencies, patches/replacements and unsupported
manifest semantics are refused. Optional declarations are accepted only when
inactive and absent from that node's lock edges; this profile activates no
features. Dependency dev/test/bench metadata is not executed or acquired: those
targets are not built when the package is used as a dependency. Broader Cargo
feature/target/build-script semantics require another explicit profile, not a
partial verification claim or automatic fallback. Projection does not prove that
Rust code compiles or that arbitrary package metadata is accepted by Cargo.

## Projection, identity and bounds

All original regular-file payload bytes remain unchanged. The adapter generates
one root `.cargo-checksum.json` per package, binding the original exact archive
SHA-256 and every projected original file's actual SHA-256. Public and hosted
packages have disjoint directory-source buckets. Public packages use `public/`;
hosted buckets hash the exact logical registry ID and index URL. Within a bucket,
`name-version/` retains exact spelling. Identical public/hosted coordinates never
share a directory or checksum identity.

| Bound | Limit |
| --- | --- |
| Expanded tar per package | 16 MiB |
| Expanded tar across the entire set | 64 MiB |
| Relative file/directory path entries per package | 8192 |
| Projected files, including generated manifests | 16384 |
| Projected file payloads, including generated manifests | 64 MiB |
| Final relative vendor path | 512 bytes |

The existing raw archive/lock-graph bounds also apply. A failure returns no partial
vendor value. The adapter owns the exact canonical bundle bytes, exposes borrowed
regular-file views and implements `CargoRegistryProvider` only for its complete
captured dependency request. Its `CargoMaterialization` digest/length describe
these actual bytes, rather than a placeholder or a serialized Rust metadata claim.
The descriptor alone still grants neither current access nor execution authority.

The byte format is `DVVENDOR1` (9 bytes), a big-endian u32 file count, then records
in lexical path order: big-endian u32 path length, ASCII path bytes, big-endian u16
mode (`0644`), big-endian u64 content length and exact content bytes. Empty inputs
produce the 13-byte header/count. Total encoded size is bounded by 64 MiB payload
plus at most `16384 * (512 + 14) + 13` bytes of framing/path data. Source bucket
metadata is derived from the exact request's public/hosted bindings; the attempt adapter also creates those declared empty source directories.

`cargo_config(root)` generates source replacement for a trusted absolute ASCII
vendor mount, with no upstream fallback or credentials. Registry aliases and
replacement names cannot collide. The selected runtime mount/configuration is
separate build-review context; the helper does not inspect paths or prove that a
root already contains these files. The existing worker's global vendor-tree digest
is **not** this bundle-format digest. Do not substitute one for the other.

`VendorInputs::revalidate` requires its exact captured archive view and fresh
source/catalog/policy before a later handoff. Revocation invalidates future use;
already acquired bytes cannot be retracted. Current policy does not authorize a
caller to reset a persistent store or overwrite an installation.

## Attempt-local worker integration and validation

Unit tests cover archive checksums/termination/CRC, traversal, reserved paths,
special/extended entries, collisions, expansion/count limits, actual manifest
identity/edges, unsupported executable/feature semantics, exact checksums,
descriptor bytes, alias-safe configuration, empty inputs and revocation. Public
and hosted identical coordinates produce separate verified vendor directories.

The explicit trusted offline Cargo proof now writes the checked file views into
disposable fixture directories and builds two consumers through the generated
directory sources. It covers an update, an older locked version and fresh-cache
refusal after deletion/corruption of a vendored source file. Fresh yank resolution
is still checked through the approved index; directory sources preserve the
already selected lock and do not implement a registry's resolution/yank service.

With both the installed Linux runner and the explicit registry root enabled,
existing build preparation acquires the entire source lock through current
publication/project-reader policy. Public archives, when required, must already
be supplied by the operator as private `public-archives/NAME-VERSION.crate` files
under that root; no download or public fallback occurs. The adapter rechecks the
selected root, exact source/catalog, policy head, raw archives and vendor graph.

Preparation exclusively creates `ATTEMPT/dependencies` through directory
descriptors, writes only verified files and a generated `cargo-config.toml`, syncs
them, and verifies the complete inventory, directory identities, modes and bytes.
Directory count is bounded at 16384. The review's dependency digest covers actual
bundle bytes; its configuration digest additionally binds the source-replacement
configuration and physical directory identities. Approval reacquires current
policy and public/hosted archive bytes and rechecks the physical inputs. Execution
repeats physical verification before consuming approval. Policy operations in the
same controller remain serialized through worker admission.

The broker derives this fixed child from the existing attempt token and mounts
it read-only at `/vendor`. The worker accepts only direct directory-source
replacement configuration with explicit matching registry identities; Cargo stays
locked, offline, uncredentialed and inside the existing isolation/resource policy.
When registry support is disabled, the existing installed global vendor and
lockfile configuration remains the build path. Missing, corrupt or unauthorized
configured-registry inputs fail; they do not select that legacy path.

Inputs belong to the attempt and follow existing history/retention cleanup. They
are never a shared persistent vendor store: a restart forgets pending reviews,
and a later preparation reacquires and verifies everything into a new attempt.
No persistent publication, reopen/recovery protocol or separate vendor cache is
needed for this lifecycle. Operator writes to controller-owned storage remain
trusted external changes; this is not isolation from the installing owner.

The disposable Linux acceptance extends the installed authenticated controller
and broker path with one built/staged/explicitly published library, two separately
registered WASIp2 consumers, an explicitly published update, an older-lock rebuild,
and missing/corrupt/revoked/staged-tamper refusal. It uses synthetic fixture data;
it authorizes no production configuration, real publication or deployment.
