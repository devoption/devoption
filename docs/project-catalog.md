# Project catalog core

The `devoption-projects` crate implements project/repository identity metadata,
associations and private catalog persistence under
[#101](https://github.com/devoption/devoption/issues/101), the first core slice of
[#16](https://github.com/devoption/devoption/issues/16). Under
[#103](https://github.com/devoption/devoption/issues/103), configured servers read
the catalog for the authenticated Projects list. Under
[#105](https://github.com/devoption/devoption/issues/105), owners can create named
empty projects. A separate [root-bounded worktree inspector](worktree-validation.md)
is available under #107. Under [#109](https://github.com/devoption/devoption/issues/109),
explicit trusted-root configuration enables the protected repository attachment
backend described below. The browser form and Git invocation remain unimplemented.

## Authenticated service view and creation

With an existing owner, startup calls the domain crate's `read_catalog` before
binding the listener. This reader applies the same bounded format and private
directory/file checks as the store, without creating files or acquiring a writer
lock. Only an absent catalog becomes an empty list. Invalid, unsupported,
oversized or unsafe state stops startup without reset or error-data disclosure.

The server reads the current validated snapshot on every Projects GET/HEAD, which
passes the existing session-cookie guard; anonymous requests receive the empty
non-cacheable redirect to Sign in. Names are HTML-escaped by `devoption-ui`, and
only names and repository counts appear. No server restart is needed after a save.
Read failures return 503 without stale rows or private error details. The
unconfigured preview never loads catalog metadata and retains its public placeholder. No fixture projects
are added to real storage or the persistent preview.

The form sends `POST /projects` with one plain UTF-8 name in the body and
`Content-Type: text/plain;charset=UTF-8` (`text/plain` also works). The body is
limited to 128 bytes and a five-second read deadline. Existing cookie, explicit
CSRF-header and same-Origin checks run first; Authorization headers are rejected.
The domain validates a nonempty, trimmed name without control characters and the
64-project capacity. Names may repeat; they are never paths or identities.

Catalog reads and writes are serialized within each server. Blocking filesystem
work runs outside the async event loop. Each creation acquires the existing
exclusive writer lock, loads the latest snapshot, creates an empty project with a
fresh ID and atomically saves it. Repository records and associations are retained.
Another process holding the lock causes a closed failure, not a stale overwrite.
The handler returns an empty, non-cacheable 204 only after save and directory sync
succeed. The browser then reloads `/projects`; it does not insert an optimistic row.
Invalid names return 400, oversized bodies 413, unsupported encodings 415, stalled
bodies 408 and a full catalog 409. Storage failures or contention return 503 with
no input/path/error disclosure. There is no automatic retry. A connection loss or
post-rename sync error may leave a saved project: reload and inspect before trying
again. Creation is not idempotent; intentionally repeating a name creates another
project.

## Domain boundary

The crate owns this cohesive domain independently of CLI, HTTP, rendering and
authentication. It depends on existing pinned randomness and filesystem
libraries, not `devoption-server` or `devoption-ui`. The server adapts catalog
reads, creation and attachment into HTTP/UI behavior; the CLI remains responsible
for dispatch/startup. Existing owner storage and authentication APIs have not moved
or changed.

`Catalog` creates distinct `ProjectId` and `RepositoryId` values using OS random
bytes. IDs are stored, never derived from names, credentials, paths or Git object
IDs. Projects have collections of repository references. A project can have
multiple repositories, and a repository can belong to multiple projects.
Duplicate associations and unknown IDs are rejected. There is no one-repository
project limit or globally unique path rule. Names need not be unique.

Repository records contain an ID and display name. Creating this metadata alone
does not validate or register an actual Git checkout. Validated registrations add
a separate common-directory binding and per-association default checkout. No
remote, credential, owner password or session is stored. This core grants no
application permissions; the HTTP adapter authorizes the authenticated owner
before calling it.

The current resource limits are 64 projects, 256 repository records and 128 UTF-8
bytes per name. A project may reference any subset of those repositories. Names
must be nonempty, trimmed and contain no control characters. Other characters
are retained as data: future renderers must escape them. Names never become
filenames. These bounds limit memory and storage, not the product's relationship
model.

## Private storage contract

`CatalogStore::open_absolute` accepts an explicit existing application data
directory on a local Unix filesystem. It rejects relative paths, parent
traversal and symlinks in every directory component. The final directory must be
owned by the current OS user and have no group/other permission bits. It creates
no directory and repairs no existing permissions. Ancestors need not be private;
operations remain anchored to the opened directory descriptor after a rename.
Selected checkouts must remain separate from application-owned data.

Opening creates `.projects.lock` with mode `0600` subject to the caller's umask.
The store holds an exclusive, nonblocking advisory lock for its lifetime. Another
cooperating store in this or another process returns `Busy`. Dropping the handle
or exiting the process releases the lock. The lock file remains: never remove or
rename it while the catalog is in use. Share one handle with serialized
read/modify/save operations; stale snapshots from separate operations on that
same handle are the caller's responsibility.

`projects.bin` and the lock must be regular files, owned by this OS user, with no
group/other permissions and exactly one hard link. A missing catalog returns
`None`; a corrupt, oversized, unsafe or unsupported catalog returns an error.
Opening and saving both validate existing catalog state. Neither treats errors
as an empty catalog or overwrites a damaged file to recover.

Saving validates the complete model, writes an exclusively created private
`.projects-*.tmp` file, syncs its contents, atomically renames it over the catalog,
then syncs the containing directory. Readers see the previous or new complete
snapshot. Failure before publication preserves previous state. If directory
sync fails after rename, the new snapshot may be visible with uncertain
durability: reload and inspect it; never reset or delete it automatically.
Ordinary failures clean up the temporary file. A terminated process can leave a
private temporary file; it is never loaded or automatically deleted on reopen.
Existing unrelated files such as `owner.bin` are untouched.

The OS account and local filesystem are trusted. Advisory locking does not
protect against that account bypassing the API or changing lock files. Network
filesystem locking/durability, ACL policy and isolation from the OS owner are
not implemented. No network, VM or deployment configuration is involved.

## Version and validation

The binary format begins with the eight bytes `DVPROJS\0`, then a one-byte version
(`1` for metadata only, `2` for registered bindings). Both versions start with a
little-endian `u16` repository count, followed by each 16-byte repository ID and length-prefixed UTF-8 name. Next is a `u16` project count; each
project has a 16-byte ID, length-prefixed name, `u16` association count and that
many 16-byte repository IDs. Name lengths are also little-endian `u16` values.

Version 2 appends a `u16`-length UTF-8 root path and its device/inode pair, then a
`u16` binding count (at most 256). Each binding contains a 16-byte repository ID
and common-directory device/inode pair. A `u16` checkout count follows (at most
64 × 256); each checkout contains project and repository IDs, a `u16`-length
relative UTF-8 path, working-directory identity and Git-directory identity. Each
identity is two little-endian `u64` values. Paths are bounded to 4096 bytes and
64 components. Validation also rejects duplicate/conflicting bindings, missing
defaults, unbound registrations and references that do not match associations.

The file limit is 512 KiB. Counts and name lengths are bounded before allocation;
decoding rejects truncation, invalid UTF-8/names, duplicate IDs/associations,
dangling references and trailing bytes. Unknown versions fail explicitly. No
reset, downgrade, reattachment or repair command is provided. Reads and startup
never rewrite version 1. Metadata-only saves keep version 1; the first successful
attachment publishes version 2 through the same lock, temporary-file, sync and
atomic-rename path. All prior IDs, names and associations survive; legacy metadata
is never silently matched by name to a physical repository. Once migrated, saves
retain version 2. Older binaries reject it: rollback needs a compatible binary
or an explicit pre-migration backup restoration, which loses subsequent changes.
Back up the private catalog as well as owner data; never delete it to bypass an
unsupported version.

`cargo test -p devoption-projects --locked --offline` covers multiple repositories
and projects, IDs across process restart, conflicting writers, complete atomic
snapshots, injected partial-write failures, abandoned temporary files, malformed
formats, unsafe paths/files and directory anchoring. The normal README workspace
checks include this crate. CLI HTTP tests additionally cover authenticated creation,
input limits, concurrent requests, read-after-write, restart and storage failures.

Tests for the worktree inspector and registration backend use temporary fixture
trees only. The browser attachment form and path/status presentation remain a later
slice. Execution
approval, environment/resource policy and build/artifact directories remain
separate prerequisites under #15/#17; creating catalog metadata authorizes none
of them.

## Repository attachment backend

This Unix-only option is explicit local administrator configuration:

```text
devoption serve --data-dir ABSOLUTE_PRIVATE_DATA_PATH --repository-root ABSOLUTE_TRUSTED_ROOT
```

There is no default root, environment-variable fallback, discovery, browser root
setter or filesystem picker. An existing owner is required. Before listening,
the server opens a root capability, enforces the [inspection policy](worktree-validation.md),
checks actual directory ancestry so root and data cannot contain one another,
and compares the configured path and filesystem identity with any saved root.
Startup creates no catalog or lock. Changing a saved root fails closed. Omit the
option to disable attachments while retaining ordinary project reads/creation;
saved paths are not inspected merely to list projects. This change configures no
production root and registers no real repository; tests create temporary fixtures.

`POST /projects/{project_id}/repositories` accepts one relative UTF-8 path as a
plain body (`text/plain` or `text/plain;charset=UTF-8`), limited to 4096 bytes and
a five-second upload deadline. The project ID is exactly 32 hexadecimal digits.
Existing cookie, CSRF, Host and Origin guards run first; any Authorization header
is rejected. There is no browser form or project-ID discovery UI yet. The endpoint
returns 204 only after a durable save, with `Cache-Control: no-store`. Failures
use fixed messages: 400 invalid ID/path or rejected inspection, 404 unknown
project, 409 duplicate association, stale/root conflict, observed filesystem change
or capacity, 413 oversized body, 415 unsupported encoding, 408 stalled body, and
503 disabled attachment, busy storage or persistence failure. Messages never echo paths or stored data.
Inspect saved state after an uncertain result; there is no automatic retry.

Under the writer lock, the domain loads the latest catalog, validates the selected
checkout with bounded reads, reuses the repository ID for its common Git directory,
adds the association/default checkout, re-inspects that checkout and atomically
saves. If the second inspection fails or differs, attachment returns a stale-checkout
conflict (409) without publishing the prepared catalog. Each new repository's display
name is the selected directory's final component and must satisfy the existing 128-byte name rule. Multiple repositories
can belong to one project; multiple projects can share a repository, selecting
different linked worktrees as their defaults. Associating another worktree of the
same repository with the same project returns a duplicate conflict and never
replaces its default. Metadata-only `associate` cannot bypass checkout validation
for a registered repository. No Git process, hook, fetch, clone, checkout, build
or execution approval is involved.

The saved root and common-directory device/inode observations deduplicate linked
worktrees while filesystem objects exist. They do not replace the random durable
repository ID or prove content integrity. Reusing a saved path, working directory
or Git directory with conflicting observations fails closed. Registration checks
the selected checkout, not every other stored path. Deleted objects, inode reuse,
filesystem replacement and trusted same-user races remain limitations; there is
no implicit rebind or relocation. A future execution adapter must revalidate its
selected checkout and obtain separate execution/resource authorization. Root and
checkout metadata are private catalog data, not public logs.

One default checkout per project/repository association does not constrain future
parallel agents/jobs to that checkout. Separate job worktrees and project releases
that pin an exact commit for every repository are the agreed future model; neither
jobs nor release manifests are implemented here.
