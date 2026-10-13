# Create server-managed repositories

Create a project, open **Code**, then choose **New repository**. Enter a name of
1–64 ASCII letters, numbers, hyphens or underscores, starting with a letter or
number. **Create repository** creates an empty Git repository on the server and
opens its file browser. Separate projects can each create a repository named `ui`.
Names are compared without ASCII case when creating within the same project. **View
status** shows branch `main`, no commits yet and a clean working tree. Git is
required for that status read, but creation itself does not run Git or project code.

Creation requires the administrator to select managed storage once. Users do not
need to prepare each repository over SSH. **Attach repository** remains available
as an optional existing-checkout inspection/import path. Projects can still have
multiple repositories, and explicitly attaching the same repository to another
project preserves its identity.

This creates a real empty checkout and persistent association. Source editing,
scaffolding, initial commits, Git push/pull hosting, builds and deployment remain
unimplemented. It extends the registration model from
[#16](https://github.com/devoption/devoption/issues/16); a new empty repository has
no pinned source commit for the future build workflow in
[#17](https://github.com/devoption/devoption/issues/17).

## Select storage explicitly

Use an existing private directory owned by the service OS user, outside and not an
ancestor of its account-data directory. The root and every ancestor must have no
symlink components. The root must have no group/other permission bits. Startup
validates it without creating a directory or changing permissions:

```sh
devoption serve --data-dir /absolute/private/account-data \
  --managed-repository-root /absolute/private/repositories
```

`--managed-repository-root` explicitly authorizes creation of new repositories in
that root. It is mutually exclusive with the existing `--repository-root`, which
continues to permit inspection and attachment only. Neither option has a default.
Changing a deployed service from inspection to managed mode is a deliberate
operator configuration decision; selecting a root in the browser is unsupported.
An existing catalog must still match the selected root's path and filesystem
identity. Use the same root when enabling managed mode for an existing catalog.
Managed mode does not overwrite or modify existing checkouts.

After selecting storage, repository creation is through the authenticated UI/HTTP
service; the CLI supplies startup configuration. There is no separate direct
repository-mutation CLI command. Back up repository contents separately from the
[account/catalog snapshot](private-linux-install.md#restart-backup-and-restore).
Retain the existing root for the catalog's filesystem bindings when restoring;
relocation is not an automatic migration.

## Publication and recovery

The domain validates the project, name, repository count and root binding while
holding the catalog writer lock. It checks the selected project’s existing
repository names, including legacy and attached records, before writing. It creates
a directory named `<32-hex-project-id>--<repository-name>` exclusively, writes
private Git metadata with an unborn `main` branch, syncs files/directories,
re-inspects the checkout and publishes its new random repository ID and project
association using the existing catalog's atomic save. The version 2 catalog format
is unchanged. Creation runs no hooks, templates, subprocesses, network access or
credentials.

New storage keys use the stable project ID, so renaming a project does not move its
repositories. Display names stay unchanged. Existing flat or attached paths,
repository IDs and associations are retained without migration. The catalog format
continues to accept legacy names and paths.

A duplicate name in the selected project, or an existing file, directory or symlink
at the new storage key, returns a conflict and preserves existing data. Other
projects’ names do not cause a conflict. Duplicate submissions create at most one
checkout with that name in that project; they do not return a second identity. New
repository records are limited by the existing 256-record catalog capacity. The
encoded catalog also has a 512 KiB limit; reaching it during publication leaves the
new checkout unregistered and requires inspection before retrying.

The checkout and catalog are separate filesystem publications. A stopped process,
failed write or sync can leave a partial or complete unregistered checkout, or a
saved association whose response was lost. The UI never reports success before both
creation and catalog durability succeed. For an uncertain result, reload Code and
inspect saved state before retrying. There is no automatic retry, rollback, deletion
or completion of an interrupted directory. If a complete checkout is unregistered,
explicitly inspect its project-prefixed storage path and use **Attach repository**
(the recovered display name defaults to that path’s last component); partial layouts
fail attachment validation. Preserve incomplete state for operator inspection rather
than deleting an existing path to make a retry succeed.

The root capability remains anchored to the startup-selected directory. All
creation uses relative descriptor operations and refuses symlinks/existing files;
root changes fail closed. This is not a boundary against the trusted OS owner or
a guarantee for network filesystem durability.

## HTTP and validation

`POST /projects/{id}/repositories/create` accepts one plain UTF-8 name, bounded to
64 bytes with the existing five-second body deadline. The existing current-session
cookie, explicit CSRF header, Host and same-Origin guards run first. The browser
cannot supply a root, command, identity or configuration.

Success is an empty, non-cacheable `204` with the new repository's same-project
`/projects/{project}/repositories/{repository}/files` `Location`. Its empty file
browser opens inside Code, even when other repositories already exist. The browser
validates that destination before navigation. Invalid names return `400`, missing
projects `404`, same-project names/existing storage paths/repository-count limits
`409`, oversized bodies `413`, unsupported media `415`, stalled bodies `408`, and
unavailable managed storage or uncertain filesystem/catalog publication `503`. Error
bodies disclose no absolute paths or private filesystem details.

Tests cover real CLI startup and authenticated HTTP creation/restart, stable
identities and shared associations, same-named repositories in separate projects,
legacy paths, project renames, independent file/status reads, controlled Git status,
CSRF/Origin rejection, input validation, duplicate/concurrent requests, preserved
existing paths, symlinks, private-root checks, depth limits, root replacement,
failed publication, explicit attachment recovery, and browser
pending/error/navigation behavior.
