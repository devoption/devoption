# Local worktree inspection core

[#107](https://github.com/devoption/devoption/issues/107) adds a read-only Unix
inspector in `devoption_projects::worktree`, before repository registration under
[#16](https://github.com/devoption/devoption/issues/16). The inspector itself neither
selects nor registers a production path. The separate
[#109 registration backend](project-catalog.md#repository-attachment-backend) now
uses this core under an explicit local root option and authenticated endpoint;
no browser field or execution permission is enabled.

## Root authority and containment

A trusted local caller explicitly opens `PermittedRoot::open_absolute`, then
calls `inspect` with one relative worktree directory. The root is an existing,
absolute, normalized path, never `/`, a default home directory, the process's
working directory or a value taken from an untrusted browser request. No root is
auto-discovered. The backend obtains the explicit local root selection independently
of the candidate path, separates it from application data and authorizes the authenticated
owner before calling this core. Future work/artifact directory policy and execution
permissions remain separate prerequisites.

Candidate paths cannot be absolute, empty, contain control characters, repeated
slashes, `.` or `..` components. There is no parent-repository search or directory
listing. Every opened component uses descriptor-relative, no-follow directory
access. Symlinks in root ancestors, candidates or inspected metadata are rejected.
The root and inspected descendants must belong to the current effective OS user
and have no group/other write bits. Read permissions may remain shared, as in
ordinary `0755` directories and `0644` Git metadata. Root ancestors need not be
owned by this user. Inspected files must be regular with exactly one hard link;
nonblocking opens reject FIFOs and other special files without reading them.

All Git directory references must stay beneath that same root. Absolute metadata
paths must have the root as a component prefix. Relative references may begin
with parent steps from an already-checked directory (as linked worktrees require),
but cannot escape the root or contain later `component/..` normalization. No
outside target is opened to canonicalize or inspect it. A linked worktree whose
common Git directory is outside the root is deliberately unsupported. Supporting
several approved roots would require a separately reviewed policy change.

The root descriptor is retained. Its path binding is checked before and after
inspection; worktree and Git-directory identities are also rechecked. Replacing
the selected root or directory does not silently retarget the capability.
The OS account and local filesystem remain trusted. This is not protection from
a hostile same-user process, mount changes, ACL grants or network filesystems.
Inspection does not provide a wall-clock I/O deadline or an atomic filesystem
snapshot; it must be repeated before any later attachment or execution decision.

## Supported metadata

The subset follows Git's [repository layout](https://git-scm.com/docs/gitrepository-layout)
and [linked-worktree layout](https://git-scm.com/docs/git-worktree#_details):

- A normal working directory with a real `.git` directory.
- A linked working directory with a regular `.git` file whose `gitdir:` reference
  points to `<common>/worktrees/<name>`. The admin directory's `commondir` must
  resolve inside the root and its `gitdir` backlink must match that worktree's
  `.git` file. Nested common-directory indirection is rejected.
- A regular per-worktree `HEAD` containing either a branch reference under
  `refs/heads/` or a 40-digit hexadecimal detached ID. An unborn branch is allowed;
  references are not dereferenced and commit existence is not checked.
- Real `objects` and `refs` directories, and a bounded common `config` declaring
  format `0` and `bare = false` exactly once each. Only simple single-line config
  syntax with unquoted values is supported. Includes, extensions, escapes/continuations, `core.worktree`,
  per-worktree config and object alternates are rejected, never followed.

Bare repositories, separate-git-dir gitfiles, submodules, SHA-256/reftable or other
format extensions, legacy symlink HEADs and richer config syntax are deferred.
Unknown simple config values are not interpreted. Hooks, working files, object
contents, ref contents, remotes and config includes are never opened or executed.
The inspector never invokes Git, a shell, a credential helper, hooks, filters or
repository code; it performs no clone, fetch, checkout, build or writes. Ordinary
filesystem reads may update access times.

Each full path/reference is bounded to 4096 UTF-8 bytes and 64 slash-separated
components. Config is limited to 16 KiB; other inspected text files to 4 KiB each.
There are at most five text reads (32 KiB of accepted content in a linked layout),
with a one-byte overflow probe per file. Fixed metadata checks and bounded path
walks replace recursion and enumeration. Missing, unsafe, unsupported, malformed,
oversized or observed-changing metadata returns a fixed error without path/content
disclosure. This recognizes a supported structure, not Git object integrity or
readiness to execute commands against arbitrary repository configuration.

## Identity and tests

The observation contains a normalized absolute display path plus device/inode
identities for the working directory, its Git directory and its common directory.
Linked worktrees have distinct working/admin identities and share the common
identity. Reopening an unchanged location or renaming an ordinary worktree keeps
its filesystem identity. Path spelling/case aliases must be compared by identity,
not display strings. Escape display paths for UI and keep private paths out of
public logs.

These observations are neither persisted `RepositoryId` values nor content hashes.
Inodes may be reused after deletion, filesystems may change, and copies have new
identities. The registration domain retains the catalog's assigned stable IDs and
revalidates the selected path binding. This inspector itself does not persist registrations.

`cargo test -p devoption-projects --locked --offline` uses temporary fixture trees
for ordinary/linked layouts, identities, traversal and symlink rejection, root
replacement, permissions/hard links/special files, metadata limits and read-only
behavior. It does not inspect or register real user repositories.
