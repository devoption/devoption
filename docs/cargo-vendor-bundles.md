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
metadata is derived from the exact request's public/hosted bindings; an eventual
publisher must also create those declared empty source directories.

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

## Validation and remaining integration

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

This establishes owned, verified vendor-bundle bytes and native Cargo compatibility.
Atomic descriptor-anchored disk publication/reopen/recovery, physical-tree
reverification, installed controller/worker wiring and two actual platform
consumers remain. No existing installation or production runtime accepts or
publishes this format automatically. Keep #385 and #383 open until that integration
and its acceptance evidence are complete.
