# Project catalog core

The `devoption-projects` crate implements project/repository identity metadata,
associations and private catalog persistence under
[#101](https://github.com/devoption/devoption/issues/101), the first core slice of
[#16](https://github.com/devoption/devoption/issues/16). Under
[#103](https://github.com/devoption/devoption/issues/103), configured servers read
the catalog for the authenticated Projects list. Under
[#105](https://github.com/devoption/devoption/issues/105), owners can create named
empty projects. Repository path intake, registration and Git invocation remain
unimplemented.

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
reads and creation into HTTP/UI behavior; the CLI remains responsible for dispatch/startup. Existing owner
storage and authentication APIs have not moved or changed.

`Catalog` creates distinct `ProjectId` and `RepositoryId` values using OS random
bytes. IDs are stored, never derived from names, credentials, paths or Git object
IDs. Projects have collections of repository references. A project can have
multiple repositories, and a repository can belong to multiple projects.
Duplicate associations and unknown IDs are rejected. There is no one-repository
project limit or globally unique path rule. Names need not be unique.

Repository records contain an ID and display name only. Creating this metadata
does not validate or register an actual Git checkout. No registration status,
path, remote, credential, owner password or session is stored. This core grants
no application permissions; a future adapter must authorize access using the
authenticated owner before calling it.

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
(`1`). It stores a little-endian `u16` repository count, followed by each 16-byte
repository ID and length-prefixed UTF-8 name. Next is a `u16` project count; each
project has a 16-byte ID, length-prefixed name, `u16` association count and that
many 16-byte repository IDs. Name lengths are also little-endian `u16` values.

The file limit is 512 KiB. Counts and name lengths are bounded before allocation;
decoding rejects truncation, invalid UTF-8/names, duplicate IDs/associations,
dangling references and trailing bytes. Unknown versions fail explicitly. No
migration, reset or repair command is provided.

`cargo test -p devoption-projects --locked --offline` covers multiple repositories
and projects, IDs across process restart, conflicting writers, complete atomic
snapshots, injected partial-write failures, abandoned temporary files, malformed
formats, unsafe paths/files and directory anchoring. The normal README workspace
checks include this crate. CLI HTTP tests additionally cover authenticated creation,
input limits, concurrent requests, read-after-write, restart and storage failures.

Later selected slices must define Git worktree/path validation before intake,
then add repository intake to the owner-authorized HTTP/UI flow. Execution
approval, environment/resource policy and build/artifact directories remain
separate prerequisites under #15/#17; creating catalog metadata authorizes none
of them.
