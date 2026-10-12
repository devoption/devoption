# Local browser enrollment authorization

The selected product direction is an explicit local setup command that opens the
browser automatically, without copied keys. This document describes the implemented
authorization core under [#81](https://github.com/devoption/devoption/issues/81).
There is no setup command, browser launcher, or production enrollment route yet.

## Authority and trust boundary

The unprivileged OS account running DevOption must control the private owner-data
directory. The existing store validates that ownership and rejects unsafe paths
and records. An explicit trusted local caller may arm `EnrollmentAttempt` against
that store's `OwnerAuthService`. Neither visiting the preview, knowing an audit
identifier, nor receiving an ordinary pre-authentication challenge authorizes it.
Future HTTP handlers must never expose the local arming operation.

The caller receives a cryptographically random 32-byte launch credential. The first
correct claim consumes it and creates an independent cookie/CSRF credential pair.
Only that matching pair can authorize one owner-creation operation against the
same service/store. Browser binding here means possession of both credentials;
it is not hardware/browser attestation. A stolen launch credential can claim the
attempt, and a stolen complete browser pair can submit it. The trusted local
launcher and secure delivery to the intended browser are therefore essential.

The OS account, its trusted launcher, and the browser are inside the trust boundary.
This does not isolate against malicious code running as that account, browser
extensions, or a compromised browser. Loopback binding does not identify an OS
user. Remote/cloud enrollment and privileged installation are outside this design.

## Implemented lifecycle

| Operation | Enforcement |
| --- | --- |
| Local arming | Validate absence of an owner before issuing a launch credential |
| Claim | Consume a matching launch credential once; recheck storage before binding a browser pair |
| Enrollment | Consume the matching cookie/CSRF pair before bounded work; recheck expiry and storage in the worker; use existing atomic create-once publication |
| Deadline | One five-minute monotonic deadline covers both stages, without renewal |
| Concurrency | Clones share one state; only one claim and one enrollment can be admitted |
| Failure | Wrong pairs leave the rightful pair usable; an accepted operation, even if invalid, busy, rate-limited, or cancelled, cannot be retried with its old credentials |

Arming, claiming, and enrollment share the existing one-worker/five-attempts-per-
minute admission limits. A normal successful flow uses three admitted operations.
Wrong credentials are rejected before that admission. Credential comparison is
constant-time for equal-length inputs. Secrets are neither Debug nor Clone and
are never formatted into audit events. There is no secure memory-erasure guarantee.

Expiry is checked when authority is used and again before worker authorization;
it is not a background timer. Work authorized before the deadline may finish after
it. Cancelling an awaiting caller does not cancel its blocking worker or roll back
an owner record. Owner creation remains exclusive even across separate attempts.
On an uncertain storage outcome, inspect the existing record; never reset it.

## Audit evidence

Each attempt exposes a local snapshot containing a public process-local counter,
wall-clock event times, and fixed event kinds for arming, claim, denial/expiry,
enrollment admission, owner creation, and failure. It holds the latest 16 events
and reports how many were omitted. Outcome recording runs inside the enrollment
worker, so caller cancellation does not discard a later completion event.
Storage/worker failures that may follow publication are marked uncertain.

Events contain no credentials, passwords, hashes, paths, or raw requests. They are
bounded in-memory evidence, not durable logs, proof of response delivery, or a
tamper-proof record. A crash, poisoned audit lock, dropped attempt, or event overflow
can limit the evidence. Failed arming returns a fixed error before an attempt is
returned; no persistent initiation event is promised. Durable audit export remains
separate work.

## Browser handoff still required

This core keeps launch credentials in memory and defines no URL or wire protocol.
The future launcher must establish that its intended browser receives the local
authority while keeping credentials out of request URLs/logs, referrers, and
persistent browser storage. Merely opening a public page and accepting its first
visitor does not establish that authority.

A fragment plus `history.replaceState` must not be presented as a guarantee that
the initial secret-bearing URL was never persisted: the API replaces an existing
history entry. That transport still needs explicit review before implementation.
See [MDN's history semantics](https://developer.mozilla.org/en-US/docs/Web/API/History/replaceState)
and [OWASP's session transport guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).

HTTP integration must preserve Host/Origin checks, HttpOnly/Secure/SameSite cookies,
explicit CSRF, bounded password input, no-cache responses, and fixed errors.
Enrollment password strength policy, setup forms in `devoption-ui`, automatic
browser launch, session issuance after setup, and audit persistence are not part
of this core. Existing low-level setup remains an internal integration primitive;
it must not become an unauthenticated HTTP handler.

Run the temporary-fixture checks with:

```sh
cargo test -p devoption-server --lib --locked --offline enrollment
```
