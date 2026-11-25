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

Relative paths are bounded ASCII names with letters, digits, `.`, `_`, `-`, `+`, `@`
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

## Pinned public WASIp2 closure

Registry-enabled preparation also supports the exact public closure committed in
`examples/static-site/Cargo.lock` under [#425](https://github.com/devoption/devoption/issues/425):
wasip2 `1.0.1+wasi-0.2.4` → wit-bindgen `0.46.0` → bitflags `2.13.2`.
The independent SHA-256 allowlist in `library/vendor/wasip2.rs` authenticates every
byte of each complete upstream archive, including manifests, sources, build
script and allocator. Lock checksums must match those pins and the actual bytes;
name/version alone never grants the exception. Public coordinates cannot be
replaced by hosted coordinates. Outgoing lock edges must match this exact chain;
bitflags has no outgoing edges. All archive/path/resource checks still apply.

wasip2's default features select std and forward bitflags to wit-bindgen, whose
default features are disabled by wasip2. Only its optional bitflags edge is
admitted. The pinned wit-bindgen build script copies and links its included
`src/rt/libwit_bindgen_cabi.a` canonical-ABI allocator on wasm32; it has no build
dependencies. Direct root public wit-bindgen is also admitted with exactly
`version = "=0.46.0", default-features = false`; additional keys, including
features, are refused. Incoming edges from any package other than pinned wasip2
remain refused, preventing default macro or async activation hidden by an
incomplete supplied lock. Root feature requests
remain unsupported. Inactive optional declarations and feature definitions in
these authenticated manifests grant no permission to acquire extra packages.
The included `wasi-cli@0.2.0.wasm` is an inert archive member; `@` is accepted in
safe relative paths without relaxing traversal, special-file or collision checks.

Other dependencies retain the restricted manifest profile above. New versions,
features, target dependencies, proc macros or build scripts need a separately
reviewed policy change; there is no permissive fallback. The public fixture
archives and attribution are documented in
[`wasip2-closure/README.md`](../crates/devoption-builds/tests/fixtures/wasip2-closure/README.md).

Preparation still returns HTTP 409 on rejected registry inputs. Controller logs
provide fixed, sanitized stage diagnostics: verify operator public archives
against the committed lock; check current publications, reader grants and source;
or inspect the supported manifest/closure policy and complete lock edges.
Messages include no package coordinates, project IDs, private paths or source bytes.
Restore the valid inputs or grants and prepare a fresh review; an earlier approval
cannot bypass these checks. No upstream network fallback is attempted.

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
When registry support is disabled, the installed global vendor and lockfile
configuration remains the build path. With registry support enabled, the exact
operator-audited public app profile below can also select that path. Missing,
corrupt or unauthorized registry inputs never trigger a retry through it.

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


The #425 regression was exercised on a local disposable Ubuntu 24.04 arm64 VM.
With the main-branch vendor validator, the public static-site returned HTTP 409
before compilation, while the library-only acceptance passed. With the pinned
closure policy, public-only and mixed published-library static-site builds passed
the ordinary authenticated owner review/approval path, component runtime
validation, health/routing and exact CSS/JavaScript HTTP checks. Changed public
archives after review, stale committed-source reviews and revoked mixed-consumer
grants remained rejected. Existing library publication, independent consumers,
version update, retained older lock and staged-input corruption checks passed.
This is disposable installation evidence, not a production rollout or a general
Cargo feature/build-script compatibility claim.

## Operator-audited public app profile

Under [#452](https://github.com/devoption/devoption/issues/452), a public-only
WASIp2 `cdylib` selects the installed prepared vendor when its **entire** committed
lockfile SHA-256 equals `lock_sha256` in the installed build configuration.
Selection happens before registry acquisition, at both preparation and approval;
it is never a fallback after a registry or integrity error. The ordinary source
validator still requires exact direct versions and a closed lock graph, refusing
root features, aliases, target/build dependencies and other unsupported manifest
semantics. Direct public wit-bindgen must use exactly `=0.46.0` with explicit
`default-features = false`; no extra features can be requested on either path.

This reuses the existing `prepare_profile(..., None)` contract: the operator
vouches for the complete prepared dependency tree, including all manifests,
checksums, executable build scripts and possible proc macros. The configured
`vendor_sha256` authenticates actual tree paths, file bytes and modes. Preparation,
approval and execution rehash prepared inputs; the isolated worker enforces
read-only input mounts and Cargo uses per-file checksums with `--locked --offline`. A hash
is an integrity binding, not an audit or permission to download packages. The
operator must verify upstream archive checksums against the approved lock before
preparing vendor inputs; archive acquisition remains outside hosted builds.

We considered expanding the independent archive allowlist to the full serde_json
closure. That would embed serde build-script, conditional dependency and proc-macro
policy in the minimal static-site exception. The prepared profile is the smaller
fit: it restores an existing operator trust boundary without broadening that
exception or implementing a partial Cargo feature resolver. There is one installed
prepared lock, including its root package and edges, rather than a new profile
store or automatic acceptance of other public packages.

The trusted `tests/fixtures/records-json` fixture pins wasip2 `1.0.1+wasi-0.2.4`,
wit-bindgen `0.46.0`, serde_json `1.0.154` and the exact JSON closure: itoa
`1.0.18`, memchr `2.8.3`, serde/serde_core/serde_derive `1.0.229`, proc-macro2
`1.0.107`, quote `1.0.47`, syn `3.0.9`, unicode-ident `1.0.27`, zmij `1.0.23`
(and bitflags `2.13.2`). Serde's derive/proc-macro packages are retained in the
lock/vendor closure; the fixture requests no derive feature. Their presence
is not a general proc-macro exception. Operators audit the entire prepared
closure, including dormant packages. The fixture includes upstream-generated
`devoption:app/records` imports, parses JSON, and exports a WASIp2 HTTP handler.

Every project containing a hosted-registry package still uses registry
materialization and current publication/reader grants, even if its lock matches
the configured prepared lock. Mixed JSON/hosted-library apps outside the restricted
manifest and pinned static-site policies remain unsupported. Other public locks
also retain the existing registry policy. No grant, isolation or runtime records
permission is conferred by selecting prepared inputs.

### Validation and rollout

Ordinary builds tests cover exact-lock selection, checksum/edge/additional-node
mutations, unsafe wit-bindgen declarations, static-site compatibility and refusal
to select prepared inputs for hosted packages. Server tests exercise selection
with registry support enabled, missing public archive refusal on the registry
path, and refusal to bypass hosted consumer grants. Existing archive mutation and
private library consumer tests remain in place.

The explicit trusted Cargo proof requires a preverified Cargo vendor tree for
that fixture, its tree digest (the installed `vendor_sha256` algorithm), and a
Rust toolchain with the `wasm32-wasip2` target and linker. It downloads nothing:

```sh
DEVOPTION_TEST_WASIP2_VENDOR=/absolute/prepared/vendor \
DEVOPTION_TEST_WASIP2_VENDOR_SHA256=THE_VERIFIED_TREE_SHA256 \
cargo test -p devoption-builds --lib prepared_records_json_build \
  --locked --offline -- --ignored --nocapture
```

`DEVOPTION_TEST_WASIP2_RUSTC` optionally selects the matching installed compiler.
The proof uses fresh Cargo homes and targets, verifies a component containing the
records import, builds the existing static-site example, then mutates a disposable
vendor copy and checks both tree-digest
and Cargo checksum refusal. It does not exercise Linux broker isolation, owner
HTTP approval or runtime/database behavior.

Rollout requires a separately authorized controller update. Before enabling the
profile, audit the app's exact committed lock, verify all archive checksums, and
prepare its complete vendor directory using trusted operator tooling. Set the
installed `vendor_sha256` and `lock_sha256` to those verified inputs using the
existing installation workflow; retain previous configuration and vendor inputs
for rollback. Preserve toolchain hashes, storage roots and registry/grant settings.
Restart the owning controller to load the updated configuration, then exercise a
fresh review/approval build and mutation refusals on a disposable installation
before any live rollout. No live deployment is performed by this change.
