# Recovering repository device bindings

This local/SSH workflow addresses a version-2 catalog whose saved device numbers
no longer match the configured repository filesystem. Normal startup continues to
reject that mismatch. It does not disable repository checks or reset the catalog.

## Why recovery requires review

The catalog stores device/inode observations, not a persistent volume identifier.
A changed device number alone does not prove a reboot/remount, and matching paths
and inodes do not prove the same filesystem. Apple exposes a separate volume UUID
attribute where supported ([getattrlist reference](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/getattrlist.2));
legacy records contain no earlier UUID to compare. This change does not invent
that history, change the format or promise automatic recovery across mount cycles.

Recovery is an **explicit new repository trust binding**, not restored execution
authority. The operator must independently verify the intended volume and every
registered repository before approval. Retain the original data and repository
contents. Never edit serialized device IDs manually or delete records to restart.

## Review, then explicitly approve

Stop DevOption and other catalog/repository writers first. As its unprivileged OS
user, run the installed binary:

```sh
devoption recover-repositories --data-dir /absolute/private/data --repository-root /absolute/repositories
```

This mode only reads the private catalog and inspects its registered layouts. It
creates no lock, backup or catalog file and reads no owner/password record. It
prints escaped paths, old/new device/inode observations, a proposed backup name
and a review fingerprint. It runs no Git commands, hooks, builds or repository code.
Keep the output private; it contains filesystem paths.

The review requires the exact saved root path, unchanged root/worktree/Git/common
directory inodes, a consistent one-to-one old/new device mapping, safe current
layouts and disjoint application data. Missing, replaced, symlinked, hard-linked
or unsafe entries fail closed. These checks constrain the operation; they do not
authenticate the volume. Recovery is refused when no root-device change exists.

After verifying the actual mounted volume and repository contents and explicitly
approving the new binding, rerun the same command with the displayed fingerprint:

```sh
devoption recover-repositories --data-dir /absolute/private/data --repository-root /absolute/repositories --approve-device-rebind REVIEW_FINGERPRINT
```

The fingerprint binds the complete old/proposed catalog and selected data-directory
identity. Application holds the normal writer lock, freshly inspects all bindings,
and rejects a stale review before backup. It creates and syncs a private catalog-only
`projects.before-rebind-<fingerprint>.bin` backup, rechecks observations, then
atomically publishes using normal catalog durability checks. Backup identity,
safety and exact contents are checked again before publication.
As with existing storage, this is not isolation from the owning OS user or a
guarantee against noncooperating filesystem writers. A matching complete
backup from an interrupted attempt can be reused; unsafe or different existing
backup contents are never overwritten. A failed write may leave a partial backup;
retain and inspect it rather than deleting or retrying blindly.

Project/repository IDs, names, associations and relative checkout paths remain
unchanged; only reviewed device observations change. Owner/Profile records,
repository contents, launch configuration, credentials and permissions are not
modified. The writer may create the normal private `.projects.lock`; never remove
it to bypass contention. The backup is not a full installation backup.

If publication/sync fails, the result may be uncertain: inspect current catalog and
backup before retrying. Restore/rollback requires a stopped service and a deliberate
offline operation; restoring the old catalog also restores its old device mismatch.
See the [backup guide](private-linux-install.md#restart-backup-and-restore).

After success, start with the original inspection or managed-root configuration.
Verify sign-in, saved project/repository names, Code/status and managed creation
where configured. Recovery does not grant execution approval. Another device
change requires another review; changed paths/inodes need a separately designed
migration rather than this command.
