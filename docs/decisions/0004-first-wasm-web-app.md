# 0004: First single-node Wasm web app

Status: Accepted in [PR #236](https://github.com/devoption/devoption/pull/236)
under [#229](https://github.com/devoption/devoption/issues/229). The opt-in
[build](../web-app-builds.md) and [private runtime](../web-app-deployments.md)
integrations implement the selected profile; public-host acceptance remains open.

## Decision

Use Rust `wasm32-wasip2` components implementing `wasi:http/proxy`, hosted by
Wasmtime's Rust API in a separate supervised DevOption worker process. Start with
one trusted, stateless HTTP app on Ubuntu 24.04 x86_64. The worker owns a private
HTTP listener; the guest exports the incoming-request handler and never binds a
port. The shared reverse proxy owns public DNS routing and TLS.

Initial pins are Rust **1.99.0** (the repository toolchain), Wasmtime,
`wasmtime-wasi` and `wasmtime-wasi-http` **49.0.3**, and guest bindings
`wasip2` **1.0.1+wasi-0.2.4**, exporting `wasi:http/proxy@0.2.4`.
Use the generated guest bindings directly, without a web framework or custom WIT
interface. The older guest binding pin deliberately fixes the small WASI 0.2.4
contract; it is not a claim that it is the newest binding release. Exact lockfile
and archive identities remain mandatory. Recheck supported patches and upstream
advisories when introducing dependencies; changes require a reviewed pin update.
No dependency or crate is added by this decision.

Choose the embedded API to enforce request admission, output bounds and denied
outbound HTTP in the host. Do not embed guest execution in the authenticated
control-server process. Reuse the existing CLI as the worker entry point where
practical; add a runtime crate only when implementation needs that boundary.
The stock `wasmtime serve` CLI is useful upstream evidence, but is not the selected
DevOption deployment adapter. No Wasm-to-native artifact distribution, containers,
WASIp3, general ABI selection or guest socket server is needed for this path.

## Application and build contract

The initial fixture is one standalone package `devoption-web-fixture`, with
`Cargo.toml`, committed v4 `Cargo.lock` and `src/lib.rs`. Its library name is
`devoption_web_fixture` and its sole crate type is `cdylib`. Set `build = false`,
use edition 2021, and pin the only direct dependency as `wasip2 = "=1.0.1"`.
The lockfile must select the full `1.0.1+wasi-0.2.4` identity and archive checksum;
Cargo requirements do not pin build metadata. Transitive dependencies are also
locked, verified and supplied offline. Reject root build scripts, extra targets,
workspaces, custom Cargo configuration and unsupported manifest fields.

The app implements the generated incoming-handler trait and exports it with
`wasip2::http::proxy::export!`. It serves these fixed responses:

| Request | Revision A | Revision B |
| --- | --- | --- |
| `GET /` | 200 HTML containing `DevOption app revision A` | 200 HTML containing `DevOption app revision B` |
| `GET /healthz` | 200 plain text `ok A` | 200 plain text `ok B` |
| Other paths | 404 | 404 |
| Non-GET methods | 405 | 405 |

Embed the HTML and revision marker in source. Commit the revision B change
separately so update evidence has two actual source SHAs and artifact checksums.
No database, persistent guest files, browser assets pipeline, secrets, runtime
configuration or outbound calls are required. Deployment configuration belongs to
the host: release identity, private port, hostname mapping and policy version.
Guest environment and arguments are empty.

The owner creates a project in the UI, stages a trusted checkout over SSH under
an explicitly configured repository root, and attaches it through the existing
repository flow. The owner commits the manifest, lockfile and source before
requesting a build. The platform selects and revalidates a full current commit,
captures its immutable source, and builds an owned copy; it does not run hooks,
modify the original checkout or introduce an editor/Git hosting service.

Before admission, the operator explicitly installs the pinned Rust toolchain and
`wasm32-wasip2` standard library and prepares a verified read-only dependency
bundle for that lockfile. Acquisition is a separate network-enabled setup action;
there is no implicit fetch during the build. Hash the compiler, linker/target
sysroot and controlled Cargo configuration in the review context. The dependency
provider must handle this complete locked bundle, not only the old single-crate
fixture seed. This needs no hosted registry or proxy service.

The build uses the reviewed absolute Cargo binary with this fixed argument vector:

```text
build --release --locked --offline --target wasm32-wasip2 --package devoption-web-fixture --lib
```

The selected output relative to the owned target directory is
`wasm32-wasip2/release/devoption_web_fixture.wasm`. Accept a binary WebAssembly
component, not a native executable, text module or serialized Wasmtime cache.
Check component type/import/export compatibility with the pinned host before
marking the release deployable; compilation/validation is bounded worker work,
not permission to instantiate guest code. Promotion preserves source, toolchain,
target, ABI, policy, dependency bundle, build result and SHA-256 provenance.

Retain the existing [build trust policy](../execution-trust.md), including its
60-second build/handoff deadline, 512 MiB memory, 256 MiB job filesystem and 4 MiB
artifact cap. These are admission limits, not evidence that this component fits.
If the fixture exceeds them, report measurements and make a focused policy change;
do not silently increase them or relabel a failed build successful. Compiling the
DevOption host and Wasmtime itself is platform installation, outside a guest job.

## Hosting and capability contract

Run one app worker in a separate unprivileged OS identity from the owner store and
control server, with only its selected read-only artifact and owned bounded logs
available. The worker is supervised in an owned systemd unit/cgroup on the selected
Linux host. Installation provisions the service boundary; the web handler must not
accept arbitrary unit names, commands or privilege escalation. Detailed unit/setup
implementation belongs to #230/#232 and must test these restrictions.

Load verified component bytes through Wasmtime's component API. Compile once per
worker startup, then instantiate a fresh Store/component per request using the
WASIp2 HTTP proxy bindings. No cross-request guest memory or mutable guest state is
promised. Validate unknown imports fail closed. Allow only the WASI plumbing
required by the pinned component: HTTP types/incoming handler, bounded streams,
clocks and randomness. Supply no preopened directories, inherited environment,
stdin or network grants. Provide denied/empty implementations where standard
library imports require interfaces without granting their underlying resources.

Explicitly deny guest TCP/UDP/DNS and override the HTTP send hook to reject every
outgoing request, including loopback, private IPs and cloud metadata. Wasmtime's
default outbound HTTP hook must not be used. HTTP handlers receive only the
application request; platform authentication headers/cookies are not forwarded.

Initial host policy values, to be implemented and tested in #232:

| Boundary | Initial limit and behavior |
| --- | --- |
| Worker OS resources | One CPU quota, 512 MiB cgroup memory, no swap, 64 tasks; compilation included |
| Startup | 30 seconds to compile, bind and pass the revision-specific health check; failure stays failed |
| Admission | One active request, at most 16 open connections, no waiting request queue; excess returns 503/connection refusal |
| HTTP input | 16 KiB total headers, 64 KiB body, 5-second header/body receipt deadline; reject oversized input before guest execution |
| Guest memory | One linear memory, maximum 64 MiB; cap tables at 10,000 elements and 16 tables/instances per Store |
| Guest work | 10 million fuel units per request plus a 5-second wall deadline covering instantiation, handler and response completion |
| Response | 16 KiB headers and 1 MiB body; bounded buffering before success is sent; reject overflow/trap as 502, deadline as 504 |
| Output | 64 KiB guest stdout/stderr per request, 1 MiB rolling worker log; truncate with an explicit marker, never block on an unread pipe |
| Stop | Stop admission, allow at most 5 seconds for termination, then kill the owned cgroup and verify it is empty |

Use Store resource limits and fuel plus epoch interruption for running guest code;
a timer on an async future alone cannot interrupt a CPU loop. Bound host I/O futures
and allocation separately. Memory-size limits are per memory, hence the explicit
memory-count limit; cgroup limits cover compiler/host allocations as well. Reject
components exceeding the structural limits rather than weakening them. These
values are selected policy, not performance measurements or an isolation claim.

Retain one active deployment and at least the current and previous verified release.
Persist the exact approved desired release/configuration and stopped/running intent.
On service/host restart, revalidate that identity and artifact before resuming only
the previously approved running intent; never rebuild or resume a stopped app.
After an unexpected worker crash, report failure rather than restart indefinitely.
Use unit/cgroup identity, not a reused PID, for stop/recovery. Revision-specific
health must pass before reporting healthy. Update may have downtime; failed update
and explicit return to the previous release are supported, not automatic rollback.

## Integration changes, not a new platform backlog

| Current implementation | Required change in the delivery series |
| --- | --- |
| `devoption-registry/src/cargo_inputs.rs` admits the narrow cdylib manifest and separates exact requirements from full locked identities | #231: materialize the complete verified dependency bundle; parsing does not prove archive availability or resolution |
| `devoption-builds/src/source_snapshot.rs` verifies the sole conventional binary or cdylib source, including complete-tree and fresh checkout checks | #231: connect library snapshots to Wasm plans; the native planner currently rejects them |
| `devoption-builds/src/lib.rs` plans `--bin` without a target triple | #231: version the plan/review identity for target, library kind, ABI, artifact path and complete toolchain inputs; invalidate old approvals |
| Snapshot, review, approval, history, bounded output and supervision helpers | #231: connect real materialization, OS-enforced runner, admission, events, cleanup and automatic log checkpoints to the UI |
| Python artifact handoff is a separate rehearsal helper | #231/#232: implement production collection with the existing no-follow, independent-copy, stopped-writer, size/hash rules; no executable permission requirement for Wasm |
| #18 release identity and #19 native process contract are planned | #232: promote `.wasm` bytes and supervise the trusted host worker, never execute the artifact as an OS binary; persist approved desired state |
| Server origin/session policy is loopback-only | #230/#233: explicit public origin and proxy/TLS policy; do not weaken the existing guards in this task |

For the selected path this record replaces the native binary/port-input assumptions
in #17–#19. Their unaffected approval, integrity, health, stop and recovery criteria
still apply. The private upstream port is a host-worker setting, not a guest `PORT`
environment variable. Public ingress remains #230/#233. Nothing here implements
these tasks or changes the existing private preview's behavior.

## Build integration update (#231)

The [Linux build workflow](../web-app-builds.md) implements the selected compiler
path. The locked dependency needs executable target scratch for its build script:
`WasmBuildV1` therefore splits bounded scratch into a noexec job filesystem and
an executable target filesystem, reserving the remaining job budget for staging,
artifact and logs. This changes no runtime capability grant. Component header,
size and digest handoff checks precede the still-required #232 HTTP world/import
and runtime compatibility validation. Cloud validation is deferred until the local
build/run/deploy path works.

## Private deployment integration (#232)

[Same-node deployment](../web-app-deployments.md) implements the embedded worker,
immutable local releases, review controls and desired-state recovery. Rust std
imports standard WASI interfaces beyond the HTTP proxy world; those interfaces
are supplied with no preopens, inherited arguments/environment or socket grants.
Guest stdout/stderr are each capped at 32 KiB; exhausting either fails the request
with an explicit output-limit marker. This is stricter than silently truncating
and continuing the guest. HTTP/1 connections close after one request; chunked
uploads and streaming responses are outside this initial adapter.

## Bounded fixture validation plan

No guest compilation or execution was performed for this decision. Under #231/#232,
use an explicitly authorized disposable Ubuntu guest and temporary fixture repository;
record exact source commits, full dependency lock, host/toolchain pins, limits,
artifact digests and results. Do not use a personal checkout or production node.

1. Prepare pinned inputs separately, then build revision A offline under the existing
   job limits. Verify source preservation, artifact cap, component world/imports and
   host compatibility. Compile a small negative native/core-module fixture and reject it.
2. Start the selected worker privately under its policy. Verify `/`, `/healthz`, 404
   and 405, plus repeated requests with fresh guest state. Measure startup, job disk,
   build/runtime peak memory and response fuel use before claiming the limits fit.
3. Exercise loop, memory growth, oversized/slow request, output flood, response
   overflow and concurrency fixtures. Demonstrate timeout/limits and complete stop.
   Attempt filesystem reads, environment access, sockets/DNS and outgoing HTTP to
   loopback/private/metadata addresses; prove no host access/network request occurs.
4. Build revision B from a new commit, promote/deploy it, and verify its marker and
   provenance. Reject tampered artifacts, stale approvals and unknown imports; test
   failed health and explicit recovery to A. Restart with running and stopped intent.
5. #234 adds the clean VM and two public HTTPS hostnames. Record all missing checks
   as unverified; local fixture success alone does not satisfy public acceptance.

The first fixture acceptance is the compatibility/size check in step 1. A mismatch
is a concrete blocker to fix in the integration PR, not grounds for another open-ended
architecture exercise or an unreviewed fallback to native/container hosting.

## Upstream evidence

Reviewed on 2026-10-16; documentation and source inspection only:

- [Rust target documentation](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html)
  identifies component output for `wasm32-wasip2`.
- [Wasmtime 49.0.3](https://github.com/bytecodealliance/wasmtime/releases/tag/v49.0.3)
  is the selected stable patch; its crates declare Rust 1.96.0 as the minimum,
  below the repository's 1.99.0 pin.
- [Versioned HTTP embedding example](https://github.com/bytecodealliance/wasmtime/blob/v49.0.3/crates/wasi-http/src/p2/mod.rs)
  provides proxy pre-instantiation and per-request Store/handler integration.
- [Versioned HTTP hooks](https://github.com/bytecodealliance/wasmtime/blob/v49.0.3/crates/wasi-http/src/ctx.rs)
  expose the outbound request hook; denial is DevOption policy to implement.
- [Versioned resource limits](https://github.com/bytecodealliance/wasmtime/blob/v49.0.3/crates/wasmtime/src/runtime/limits.rs)
  and [engine configuration](https://github.com/bytecodealliance/wasmtime/blob/v49.0.3/crates/wasmtime/src/config.rs)
  support store bounds, fuel and epoch interruption; host-wide bounds remain our responsibility.
- [Guest bindings 1.0.1](https://docs.rs/crate/wasip2/1.0.1+wasi-0.2.4/source/examples/http-proxy.rs)
  demonstrate the generated handler trait/export and cdylib pattern. The published
  crate declares Rust 1.82.0 minimum. Its source was inspected, not copied or run.
- [CLI hosting caveats](https://docs.wasmtime.dev/cli-options.html#serve)
  distinguish the standalone development server from a bounded hosting integration.
