# Deployment identity and cleanup validation

On 2026-10-17 (America/New_York), the remaining negative integration cases in
[#19](https://github.com/devoption/devoption/issues/19) passed on a disposable
Ubuntu installation. This supplements the normal deployment lifecycle in the
[cloud report](public-cloud-acceptance.md) and the
[criterion reconciliation](single-node-acceptance-status.md).

## Scope and provenance

The run used a separate copy-on-write overlay of the stopped
[clean local rehearsal](clean-local-rehearsal.md), preserving its base disk.
Ubuntu 24.04.5 x86_64, kernel `6.8.0-146-generic`, systemd
`255.4-1ubuntu8.17`, QEMU TCG with two CPUs and 2 GiB RAM were observed.
The platform controller stayed stopped. The checker explicitly exercised only
the saved public example component, synthetic HTTP/child-process fixtures and
reserved systemd units. No cloud resources, credentials, DNS, certificates or
repository settings were created or changed.

The source baseline was main `ad35592c42b44f6ac66d98c2a5005cd9ddbb10a1` plus the
issue #19 broker/checker changes in this PR. File digests identify the tested
versions without a circular reference to this report's own commit:

| Input | SHA-256 / identity |
| --- | --- |
| `scripts/deployment_broker.py` | `42913335b3ef1ef8c72fac3a546cbcaaa47ece06e5fb9c4d7df3cd1517caa772` |
| `scripts/check_deployment_identity.py` | `8f246c2c7f4d6ec5eb61fe8ac16a8524075f8ea62083359e492fd71b96d5eb0d` |
| Installed Wasmtime worker | `fa388c551d0f1c16aa697943d4cec9ed2481c21859354d130595a25e7e111e1a` |
| Saved release / manifest digest | `4aebab0346cb19a064aa4fff3f53ddc8a171ad768ca11336749e6b906f0e5494` |
| Fixture component, 123,606 bytes | `d085e733cfdb994e7541c3333a8f12220098bef984d7707310b2425c1e8b33f1` |
| Fixture source commit | `4e9471edcb9ca330f67f1462ad9ebc04c81bd116` |

The worker and saved release were reused from the earlier source-built rehearsal;
this run does not claim a fresh platform build or another browser approval
walkthrough. The runtime source has not changed since its implementation in
PR #241. The checker requires its adjacent broker source to match the installed
broker bytes and holds the normal broker lock throughout the run.

## Observed results

| Case | Result |
| --- | --- |
| Occupied private port | A separate dynamic-user HTTP listener served the exact `ok A` body on the app port. Real Wasmtime deployment failed its health check, retained failed/stopped intent and left no candidate cgroup. The unrelated listener's systemd invocation stayed unchanged and its port stayed open. |
| Missing invocation header | Real HTTP health returned false against a known systemd invocation. Missing expected identity was also rejected. |
| Wrong invocation header | Exact health body with another 32-character marker was rejected. |
| Matching marker, positive control | A fixture serving its own systemd `INVOCATION_ID` passed health. |
| Changed systemd invocation | Restarting the reserved fixture unit changed its actual InvocationID. Replaying its old header failed health and saved-deployment status reported failed. |
| Unconfirmed stop | Explicit fault injection suppressed only `systemctl stop` for the app unit. Real systemd/cgroup/HTTP observations remained in use. Stop failed, status stayed stopping, and a subsequent start was rejected without replacing that invocation. |
| Inactive unit with surviving child | A bounded synthetic parent/child unit used `KillMode=process` and `SendSIGKILL=no` to leave a real nonempty cgroup after the parent stopped. Stop and replacement start were rejected; status stayed stopping. The checker then killed only that reserved fixture cgroup. Production continues to use whole-cgroup termination. |
| Real worker, positive control | After confirmed fixture cleanup, the saved component deployed through the real broker/Wasmtime worker, became healthy and explicitly stopped. Final state was stopped with no app/validation/sentinel cgroups. |

The unconfirmed-stop case is deliberate command fault injection, not a claim of
reproducing a kernel/systemd stop failure. The surviving-child case uses actual
Linux process/cgroup state without mocked state or health. Compatibility validation
may run before a replacement is refused; no replacement serving worker starts
until the previous unit's stop and empty cgroup are confirmed.

The new socket regression test fails against the original broker: an absent
expected invocation, absent HTTP header and absent systemd identity could all
compare equal as `None`. The broker now requires a nonempty 32-character lowercase
hex systemd invocation before probing health. The HTTPS route check applies the
same requirement to saved identity. Tests cover malformed identities and a changed
invocation during the health observation. This establishes fail-closed behavior
for missing observations; it does not establish an active production exploit.

## Repeat on an authorized disposable installation

This is an opt-in, destructive integration checker, excluded from ordinary
unprivileged test discovery. Use a dedicated disposable supported Linux
installation containing the installed runtime, valid deployment configuration and
an explicitly selected saved **fixture** release. Stop the controller and app
first. The app, validation and `devoption-identity-sentinel.service` unit names
must be unused by anything else; the configured app port must initially be free.
Do not use a user's live installation or substitute a real application.

After reviewing the exact source, stage both scripts together in a root-owned,
traversable directory (for example `/opt/devoption-identity-check`) and install
the same broker bytes at `/usr/local/libexec/devoption-deployment-broker`.
Changing broker bytes changes the runtime fingerprint; normal deployments require
a fresh review. Replace the two variables below with the disposable installation's
configured platform UID and saved fixture release digest:

```sh
sudo env SUDO_UID="$fixture_uid" python3 /opt/devoption-identity-check/check_deployment_identity.py \
  --disposable-systemd --release "$fixture_release" --health 'ok A'
```

This explicit checker invocation supplies validation authority directly; it does
not create browser approvals or change the production broker's allowed interface.
It prints one JSON result per successful case and a final JSON summary with the
broker/worker/release identities. An assertion or operation failure exits nonzero;
partial case output alone is not a pass. Its finally block cleans only the three
reserved fixture units and persists stopped intent after confirmed cleanup.
Inspect any cleanup error before retrying. The rehearsal overlay was powered off
after the run; the original VM remained stopped and unchanged.

## Ownership assumption and limits

The operator must reserve `devoption-app.service` and
`devoption-app-validate.service` exclusively for this installation, as required
by the [deployment installation guide](web-app-deployments.md#operator-installation).
The broker can stop those fixed units and their cgroups. It cannot establish that
an arbitrary root-created foreign unit using a reserved name belongs to DevOption;
this test does not make that claim. The sentinel listener has a distinct unit,
and neither its PID nor a port owner's PID is ever selected for termination.

Invocation binding protects against stale or unrelated responses under the
trusted single-node profile. It is not a secret credential or protection from a
malicious host administrator. Runtime/resource limits, artifact integrity,
approval freshness and the trusted-source boundary are unchanged. These results
do not approve a product release; [#21](https://github.com/devoption/devoption/issues/21)
remains the release gate.
