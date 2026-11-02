# Scoped HTTPS integrations

The opt-in Linux WASIp2 runtime can authorize **one fixed HTTPS JSON POST
endpoint** using one project environment secret as bearer authorization. This
capability is disabled by default. It does not enable guest sockets, DNS, arbitrary
HTTP, raw secret reads, native service hosting or build-time injection.

## Operator policy and owner approval

Enable [encrypted project secrets](project-secrets.md) and the existing
[deployment workflow](web-app-deployments.md) first. An operator adds this table to
the protected deployment configuration after reviewing the intended provider:

```toml
[https]
project = "0123456789abcdef0123456789abcdef"
environment = "preview"
secret = "SERVICE_API_KEY"
revision = 1
endpoint = "https://api.example.com/events"
```

These are generic examples, not provisioning commands or usable credentials.
Select the actual saved project's environment, secret name and **exact current
revision**, visible in Settings → Environments. The endpoint must be a canonical
lowercase ASCII DNS hostname on HTTPS port 443 with a literal path: no userinfo,
IP literals, explicit port, query, fragment, percent encoding or dot segments.
Its project must match an installed app hostname binding, if one exists.

Promoting a successful build snapshots this value-free policy into its immutable
release identity. A previously saved stateless release stays stateless. The owner
then prepares deployment review, which displays environment, name/revision,
endpoint and POST authority. The controller resolves that exact revision at
preparation and again at approval. Missing, changed or removed values fail closed;
a current value is never silently substituted. Approval also binds the installed
configuration/worker fingerprint and current deployment generation.

The broker accepts only the configured controller UID. It rechecks the selected
policy and release, resolves public IPv4 addresses with a three-second DNS bound,
and rejects empty, excessive, private, loopback, link-local, multicast or mixed
public/private answers. Addresses are pinned for this worker lifetime and the
systemd network filter permits only those addresses plus its private listener.
Provider IP changes require another reviewed deployment; there is no DNS refresh
or fallback to a new address in the worker.

## Delivery and guest contract

Only the selected value crosses the controller/broker pipe. Values never enter
process arguments, browser responses, release manifests, build inputs or persisted
deployment intent. A root-private temporary file under `/run` supplies systemd
`LoadCredential`; the broker removes this source after systemd starts the unit.
The runtime reads the per-unit credential into host memory. It receives no master
key or owner-store mount. The guest has no filesystem preopens or inherited
arguments/environment, and cannot read the credential file. Both DynamicUser
stateless workers and the existing dedicated record-worker identity are supported.
See [systemd credentials](https://systemd.io/CREDENTIALS/) for the underlying
per-unit credential mechanism.

Use the standard `wasi:http/outgoing-handler` import, already available in the
runtime. The guest sends:

- The exact approved HTTPS authority and path, method `POST`.
- `Content-Type: application/json` and a valid JSON body of at most 32 KiB.
- `Idempotency-Key`: 1–128 ASCII letters, digits or `._:/-`. Use a stable persisted
  operation identity when retrying the same operation.

The guest supplies no Authorization or Cookie header. The host constructs the
provider request and adds `Authorization: Bearer <selected value>`. This first
slice accepts nonempty visible ASCII bearer tokens up to 8192 bytes; other secret
formats can still be stored, but cannot be used by this capability.

Only one outgoing attempt is allowed per incoming app request, including rejected
attempts. The host enforces a three-second overall bound, a two-second connection
bound, verified TLS using embedded public roots, HTTP/1, no ambient proxies, no
redirects, and no automatic retries. It rejects response headers above 16 KiB and
response bodies above 64 KiB. It returns status and body with a JSON content type;
upstream headers are not forwarded. Literal credential echoes are rejected. The
operator-approved provider remains trusted with the credential and payload;
this is not protection against a malicious provider encoding a secret in its
response. See [reqwest's client configuration](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html)
for the transport controls.

Non-success HTTP status is a provider response, not confirmed success. TLS,
connection, timeout and body errors may occur after the provider accepted a POST.
Do not retry with a new operation key after an ambiguous result. Interpret the
provider's response and idempotency contract in the application. This slice adds
no durable queue, scheduling, retry worker or transactional outbox. The existing
five-second app request deadline and other guest resource limits remain in force.
The [durable-job proposal](background-jobs-proposal.md) describes a separate
future execution contract; it is not enabled by an HTTPS grant.

## Rotation, stopping and recovery

Replacing or removing a saved value does not alter a running worker's approved
snapshot. Stop the app and revoke the upstream credential when immediate
revocation is required. To use a replacement, update the operator policy revision,
promote the same successful build again, then review and deploy its new release.
A previously saved release that requests the old revision cannot restart with the
replacement. Policy removal/change also blocks reuse of that release's grant.

A raw systemctl restart cannot restore a deleted credential source. On controller
startup, previously approved healthy running intent may recover only if its exact
policy, revision and existing secret key/store remain available. The controller
resupplies the selected value privately; missing bindings stop recovery while the
control interface remains available for a fresh review. Stopped intent stays
stopped. No plaintext value is recovered from a backup of release metadata.

## Disposable proof and limits

Build `examples/https-app` with the pinned Rust toolchain and `wasm32-wasip2` target,
then run:

```sh
cargo build --manifest-path examples/https-app/Cargo.toml --release \
  --target wasm32-wasip2 --locked --offline
python3 scripts/check_app_https.py \
  --component examples/https-app/target/wasm32-wasip2/release/devoption_https_fixture.wasm
```

The checker starts a loopback TLS endpoint with a disposable CA and synthetic
bearer token. An explicitly ignored Rust test invokes the real compiled guest and
host hook. Only this test binary can inject that loopback resolver/custom CA;
production credential loading has neither override. It proves the allowed POST,
TLS issuer/name refusal, ignored proxy variables, denied host/path/missing retry
key, malformed/oversized JSON refusal, one-attempt rule, redirect refusal,
response header/body bounds, timeout and literal credential-echo refusal. No provider is
contacted and no system trust is installed.

Linux CI additionally passes `--https-component` to the existing explicitly
**disposable-system-host** managed-record checker. It exercises real systemd
credential loading with both worker identities, metadata/argv secrecy, exact
recovery delivery, missing-delivery refusal, guest isolation, retained record data
and return to a saved stateless release. It performs public DNS resolution of
`example.com` but sends no HTTPS request there. Never run that privileged checker
on a workstation or an existing installation.

These proofs are separate: the local TLS checker tests the actual guest/transport;
the Linux checker tests the actual broker/systemd custody and recovery path. This
does not establish real-provider delivery, app-specific rate limiting, durable
background work, native-service credential integration or production readiness.
