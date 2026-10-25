# Native application identity foundation

`devoption_server::app_identity` is an explicit Unix host-side library for native
application accounts and RBAC. It is **not wired into HTTP, guest imports, the
deployment broker or installed services**. Existing app cookie stripping and
platform-owner authentication remain unchanged. Application login through the
real proxy remains a dependency of [#294](https://github.com/devoption/devoption/issues/294).

## Account and authorization boundary

The host selects one validated application ID and an existing absolute private
directory, then opens `AppIdentityService`. This blocks briefly at startup for a
dummy Argon2 hash. Opening creates only an app-specific lock file; it never creates
an application account, platform owner, OS identity or credential file. Namespace
snapshots and session state do not reference platform owners. Clones share one
service; a second process/service cannot open the same namespace concurrently.

`provision`, `configure` and `replace_password` are **operator APIs**. The embedding
host must independently authorize them. They are not guest capabilities, public
registration, organization-admin operations or browser password-recovery routes.
An API caller that can invoke them is already trusted; possession of an account ID
alone provides no authorization. The first account has no special privileges.

Built-in application roles are `Member` and `Admin`. Both can access the portal;
only `Admin` passes `AccessAdmin`. Organization memberships independently grant
`ReadOrganization`; organization admins additionally pass `ManageOrganization`
for that organization. Neither role implies another organization's permissions.
An application administrator also needs explicit membership for these organization
permissions. Unknown/unassigned permissions fail closed. Further domain permissions
must be added explicitly rather than treating a successful login as authorization.

Use `authorize_read` for a protected safe read and `authorize_mutation` for an
unsafe operation. Each checks the live session against current account state,
enabled status, credential/authorization revision and required permission. The
returned `Principal` is a point-in-time result: do not cache it for later requests.
Changing roles, memberships, enabled status or passwords revokes existing sessions.
Already-authorized work is not cancelled; reopening the service revokes all sessions.

## Credentials, storage and bounds

Passwords use the existing fixed Argon2id profile with fresh OS-generated 16-byte
salts. Provisioning/replacement requires at least 15 Unicode scalar values and at
most 1024 UTF-8 bytes, without password trimming, truncation or normalization.
Login identifiers are explicitly ASCII email addresses, trimmed and ASCII-folded
to lowercase; this is a local identifier policy, not email deliverability proof.
Unknown, disabled and incorrect credentials all produce the same `None` result.
Unknown valid addresses run a dummy verification at the same cost profile.
Returned errors and credential types do not print passwords/hashes/tokens.
Secure memory erasure is not guaranteed.

Account snapshots contain IDs, normalized login identifiers, password hashes,
roles, enabled status, revisions and organization memberships. They contain no
plaintext passwords or session/CSRF credentials. Snapshots use versioned, fixed
shape JSON arrays; unknown versions, extra fields, duplicate identities/login
identifiers, unsupported hashes/roles and malformed memberships are rejected.

Storage is anchored to a no-symlink directory descriptor owned by the process
user with no group/other mode bits. Namespace files must be private regular files
with one link; snapshots are written to exclusively created mode-0600 temporary
files, synced, atomically renamed and directory-synced. Failed writes latch the
service unavailable until explicit reopen, because publication may have occurred
before the error. Never delete/reset storage in response. This is not a boundary
against the owning OS user, ACLs or manual snapshot editing. Only the service may
write its locked namespace. Interrupted pending files are ignored, not loaded.

Each application has these bounds:

| Resource | Bound |
| --- | --- |
| Accounts | 1,000 |
| Memberships per account | 64, with unique organization IDs |
| Snapshot | 4 MiB |
| Active sessions | 256; full capacity rejects sign-in without evicting sessions |
| Absolute session lifetime | One hour, with no sliding extension |
| Session and separate CSRF credentials | 32 random bytes each |
| Hashing/mutation worker | One, using `spawn_blocking` |
| Well-formed sign-in attempts | 12 per 60 seconds across service clones |

Cancelled callers retain their charged attempts and the worker permit until the
actual work exits. Attempts and sessions reset on process restart; these limits
are a local guard, not distributed/per-IP abuse protection. Expired sessions are
removed. Disable/password/role updates remove that account's sessions. Unknown
or incorrect CSRF credentials cannot authorize a mutation or log out a session.
Synchronous authorization returns `Busy` when a password worker or account write
holds the account lock; it never waits behind hashing on the async host thread.

## Remaining application transport

No HTTP routes, login UI, account enrollment/recovery workflow or production
activation is added. Before wiring this library into an app, implement and test:

- A reviewed app-origin grant and lifecycle that cannot read platform-owner state.
- TLS-only, app-scoped host-only HttpOnly cookies with an explicit SameSite policy,
  and a separate explicitly submitted CSRF credential.
- Exact Host/Origin validation, pre-authentication CSRF protection, bounded request
  parsing and external abuse controls. Successful password verification alone is
  insufficient for a safe browser sign-in endpoint.
- Authenticated sign-in/request/sign-out through the actual proxy and worker,
  including forged-header, cross-app and control-plane credential isolation.
- Application-owned fixed post-login destinations selected from the **verified
  current role**. User-supplied role, return URL or email domain cannot grant an
  administrator destination or permission. A redirect never substitutes for
  authorization at the destination.
- Independently reviewed provisioning and recovery before any real enrollment.

Run focused fixtures with:

```sh
cargo test -p devoption-server --locked --offline app_identity
```

Fixtures use disposable accounts/directories. They prove the library boundary,
not live browser authentication, deployed storage or multi-host SSO.
