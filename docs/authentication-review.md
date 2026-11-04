# Authentication review decisions

This resolves the four deferred review decisions in
[#99](https://github.com/devoption/devoption/issues/99) against the current
implementation. Retain the existing controls and public APIs; no runtime change
or new hardening dependency is selected. These decisions do not establish
production security readiness or replace the release review in
[#21](https://github.com/devoption/devoption/issues/21).

## Shared authentication admission

Retain one authentication worker, five admitted password operations per rolling
minute, and at most 16 pending pre-session challenges with five-minute lifetimes.
The limits bound expensive work and challenge storage, but callers share
availability: one caller can occupy the worker, spend the password budget or fill
the challenge table, delaying legitimate sign-in. Loopback IP buckets cannot
identify distinct local callers; forwarded addresses are not an authenticated
client identity. Public proxy mode does not remove this tradeoff.

The [authentication service](../crates/devoption-server/src/owner_auth/service.rs)
shares state across clones. Busy requests fail immediately without queueing or
spending an attempt; admitted successes and failures spend the same budget.
Cancellation of the async caller does not stop blocking password work or release
its worker slot early. Limits are in memory and restart resets the budget.
Owner sign-in, member sign-in and registration share it. Account reads and
management use the same worker without spending that password budget, so they
can also encounter or cause a busy response. Delegated MCP account checks use
that same serialized account path.

The [challenge store](../crates/devoption-server/src/preauth_csrf.rs) refuses full
capacity without evicting live entries. Consumption or expiry frees capacity.
HTTP responses distinguish exhausted password attempts (429) from busy work or
full challenge storage (503), with bounded retry hints; a retry hint reserves no
capacity. Host/Origin and challenge validation precede password-body processing.

Revisit admission in a separate issue if multi-caller availability requirements
change. Such a proposal needs a trustworthy caller boundary, bounds on any new
bucket state and expensive work, and starvation/cancellation tests. Increasing
limits or splitting buckets alone is not a fairness proof.

## Route methods and authorization

Retain GET/HEAD/OPTIONS/TRACE as the shared guard's safe-method classification.
This classification grants neither access to protected reads nor permission to
mutate domain data. Configured page reads authenticate their session separately;
OPTIONS/TRACE have no mutation handlers. Explicit POST routes perform domain and
authentication mutations. Unsupported methods and route misses must not bypass
the applicable guard.

The [execution trust contract](execution-trust.md#existing-http-boundary-and-method-policy)
remains authoritative for browser mutations: matching Host/Origin, session cookie
and explicit CSRF token, followed by handler-specific authorization and input
bounds. Pre-authentication routes use their own single-use challenge.
[Account routes](../crates/devoption-server/src/server/auth/accounts.rs) enforce
their own read/mutation and owner checks because they are mounted with the
authentication router. The separately mounted [MCP transport](mcp.md) has its own
origin, OAuth and read-only tool policy; it does not inherit browser mutation
authority. Review these adapters when adding routes rather than assuming the
common page guard covers every router.

Reassess methods and guards for each new handler. Rejection tests must show that
unauthorized requests cannot reach its mutation; safe-method requests must not
enqueue builds, promote releases, change accounts or create an owner. Ordinary
request observations and audit recording do not grant domain mutation authority.
No method-policy rewrite is selected by this review.

## Rust API and owner-store paths

Retain the public authentication, session and owner-storage modules. Public Rust
items are library entry points used by the CLI and integration fixtures, not HTTP
routes. Visibility alone cannot establish remote reachability or authorization.
The [local/SSH setup flow](local-enrollment.md) remains selected; the tested
browser enrollment core remains unmounted, and is not removed by this review.

Production [server startup](../crates/devoption-server/src/server.rs) and
[local setup](../crates/devoption-server/src/owner_auth/local_setup.rs) use
`OwnerStore::open_absolute`. It rejects relative/parent-traversing paths and opens
each directory component without following symlinks, retaining a directory
descriptor. It neither creates directories nor repairs permissions.
`OwnerStore::open` remains a lower-level API used by fixtures: it rejects a final
symlink and checks the final directory, but does not provide the same
component-by-component ancestor protection. Do not substitute it for the
production opener or describe the two as equivalent.

A future API restriction needs a scoped caller/compatibility review, including
CLI and integration consumers. A future production caller must explicitly select
and test its path boundary. No blanket export restriction, crate split or
enrollment removal is selected here.

## Secret-buffer lifetime

Retain the documented absence of a secure memory-erasure guarantee. Passwords
and tokens are intentionally excluded from formatted errors and routine logs,
but that does not erase their allocations. The
[CLI terminal reader](../crates/devoption-cli/src/setup.rs) builds password bytes
and returns owned strings for password and confirmation. The
[HTTP password extractor](../crates/devoption-server/src/password_form.rs) holds
an encoded body and decodes fields into owned allocations; account JSON parsing
also produces owned strings. The blocking authentication closure owns its
password until the work ends, even after caller cancellation. Session arrays,
encoded cookie/header values and framework/browser allocations add other copies.

Wrapping one string on drop would not cover those copies, earlier reallocations,
cancelled/error paths, registers, allocator behavior, or framework/browser memory.
Existing best-effort `zeroize` use for [project secrets](project-secrets.md)
does not establish password/session erasure. Do not extend that claim to these
authentication paths. No additional dependency or buffer rewrite is selected.
If best-effort erasure is proposed later, first inventory copies and ownership
through parsing, hashing, cancellation and response construction, define its
limited threat model, and test cleanup paths without claiming universal erasure.

## Evidence and closure boundary

Existing checks exercise the retained behavior:

| Decision | Regression evidence |
| --- | --- |
| Shared admission | [Service tests](../crates/devoption-server/src/owner_auth/service/tests.rs): exact rolling expiry, shared budget, busy rejection, cancellation, panic and poisoned-state handling |
| Pre-session capacity | [Challenge tests](../crates/devoption-server/src/preauth_csrf/tests.rs) and [HTTP pre-auth tests](../crates/devoption-server/src/server/session_tests/preauth.rs): expiry, capacity/retry, consumption and Host/Origin/method rejection |
| Method/transport boundary | [Session guard tests](../crates/devoption-server/src/server/session_tests.rs), [page tests](../crates/devoption-server/src/server/session_tests/pages.rs), [account tests](../crates/devoption-server/src/server/session_tests/accounts.rs) and [MCP tests](../crates/devoption-server/src/server/mcp/tests.rs): adapter-specific authentication and authorization |
| Path/API boundary | [Owner-store tests](../crates/devoption-server/src/owner_store/tests.rs) and [local setup tests](../crates/devoption-server/src/owner_auth/local_setup.rs): ancestor symlinks/traversal, descriptor anchoring, private storage and create-once behavior |
| Secret lifetime | Source ownership review above; functional tests do not prove erasure |

Run the [development checks](development.md#checks) and record actual results in
the PR. These decisions complete #99's review tracking when reviewed and merged;
they do not claim that shared-capacity denial of service or memory remanence has
been eliminated. Future policy/API/erasure changes require their own scoped
issues. CLI-first enrollment remains selected, the obsolete bearer decoder stays
removed, appearance has its own tracking, and no toolchain workflow change is
needed under this issue.
