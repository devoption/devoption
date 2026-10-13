# Build-plan metadata

`devoption-builds` implements a pure Unix Rust build-plan model under
[#116](https://github.com/devoption/devoption/issues/116), an initial domain slice
of [#15](https://github.com/devoption/devoption/issues/15) and
[#17](https://github.com/devoption/devoption/issues/17). It has no runtime caller,
HTTP route, UI, persistence, approval issuer or executor. Builds remain unavailable.

Every plan also carries an immutable [Cargo dependency descriptor](registry-contract.md)
from the DevOption registry contract added in
[#119](https://github.com/devoption/devoption/issues/119). The descriptor records
manifest/lockfile input hashes, claimed package checksums, original source and serving-registry identity,
and bundle digest/size. Every field participates in plan equality. No lockfile or
dependency bytes are read or verified here, and no direct-public-fetch fallback exists.
The separate [supplied-byte adapter](cargo-inputs.md) validates a narrow Cargo
subset into that descriptor. It does not verify committed source or actual archives.
The [root source verifier](cargo-source-proof.md) separately verifies membership
of both inputs through supplied Git objects relative to a selected commit ID.
Its proof is not yet required by or bound to `BuildPlan`; independent revision
selection, complete source/context and archive verification remain future gates.
The [offline fixture proof](offline-fixture-build.md) separately compiles trusted
test code from a checksum-verified seed. It does not consume a `BuildPlan` or
implement production approval, isolation or job lifecycle.
The separate [job lifecycle model](build-jobs.md) now defines in-memory records and
transition rules using exact plan equality. It has no production runner-event
issuer, persistence or runtime integration.

`BuildPlan::validate` consumes owned `PlanInput` values and reads a supplied
in-memory catalog snapshot. It requires existing, associated project/repository
IDs with a registered default checkout. Metadata-only associations are rejected.
It neither re-inspects that checkout nor reads any filesystem, environment, Git
object or network resource. Tests register synthetic Git layouts in temporary
directories, remove them, then validate against the saved in-memory snapshot.

The immutable result exposes read-only details and a fixed argument vector:

```text
build --release --locked --offline --package PACKAGE --bin BINARY
```

There is no caller-defined shell string, argument list or inherited environment.
No arguments are executed. All input fields participate in plan equality; editing
any reviewed field requires a different plan value. Equality alone is not an
approval, freshness check or durable plan identifier.

## Supported metadata

- A nonzero, lowercase, full 40-character SHA-1 object ID; symbolic refs,
  abbreviations and SHA-256 are unsupported. Syntax does not establish that the
  object exists, is a commit or belongs to the selected repository.
- An exact stable numeric Rust version (`major.minor.patch`, each component at
  most 65535, nonzero major, no leading zeros). Floating channels, prereleases
  and target triples are unsupported. This records the requested version; it
  neither selects nor verifies the installed compiler.
- Explicit absolute Unix Cargo executable and candidate work-directory paths,
  at most 4096 UTF-8 bytes, without controls, empty components, `.` or `..`.
  Cargo's path must end in `/cargo`. Paths are not canonicalized or opened;
  existence, symlinks, ownership and executable identity remain unchecked.
- One package and one binary target, each at most 128 ASCII bytes. Names start
  with a letter or underscore and then permit letters, digits, `_` and `-`.
  This is a conservative subset; arbitrary Cargo flags and target kinds are
  unsupported.
- Up to 32 explicit environment entries: unique, case-sensitive POSIX-style
  names of 1–64 ASCII bytes and values of at most 1024 UTF-8 bytes without control
  characters. Empty values and an empty list are allowed. No variables are
  supplied by default. Callers must supply non-secret values; syntax validation
  cannot detect secrets or decide whether a variable is safe. Input and plan
  types deliberately have no `Debug` implementation, and errors omit values.
- Nonzero `u32` elapsed-second and log-byte limits. These are required proposed
  values, not enforced limits or an approved resource budget. Structural bounds
  do not define production execution policy.

## Before execution can exist

The future approval/execution layer must verify the source commit and fresh
catalog binding, bind and verify the registry descriptor against the committed
manifest, lockfile and actual bundle, create a clean owned work directory without modifying
the user's checkout, verify toolchain/executable identity, and define an explicit
environment allowlist and registry access/network policy. A Cargo `--offline` flag does not
prevent build scripts or compilers from using the network and is not a sandbox.

Approval must bind the complete plan and relevant verified external state to the
owner's explicit review; changes, expiration and restarts need a defined lifecycle.
Execution also needs resource policy and enforcement, cancellation and child cleanup,
bounded escaped logs, job persistence and provenance. None of those permissions or
mechanisms are introduced here. The first workflow remains limited to trusted code
running as an unprivileged OS account; supporting arbitrary untrusted builds would
require a separate isolation design.

The selected architecture acquires dependencies through DevOption's registry/proxy
before a separately approved isolated offline build. Provider transport, allowed
environment/toolchain, owned work-directory policy, resource caps and approval
lifetime remain undecided. This model chooses no production paths, environment
permissions or execution defaults.
