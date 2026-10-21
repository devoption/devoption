# Candidate readiness and canary policy

The read-only `scripts/release_promotion.py` command checks one independently
reserved, authenticated candidate. It preserves the full reservation, including
archive, proof and transport digests, signing run/attempt and source commit. It
never rebuilds, rewrites, retags or publishes anything. A failed candidate keeps
its existing immutable identity; use a new version for changed bytes.

`readiness_passed` describes the evidence checks below. **Every result retains
`publication_eligible: false`.** The [#21 release decision](releases.md), explicit
publication approval and any eventual publication writer remain separate. Stable
versions are still rejected by release metadata; even a passing stable-duration
canary cannot promote a preview or relabel its bytes as a stable release.

## Checked-in stop and thresholds

[`release-promotion.toml`](../release-promotion.toml) is the authoritative policy.
It starts with `stopped = true` and `automatic_publication = false`. The evaluator
fails while stopped; the collector makes no requests. To conduct an authorized
canary, the operator must explicitly select a reviewed checkout with its stop
control cleared. Changing that file does not grant installation, workload,
infrastructure or publication authority.

| Gate | Required observation |
| --- | --- |
| Preview window | At least 1 continuous hour |
| Stable readiness window | At least 24 continuous hours; stable version/publication policy additionally blocks promotion |
| Workload | Already deployed trusted `examples/web-app` revision A, independently selected component SHA-256 |
| Per-minute probes | Platform `/sign-in` 200; anonymous `/projects` 303 to `/sign-in`; app `/healthz` 200 with exact body `ok A` |
| Coverage | Samples 60–90 seconds apart, exact first/last samples, no missing route or reordered observations |
| Failures | Zero, including timeouts, TLS failures, bad responses or changed installed identities |
| Latency | Each route's nearest-rank p95 at most 2,000 ms, never a combined percentile hiding a slow app route |
| Freshness | Acceptance completion and final canary observation at most 24 hours old; future timestamps rejected |
| Bounds | At most seven days/10,081 samples, 8 MiB per JSON evidence input |

Policy validation permits stricter limits within the reviewed bounds, not shorter
minimum windows, relaxed failure/latency thresholds or automatic publication.
Any policy-byte change invalidates an observation bound to the previous digest.
The evaluator reads the live policy before packet authentication and after all
checks; a change during evaluation fails. The collector checks it between probes
and every five seconds while waiting. Each HTTPS request has a five-second socket
timeout and a ten-second total deadline, including a peer that drips bytes;
root identity inspections have bounded subprocess waits. Identity is rechecked
after all three probes, and the whole sample must retain coverage within 90 seconds.
Setting `stopped = true` stops collection/readiness checking, not running apps or
an unrelated maintainer command. There is no publication writer to stop today.
A future writer must reauthenticate the packet and re-evaluate the current policy
immediately before a serialized write; a saved readiness report grants no authority.

## Collect on an existing canary node

Installation and app deployment must already have been explicitly authorized and
completed. The collector only reads identities and issues anonymous HTTPS GETs.
It creates no account, key, DNS record, service, infrastructure or workload.
Use the reviewed source checkout and the same pinned `gh` verifier required by
[packet verification](bundle-installation.md).

```sh
sudo python3 scripts/release_canary.py \
  --reservation release-candidates/REVIEWED_VERSION.json \
  --packet /var/tmp/REVIEWED_CANDIDATE.zip --gh /opt/attestation-gh/bin/gh \
  --channel preview --platform-hostname platform.example.com \
  --app-hostname app.example.com --app-sha256 REVIEWED_COMPONENT_SHA256 \
  --output /var/tmp/NEW_canary.json
```

Collection requires root on the installed node to inspect its protected receipt,
deployment record and active systemd process identities. It authenticates the
packet without executing downloaded code, compares installed platform/worker
bytes with it, confirms `/proc/PID/exe` matches the expected versioned binary,
and checks the app's saved invocation, manifest/component digest, configuration
fingerprint and running/healthy intent. Identity inspections surround every
sample; an unrelated HTTP listener cannot substitute for those identities.
Both configured hostnames must match. TLS connections pin **127.0.0.1:443** with
the real SNI/Host name and normal system CA trust, preventing DNS from directing
the probes to another node. No alternate CA option, cookies, credentials,
redirect following or response bodies are retained. This tests the node's local
HTTPS ingress; external DNS/routing and ACME remain separate acceptance evidence.

This profile requires a schema-3 bundle with the matching app worker and the
reviewed app configuration. Retained preview.1/preview.2 packets predate that
layout and cannot supply this canary evidence. No successful live observation
for a new candidate is claimed by implementing or unit-testing the collector.

Output is exclusive/private (0600); an existing output is never resumed or
replaced by a new run. Samples are retained as collection progresses. Failure or
interruption leaves partial evidence, which fails complete-window checks. Keep
it for diagnosis and choose a new filename for another run. A wall-clock jump
relative to the monotonic clock stops collection. This is a bounded operator-led
command, not an unattended monitor or deployment controller.

## Acceptance evidence and evaluation

An operator-reviewed acceptance index uses this schema (the real `checks` array
must contain **all ten** cases below, exactly once):

```json
{
  "schema_version": 1,
  "reservation": "REPLACE with the complete separately reviewed reservation object",
  "completed_at": "2026-10-21T12:00:00Z",
  "checks": [
    {"case": "fresh_install", "passed": true, "report_sha256": "SHA256_OF_REVIEWED_REPORT_BYTES"}
  ]
}
```

Required cases: `fresh_install`, `owner_login`, `project_git_persistence`,
`app_build_deploy`, `platform_app_upgrade`, `running_app_reboot`,
`stopped_app_reboot`, `failed_upgrade_recovery`, `interrupted_upgrade_recovery`,
and `corrupt_packet_refusal`. Keep each original reviewed JSON report in
`REPORT_DIRECTORY/CASE.json`; the evaluator reads these fixed filenames and
checks the index hashes. Reports must cover the same exact candidate, supported
source version and Ubuntu profile. Missing, failed, duplicate, different-candidate
or unavailable reports fail. Do not turn historical base-platform or source-build
success into an app-capable signed-bundle result.

The index is a **trusted operator assertion** about the reports' scope and truth.
The command checks completeness, exact reservation binding and report bytes; it
does not interpret arbitrary historical report formats or cryptographically
attest those assertions. Report hashes detect changed/missing inputs, not forged
claims by their author. Keep reports/index in the reviewed evidence handoff.
An eventual unattended writer needs authenticated evidence producers and must
not accept contributor-supplied JSON as release authority. No general acceptance
adapter or authenticated producer for the remaining app-upgrade tests exists yet.

```sh
python3 scripts/release_promotion.py \
  --reservation release-candidates/REVIEWED_VERSION.json \
  --packet REVIEWED_CANDIDATE.zip --gh /opt/attestation-gh/bin/gh \
  --channel preview --app-sha256 REVIEWED_COMPONENT_SHA256 \
  --acceptance REVIEWED_acceptance.json --reports REPORT_DIRECTORY \
  --canary NEW_canary.json
```

The command cryptographically re-verifies the reserved packet on every invocation.
Its JSON result includes exact reservation, policy/report digests, blockers,
canary metrics and false publication eligibility; failed readiness exits 1.
Missing/unreadable/malformed inputs exit 1 rather than yielding partial success.
The repository's current stop state and missing app-upgrade evidence therefore
cannot pass. The existing retention writer only creates unpublished validation
drafts and is unaffected by this read-only publication-readiness gate.

## Validation and remaining scope

`python3 -m unittest discover -s scripts -p 'test_*.py'` runs the policy/evidence
fault matrix, per-route latency boundaries, exact identity preservation, full
preview-window collector with a simulated clock, live stop while waiting,
failed/TLS/wrong-body probes, process identity changes, stale app configuration,
partial evidence, duplicate keys, nonfinite JSON, unsafe file types and exclusive
output checks. These are deterministic tests; simulated time does not count as a
real one-hour/24-hour canary. Ordinary CI runs them under the existing required
Python checks. No workflow permission or publication activation is added.

[#263](https://github.com/devoption/devoption/issues/263) remains open for coordinated
app/build upgrades and their signed-candidate acceptance, any future schema-changing
migration contract, and reconciliation of the [#21 publication decision](releases.md).
Stable format/policy activation and authenticated unattended evidence ingestion
remain unimplemented. The readiness checks provide stop/measurement rules before
any such publication automation can be enabled.
