# Owner-only registry controller controls

The opt-in controller adapter connects existing staged Cargo candidates to the
[local publication/access policy](package-publication-policy.md) under
[#385](https://github.com/devoption/devoption/issues/385). It implements owner
review/approval of retained-build staging and publication, status, project-scoped
index/archive acquisition and explicit interrupted-decision recovery over
authenticated HTTP.

This is a control/acquisition API with JSON reviews and newline-delimited package
index entries. There is no new web page, candidate upload, automatic staging,
Cargo `config.json` or complete Cargo sparse/Git transport. Build dependency
acquisition uses the existing authenticated build review/approval path described
in [verified vendor inputs](cargo-vendor-bundles.md). A configured index
URL is an immutable namespace identity; enabling these controls does not serve
that URL or make the platform a Cargo-compatible registry. The installed runner consumes verified attempt-local inputs; the configured URL
continues to identify the namespace rather than a publicly reachable service.

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
followed by `PackageStore::stage`. With repository, history and execution
capabilities configured, the explicit staging endpoints below can generate and
stage a candidate from a successful retained library build. They do not execute
another build, publish a package or automatically ingest completed attempts.

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
read bound and an 8192-byte limit (32 bytes for build-candidate staging). Use
`Content-Type: text/plain` (or the existing `text/plain;charset=UTF-8` form);
structured request/response values are JSON.
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

## Successful-build candidate staging

Both endpoints require the configured owner, live session/CSRF, private registry
root, registered repository root, history and enabled execution configuration.
Publication controls remain usable without execution; staging returns 503 when
its additional capabilities are absent or the policy is recovery-only.

| Endpoint | Body / result |
| --- | --- |
| `POST /projects/P/builds/attempts/A/registry/prepare` | Empty plain body; returns exact candidate review metadata and a fresh nonce. No candidate write occurs. |
| `POST /projects/P/builds/attempts/A/registry/approve` | Exactly the returned nonce as the plain body; revalidates and stages only that candidate. |

`P` and `A` are lowercase 32-character project/attempt IDs. There are no supplied
archive bytes, registry URLs, names, versions, actor IDs or reader grants. The
selected registry comes exclusively from the private root configuration; package
identity and provenance come from the verified successful build and source.
The review reports `change.action=stage`, attempt, registry/index identity,
package/version, source project/repository/revision, source and source-archive
checksums, Cargo archive checksum/size, index-line checksum and exact receipt
checksum. It never returns source or receipt bytes in the review.

The registered checkout must still be clean at the attempt's exact revision.
The controller performs no reset, checkout, fetch or source reconstruction.
To stage an older attempt, first select its exact clean revision separately;
already-staged versions remain available independently of later checkout changes
and build retention. The [native adapter](cargo-package-generation.md#successful-retained-build-candidates)
refuses unavailable, pruned, unsuccessful, non-library, corrupt or mismatched
retained artifacts. The controller additionally requires the attempt's current
project/repository association. No plan-only fallback occurs.

Staging takes nonblocking execution, history and registry mutexes in that order,
then the catalog gate/writer lease and execution retention lock. An active or
blocked runner or any contention returns 409. These guards remain held through
source capture, retained-artifact validation and candidate storage. Session/CSRF
and root identity/configuration are also rechecked after source/artifact reads,
before saving a review or writing the candidate. Expiry is rechecked after those
reads. Current catalog equality and exact regenerated candidate equality are
required at approval, including the full receipt and attempt provenance.

A staging review shares the one five-minute, single-use pending slot with
publication and recovery reviews. New preparations replace it. An admitted
approval attempt consumes it even when its token, project, attempt or operation
does not match; expired reviews and restart also invalidate it. Rejected request
authentication, transport-body or lock admission does not consume a pending review.
An admitted preparation clears the slot even if its selection/body is invalid. No nonce
is stored on disk. A successful response is `{"staged":true,"already_present":false}`;
exact retry after a new review returns `already_present=true`. Occupied versions
with changed source, build receipt or provenance return 409 without replacement.
A storage failure preserves uncertain evidence for exact native candidate-store
recovery; this endpoint does not discard or automatically recover it.

Private staging grants no reader access and records no publication decision.
Use the separate publication preparation/approval below before acquiring index
or archive bytes through project grants. These controls still do not implement
Cargo index aggregation or runner dependency materialization.

## Complete project index entry

`GET /projects/P/registry-index/PATH` returns one complete authorized package index
entry with `Content-Type: application/x-ndjson`, `Cache-Control: no-store` and
`X-Content-Type-Options: nosniff`. `PATH` is the canonical lowercase Cargo index
path, for example `sh/ar/shared-tool`, `3/a/abc`, `2/ab` or `1/a`. It is a logical
index selector, never a filesystem path. The registry identity comes exclusively
from the configured private root. Names/path prefixes, underscore/hyphen aliases,
uppercase paths, extra components and mismatched registry identities cannot be
substituted. Unknown, noncanonical or wholly unauthorized entries return 404.

The same configured-owner cookie, Host/Origin guards, registry/catalog admission,
fresh catalog writer lease and root validation used by per-version reads apply.
Members remain forbidden, and there is no anonymous or bearer fallback. This route
requires neither execution configuration nor a CSRF header for a guarded read;
recovery-only/unavailable policy or invalid storage returns 503. A complete entry
is assembled before sending a response; no prefix is sent on refusal. It uses the
[native policy's current per-version grants, yanks and bounds](package-publication-policy.md#project-reads-revocation-and-yanks).
That includes a 1 MiB response limit and fail-closed acquisition at the event cap.
Corrupt/unsafe store inventory or a missing included candidate refuses acquisition.

The existing `/projects/P/registry/N/V/index` route still returns exactly one
approved version. This new entry groups only the versions currently visible to
`P`; staged and ungranted versions are absent. Revocation changes future replies
and cannot retract a caller's already acquired bytes. There is no global package
listing, automatic credential creation, `config.json` or upstream fallback.
The configured sparse/Git index URL remains an immutable namespace identity;
this project-cookie route does not make that URL an ordinary Cargo client
endpoint. Complete dependency acquisition and verified offline materialization
remain separate work.

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
| `GET /projects/P/registry-index/PATH` | Complete project-authorized newline-delimited package index entry, with current yank flags. |
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

The disposable Linux library checker additionally exercises real build-to-candidate
HTTP review/staging, retention contention and logout during body delivery, dirty
source and changed catalog/receipt refusal, corrupt archive refusal, replay/restart
invalidation, exact staging retry, separate publication approval/project grant
and exact generated Cargo archive acquisition. It also verifies complete index
entry headers/bytes, current yank state, reader revocation/regrant and restart.
Native offline Cargo proofs use aggregated entries for two consumers, version
updates, yanks and retained older-version builds.

The native [locked-archive adapter](package-publication-policy.md#exact-locked-archive-acquisition)
can now verify and acquire complete admitted source-lock archive sets with hosted
edge/grant checks and fresh-policy revalidation. These HTTP controls do not yet
expose that operation or wire it to the runner. The native [vendor-bundle producer](cargo-vendor-bundles.md) additionally verifies
archive contents and a bounded public/hosted manifest profile and generates exact
vendor files/checksums. Physical publication/reverification and worker use remain
separate work.

Remaining #385 work is Cargo transport/configuration, complete authorized dependency
closure/archive/vendor verification, and service/runner wiring with two actual platform consumers,
version updates and retained older builds. No installer/deployment, global
publication, public DNS, production credentials or production access-policy changes were
performed or authorized by implementing these controls.

## Consumer builds

With the installed Linux runner enabled, register each consumer checkout in its
own project, commit its supported manifest and exact v4 lockfile with the explicit
configured registry name/origin, and grant that project read access through a
separate publication approval. Use the existing build prepare/review/approve
controls. No Cargo credentials or network access are given to the worker.

Every preparation and approval checks the current source, catalog and package
policy; a grant change or unavailable/corrupt archive invalidates a pending review.
The complete verified vendor tree is private to that attempt and is checked again
before execution. A new immutable version requires its own library build, staging
and publication approvals. Update the consumer's committed manifest/lockfile and
prepare a new review to use it. Retained old locks remain rebuildable while their
exact candidates and reader grants remain available; yanks do not erase them.

If the lock includes public packages, pre-provide exact checksum-matching archives
in optional `public-archives/NAME-VERSION.crate`: the directory must be owned by the
controller UID with mode `0700`; each regular, single-linked, nonsymlink archive
must have that owner and mode `0600`. Reads are bounded at 8 MiB per archive and
32 MiB total, before the stricter verified vendor/manifest profile applies. The
controller never creates this optional directory or fetches missing archives.
