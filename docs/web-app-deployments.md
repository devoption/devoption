# Deploy a web app on the same node

This opt-in Linux workflow follows [building through the interface](web-app-builds.md).
It promotes successful Wasm builds into local releases and runs one approved app
behind a private loopback HTTP listener. It does not configure public ingress,
DNS, certificates, a registry, or a DevOption product release.

## Operator installation

Use the disposable Ubuntu 24.04 x86_64 rehearsal before a cloud installation.
Install the build workflow first. The build broker, platform storage and checkout
root remain as described in that guide. Deployment uses a separate systemd
`DynamicUser` identity; it never runs the guest as the platform owner.

Build the workspace with the pinned toolchain and locked dependencies. In addition
to `devoption`, it produces `devoption-runtime`. The runtime embeds Wasmtime,
`wasmtime-wasi` and `wasmtime-wasi-http` 49.0.3. Compilation of the platform/runtime
is operator installation, not an application build job.

After checking that `devoption-app.service` and `devoption-app-validate.service`
are not used by another installation, install the broker and runtime:

```sh
sudo install -d -m 755 /etc/devoption /usr/local/libexec
sudo install -m 755 scripts/deployment_broker.py /usr/local/libexec/devoption-deployment-broker
sudo install -m 755 target/release/devoption-runtime /usr/local/libexec/devoption-app-worker
sudo install -d -m 700 /var/lib/devoption-app
```

Create root-owned `/etc/devoption/deployment.toml`, mode 0644, with these exact keys.
Replace the example UID and paths with the existing build installation's values.
Use canonical absolute paths with no links or whitespace. `state` and `worker`,
the configuration, broker, and their ancestors must be root-owned and not writable
by other users. `state` must have mode 0700. Keep it separate from all platform and
build directories. Choose an unused unprivileged private port.

```toml
uid = 1000
build_work = "/home/devoption/build-jobs"
state = "/var/lib/devoption-app"
worker = "/usr/local/libexec/devoption-app-worker"
port = 15000
owner_data = "/home/devoption/owner-data"
history = "/home/devoption/build-history"
repositories = "/home/devoption/repositories"
```

The selected owner, repository and history paths must match `devoption serve`.
The platform OS user needs the additional narrow sudo rule below, alongside the
build broker rule. Substitute the actual account name and validate with `visudo`:

```sudoers
devoption ALL=(root) NOPASSWD: /usr/local/libexec/devoption-deployment-broker *
```

The broker accepts only its configured non-root caller UID and fixed operations;
it accepts no caller-supplied paths, ports, commands or unit names. The platform's
service needs the same explicitly reviewed `NoNewPrivileges=false` exception as
the build broker. The app worker retains `NoNewPrivileges=yes`, no capabilities,
read-only system paths, hidden home/platform storage, and its own dynamic identity.
No new login credentials are required. Installation and sudo configuration are
operator actions, never browser actions.

Add `--app-deployment enabled` to the existing `serve` command, retaining
`--build-execution enabled` and its owner/history/repository flags. Enabling it
reconciles saved intent before the control listener is opened. It does not create
storage directories or enroll an owner. Without it, Releases explains that the
runtime is unavailable and offers no mutation controls.

## Interface flow

1. Build the committed example app using the existing review/approval flow.
2. Open the project's **Releases** tab. Under **Successful builds**, choose
   **Save release**. Promotion independently copies the component into root-owned
   storage and retains source commit, source/review/compiler/dependency/configuration
   digests, target, ABI, build policy, job identity and artifact checksum/size.
   The release ID is SHA-256 of its canonical manifest. Duplicate promotion returns
   the same release; existing release bytes are never overwritten.
3. Expand that saved release. Enter the exact `/healthz` response: `ok A` for the
   example's first revision. Choose **Review deployment**.
4. Review release identity, expected health, private port, runtime configuration
   digest and capability/resource policy. **Approve and deploy** consumes the
   five-minute review once. Changed configuration or deployment state requires a
   new review. The broker rechecks the immutable manifest and component bytes.
5. Refresh health and logs. Healthy means the selected systemd invocation answered
   `/healthz` with status 200 and the exact expected body. The private endpoint is
   on the node, not the computer displaying the browser. Use an SSH tunnel for a
   local browser preview; public routing is a separate capability.
6. **Stop app** persists stopped intent before stopping the owned unit. It confirms
   an inactive/failed unit and empty cgroup before reporting stopped.

Older build receipts from before deployment integration lack explicit provenance
fields. Rebuild those sources with the current platform before promotion. The
platform does not invent missing provenance or upgrade an old receipt in place.
Interrupted pending writes or malformed root-owned state require operator inspection;
the broker never overwrites that evidence or invents a replacement identity.
At most 32 local releases are retained; current and previous releases are not
silently removed. A full store requires operator inspection. This first version
has no release-deletion interface.

## Updates, failures and recovery

Commit revision B, build it, save its release, and review deployment with `ok B`.
Candidate compatibility validation runs in a bounded validation worker before
stopping the active app. It compiles and checks the WASI HTTP exports/imports but
does not instantiate guest code. Starting the candidate then stops the prior
worker; updates can have downtime.

A candidate must compile, bind and pass health within 30 seconds. A failed start
is stopped and retains failed/stopped intent. There is no automatic rollback.
To return to A, select its saved release and approve a new deployment with `ok A`.
This uses the preserved component and provenance; it does not rebuild the source.

On platform or host restart, validate catalog association, saved manifest,
component and installed runtime fingerprint. Only a previously healthy approved
running intent may resume. An interrupted start, stopped app, changed runtime,
missing association or failed recovery does not gain new execution permission.
Recovery failure is recorded while keeping the control interface available.
The app worker has no automatic restart loop: an unexpected exit is reported as
failed. A later platform restart makes at most one recovery attempt. Pending
browser reviews and build approvals never survive restart.

App traffic is a separate HTTP listener and never passes through the platform's
owner/session API. Incoming cookies, authorization, proxy authorization and
`x-devoption-*` headers are removed. The host's invocation header binds health
checks to the selected systemd invocation. Guest response headers cannot replace it.

## Enforced runtime policy

The worker receives only the selected read-only component. WASI contexts inherit
no arguments, environment, stdin or preopened directories. Standard WASI interfaces
needed by Rust's WASIp2 standard library are linked with these empty/denied grants;
unknown imports fail validation. TCP, UDP, DNS and outbound HTTP are denied,
including loopback, private addresses and metadata endpoints. The outbound HTTP
implementation is compiled without the default send-request feature and explicitly
returns request-denied.

Each request creates a fresh Store/component. Limits are one 64 MiB linear memory,
16 tables/instances, 10,000 table elements, 10 million fuel units and five seconds
covering instantiation, handler and complete response. Fuel and epoch interruption
bound guest code; a wall deadline bounds asynchronous host I/O.

The HTTP adapter admits 16 connections and one active guest request, with no
waiting guest queue. It accepts HTTP/1 requests with a five-second input deadline,
16 KiB headers and a 64 KiB body. Ambiguous lengths, transfer encoding and
absolute-form request targets are rejected. Each connection serves one request
and closes; chunked uploads, upgrades and streaming responses are not supported.
Responses are buffered with 16 KiB headers and a 1 MiB body cap. Traps/overflow
return 502, asynchronous execution deadlines 504, input deadlines 408, and excess
request admission 503. One failed request must not kill the worker.

Guest stdout and stderr are each bounded to 32 KiB per request. Exhaustion fails
the request, records an explicit limit marker, and never blocks on a full unread
pipe. The worker log rolls at 1 MiB; the interface displays at most the latest
64 KiB and escapes it as text. Logs are loaded on refresh, not streamed live.
Worker logs live in `/run` and reset on host reboot; release manifests and approved
intent persist in the root-owned state directory.

The systemd boundary applies one CPU, 512 MiB memory, no swap, 64 tasks, no core
dumps, no capabilities, a five-second stop grace and whole-cgroup termination.
Only the fixed units are targeted. These limits do not make this an approved
multi-tenant service for hostile application authors. The initial workflow is for
trusted source and dependencies on one disposable/controlled node.

## Validation

Run the [contribution checks](development.md#checks). The broker's filesystem,
integrity, stale-configuration, failed-health and recovery tests are in
`scripts/test_deployment_broker.py`. Browser-action tests cover fresh CSRF, single
submission, exact review input and ambiguous responses without retries.

For explicitly authorized disposable runtime validation, compile the example app
and separately build `scripts/fixtures/runtime-probes.rs` using the example's
pinned manifest/lock/vendor bundle. Never replace a user's app to run probes.
Against explicitly started loopback workers, run:

```sh
python3 scripts/check_app_runtime.py 15000 --fixture A
python3 scripts/check_app_runtime.py 15001 --fixture probes
```

The probe validates denied files/environment/sockets/DNS/outbound HTTP, control
header removal, CPU/memory/output limits, deadlines, concurrency admission and
subsequent worker health. The fixture checks `/`, `/healthz`, 404 and 405. Both
exercise oversized and incomplete input. Local worker proof does not replace
Linux systemd, browser deployment/recovery or later cloud/DNS acceptance evidence.

### Disposable local evidence

The issue #232 rehearsal used Ubuntu 24.04 x86_64 under local QEMU, the pinned
Rust toolchain and Wasmtime versions, and a release build at optimization level 1.
Three real interface-approved builds supplied revisions A, B and the capability
probe. A and B served their expected bodies, 404 and 405 responses through the
private listener. An intentionally wrong health expectation stopped the candidate;
selecting saved A restored its original bytes without rebuilding.

The Linux probe passed denied filesystem/environment/socket/DNS/outbound HTTP,
control-header stripping, fuel/memory/response/stdout limits, deadline, concurrent
request rejection and health-after-failure checks. Systemd reported DynamicUser,
no capabilities, NoNewPrivileges, 512 MiB memory, no swap, 64 tasks and one CPU.
Killing the app worker reported failure without an automatic restart. Tampered
release bytes and stale configuration/deployment identities were rejected while
the unrelated existing platform process remained running.

The approved A release resumed after both platform restart and a full VM reboot.
After **Stop app** in the interface, a second VM reboot kept its worker inactive
and private port closed. Neither reboot started a new build.

These are local private-endpoint results, not production readiness or cloud DNS,
ACME, public app routing, multi-user or hostile multi-tenant acceptance.
