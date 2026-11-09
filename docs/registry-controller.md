# Owner-only registry controller controls

The opt-in controller adapter connects existing staged Cargo candidates to the
[local publication/access policy](package-publication-policy.md) under
[#385](https://github.com/devoption/devoption/issues/385). It implements owner
review/approval, status, project-scoped index/archive acquisition and explicit
interrupted-decision recovery over authenticated HTTP.

This is a JSON control API over already staged candidates. There is no new web
page, candidate upload, build-to-candidate promotion, Cargo `config.json`, complete
sparse/Git registry index or build dependency acquisition yet. A configured index
URL is an immutable namespace identity; enabling these controls does not serve
that URL or make the platform a Cargo-compatible registry. Keep #385 and #383 open
until verified materialization and actual platform-consumer builds are integrated.

## Explicit startup and private storage

Without `--package-registry-dir`, no registry root is read or opened and the
controls return unavailable to an authenticated owner. Existing stateless hosting,
project/build/deployment options and storage formats are preserved. The installer
does not create or enable a registry root. Startup requires Unix and an existing
configured owner; it never creates an account or credentials.

Select an existing absolute private root with no symlink, empty or parent path
components. Its fixed layout is:

```text
registry-root/              current service OS user, mode 0700
  config.toml              same user, regular 0600 file, one hard link
  candidates/              same user, mode 0700; native candidate-store records
  policy/                  same user, mode 0700; native policy-ledger records
```

The value-free bounded configuration contains only:

```toml
schema = 1
registry = "team"
index = "sparse+https://packages.example.test/index/"
```

`config.toml` is at most 4096 bytes; unknown fields, unsupported schemas and invalid
registry/index bindings are refused. No credential, caller role, download URL or
caller-selected filesystem path comes from an HTTP request. The root may contain
only these three entries. Native stores may create their private lock files;
startup creates no directories, fixes permissions, migrates records, stages source,
publishes candidates or repairs a pending decision.

The root must be disjoint by actual filesystem ancestry from owner data,
repositories, history and any selected runner work/vendor/toolchain roots. Keep it
outside untrusted processes and build inputs. Startup validates this before
stopping a prior build for the combined runner profile. The source/candidate
formats and [native storage safety rules](cargo-package-storage.md) remain
unchanged. All retained policy bindings, including a complete pending decision,
must match the one configured registry identity. Different registries need
separate roots; this adapter exposes one selected root per controller.

For example, after an operator has separately prepared the private root and
verified candidates using the native APIs:

```sh
devoption serve --data-dir /absolute/owner-data --package-registry-dir /absolute/registry-root
```

This combines with existing repository/history, project-secret, runner and
app-deployment options when those capabilities are independently configured. It
adds no execution, provider or publication authority to a candidate. The current
candidate producers are the trusted plan-only generator and the
[successful retained-build adapter](cargo-package-generation.md#successful-retained-build-candidates),
followed by `PackageStore::stage`. These native APIs do not ingest successful
library builds/downloads through this controller or enable automatic staging.

Root/config bytes and retained directory identities are rechecked on each
admitted operation. Changed configuration, replaced roots/children, unsafe files
or unavailable native records fail closed. Stop the controller before operator
configuration/storage changes and restart with verified inputs; no hot reload or
implicit rebinding is supported. Normal startup refuses partial/corrupt policy
state. A complete valid pending decision can start in recovery-only mode, with
index/archive/status unavailable until a fresh owner recovery approval completes.

## Authentication and admission

All endpoints live inside the existing [owner session and origin
boundary](using-devoption.md). Reads require the live host-only browser cookie;
mutations require matching Host/Origin, that cookie and its current CSRF header.
Only the configured owner identity supplied by the server session can use these
controls. A signed-in member, forged owner/role header, supplied actor ID or guessed
review nonce grants no access. There are no registry bearer credentials, anonymous
reads, CORS permissions or credentials injected into a source/build environment.

One controller operation runs at a time; contention returns 409 rather than
queuing work with stale authority. Mutation bodies have the existing five-second
read bound and an 8192-byte limit. Use `Content-Type: text/plain` (or the existing
`text/plain;charset=UTF-8` form); structured request/response values are JSON.
Other media types/encodings are refused. Session/CSRF is rechecked after body
receipt and lock acquisition, immediately before filesystem admission. A logout
or rotation during body delivery cannot authorize the operation. An already
admitted operation may finish after a later logout; cached/acquired bytes cannot
be retracted.

The controller holds its registry mutex, catalog gate and the cross-process
catalog writer lease through native approval/acquisition. That serializes current
project observations with cooperating HTTP and CLI catalog writers. Filesystem
work runs in a blocking worker, retaining those guards even if HTTP disconnects.
The native byte/count limits apply, but no OS-filesystem latency guarantee is
claimed. The OS owner/local filesystem remain trusted; this is not protection
against an administrator replacing or rolling back all private state.

## Endpoint contract

`P` is a saved lowercase 32-hex project ID. `N`/`V` are exact supported Cargo
coordinates; the registry ID/index come solely from startup configuration. Use
the source project for status and all normal mutations. For acquisition, use the
actual consuming project: even an owner session cannot bypass its explicit grant.

| Method/path | Input/result |
| --- | --- |
| `GET /projects/P/registry/N/V/status` | Owner metadata: exact coordinates, source revision/digests, archive checksum/size, readers, yank flag and event budget. |
| `POST /projects/P/registry/N/V/prepare` | JSON action below; returns bounded review metadata and a fresh review nonce, without publication/access changes. |
| `POST /projects/P/registry/N/V/approve` | Exactly the returned nonce as the plain body; consumes the matching prepared review once. |
| `GET /projects/P/registry/N/V/index` | One authorized JSON index line with current yank flag; not an aggregated Cargo index service. |
| `GET /projects/P/registry/N/V/archive/CHECKSUM` | Exact-checksum authorized `.crate` bytes as an attachment. |
| `POST /package-registry/recovery/prepare` | Empty plain body; fresh review of the complete pending decision in recovery-only mode. |
| `POST /package-registry/recovery/approve` | Exactly that recovery nonce; completes only the reviewed original decision. |

Supported normal preparation bodies are:

```json
{"action":"publish","readers":["0123456789abcdef0123456789abcdef"]}
```

```json
{"action":"readers","readers":[]}
```

```json
{"action":"yank","yanked":true}
```

Fields are strict. Reader IDs must be real current projects and distinct; the
source project always retains access. Empty requested readers therefore means
source-only access. An unyank uses `false`. Every version needs separate explicit
publication approval. Native immutable version, namespace ownership, checksum,
revocation and yank rules apply; see the [authoritative policy](package-publication-policy.md).

Preparation reports the exact source project/repository/revision, registry/index,
archive checksum/size, full candidate/source fingerprints, proposed full action,
sequence and remaining event budget. Recovery also reports the observed final-link
state. Private source contents, owner-storage paths and authentication values are
omitted. Inspect this concrete response before submitting approval.

One pending review is retained per controller. A new admitted preparation replaces
it. Nonces have 128 bits of fresh entropy and a five-minute lifetime; they are
review references, not session credentials. The native review also enforces
expiry/context equality. Approvals bind the exact source route and coordinates;
a mismatched/replayed nonce or route consumes/refuses the pending action. Restart
forgets all nonces. Native policy decisions persist. Recovery has a distinct nonce
path/type and cannot be used as ordinary publication approval.

Responses use `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`;
archive responses use a fixed attachment filename. Errors omit underlying paths,
source text and storage details. Unknown/unauthorized acquisitions both return
404; malformed bodies return 400, stale/conflicting/busy actions 409, and
unavailable/capacity/storage states 503. Existing guard failures retain their
normal status, including unauthenticated read redirects and mutation rejection.
No error is interpreted as proof that an earlier write did not commit: refresh
status, or explicitly prepare recovery if the service is recovery-only.

## Recovery, validation and remaining work

A failed write drops/reopens the ordinary policy handle only to observe current
state; it does not repair it. While blocked, normal data/status calls return 503.
The separate recovery preparation/approval uses the exact [fresh owner recovery
contract](package-publication-policy.md#fresh-review-of-an-interrupted-decision),
including complete prefix, candidate, current catalog, inode/link and capacity
checks. Partial/corrupt/conflicting evidence is retained for separate inspection.
Do not delete a pending record or reset a ledger to restore service.

Router tests cover complete publication/status/download/yank/revocation flow,
preparation without access, owner/member/forged identity/origin/CSRF refusal,
request bounds, stale/replayed/cross-project reviews, logout during body delivery,
external writer exclusion, restart/nonces, config/permission/ancestry refusal and
both complete recovery boundaries with a pending revocation. A real loopback HTTP
fixture returns the exact approved archive to an owner and refuses anonymity.
CLI process tests additionally exercise actual startup, owner sign-in, default
disablement, opted-in controls and unsafe configuration refusal before listening.
All use synthetic identities/source and disposable private directories. Native
Cargo consumer proofs remain separate; these tests do not run platform consumers.

Remaining #385 work is successful-build candidate production/ingestion, approved
Cargo index aggregation/serving, complete authorized dependency closure/archive/
vendor verification, and service/runner wiring with two actual platform consumers,
version updates and retained older builds. No installer/deployment, global
publication, public DNS, production credentials or access-policy changes were
performed or authorized by implementing these controls.
