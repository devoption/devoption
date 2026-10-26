# Project environment secrets

The owner can manage encrypted secrets at **Project → Settings → Manage environment
secrets**, independently for **Development**, **Preview** and **Production**.
This is the management slice of [#326](https://github.com/devoption/devoption/issues/326).
There is no automatic copying, inheritance or fallback between projects or environments.

## Current behavior

An enabled installation supports up to 64 names per project/environment. Names
match `[A-Z_][A-Z0-9_]{0,63}`. Values are nonempty UTF-8, at most 8,192 bytes,
without NUL; spaces are preserved. The browser's masked input supports single-line
values. Empty values are rejected rather than treated as removal.

The page lists **names, revisions and update times**. Stored values cannot be
revealed or downloaded through HTTP. Add saves a new name; it cannot replace an
existing name. **Replace** requires a new value. **Remove** requires a checked
confirmation and removes that value from the current snapshot. Re-enter a value
to restore a removed secret. None of these operations revoke an upstream API key
or change a database password at its provider.

Each write compares the displayed revision with current storage. Stale replacements
and removals fail with 409. Success clears the input and reloads metadata. Failure
or a lost response clears all secret inputs and disables further writes until
reload: the server may have committed even when the browser did not receive a
response. The client never retries a mutation automatically. No values enter URL
parameters, HTML, browser storage, audit entries or error messages. JavaScript is
required for mutations; navigation and metadata reads use ordinary links.

**Saving does not itself grant runtime access.** An operator can bind an exact
project/environment/name/revision to a [scoped HTTPS integration](app-https.md),
then the owner separately reviews and approves its deployment. Values stay in the
host; the guest receives no raw-secret read API, environment variables or build
injection. Environment labels do not create separate running environments.

## Explicit operator enablement

Secrets remain disabled by default. An operator supplies an **existing, approved,
cryptographically random 32-byte binary installation key** in a protected file.
Startup creates neither a key nor a credential. Do not use an API key, owner
password, textual hex string, sample/test key or environment variable as this key.
Key provisioning and live installation changes require their own authorization.

The key must have a canonical absolute path without symlink components or parent
traversal, be a regular file with one link, be owned by the serving OS account,
and have mode 0600 or 0400. Keep its parent protected. It must be outside the owner
data directory and any configured repository/history roots. Never commit it,
put its contents in command arguments, or place it in an application's checkout.
The serving account must be able to traverse/read the selected path.

After owner enrollment, add this option to the existing reviewed service command:

```sh
devoption serve --data-dir /private/platform-data \
  --project-secrets-key-file /private/operator-keys/project-secrets.key
```

Paths are examples, not provisioning commands. Keep the installation's existing
public-origin, repository, history and optional workload flags when adapting its
command. The key flag does not enable builds/deployments. A missing/unsafe key,
competing writer, or an encrypted record that cannot be authenticated prevents
startup when enabled. HTTP storage errors remain unavailable, never empty-success.
Disabling the flag leaves ciphertext untouched and renders the unconfigured state.
This flag is not added automatically to bundle installations or service generators;
a customized service remains outside the base bundle upgrader's supported contract.

## Storage and trust boundary

The server retains the owner directory capability and a separately opened key
capability. Ciphertext lives in owner storage as
`project-secrets-PROJECT_ID-ENVIRONMENT.bin`; a retained private
`.project-secrets.lock` serializes cooperating processes. Never remove the lock
file from a running installation. In-process operations serialize off the async
event loop, retain their guards through caller disconnect, and load a fresh
project catalog; mutations retain its writer lock too. Unknown projects or
unsupported environments receive 404. Existing owner/session, Host, Origin and
CSRF protections apply; responses are `Cache-Control: no-store`.

Each snapshot uses [RustCrypto `chacha20poly1305`](https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/) **0.11.0** XChaCha20-Poly1305 with a
fresh OS-random 192-bit nonce. The version, owner identity, project ID and environment
are authenticated additional data. Names, values, revision counters and recent
change metadata are encrypted together. Copying a record to another scope, changing
its bytes or supplying a different key fails authentication. Each file is bounded
to 1 MiB, opened without following symlinks, and checked for private ownership,
permissions and a single link. Key material and owned plaintext buffers use
`zeroize`; this is best-effort process-memory hygiene, not protection from a debugger,
swap/core dumps, browser memory, a compromised serving account or root.

Writes use a fresh exclusive private temporary file, file sync, atomic rename and
directory sync. An error after rename is **uncertain**: inspect fresh metadata
before retrying, without resetting the store. Orphaned temporary files are not
loaded as committed state; preserve/inspect them offline before backup. Recent
changes retain the latest 100 successful add/replace/remove events per environment,
including name, action, revision and time under the authenticated owner identity.
They contain no values and are bounded history, not an external tamper-proof log.

The local OS owner is trusted. Encryption protects ciphertext copied without the
key; it does not stop that owner from reading process memory, deleting records or
restoring an older authenticated snapshot. Revisions reject stale browser writes;
they do not provide external rollback detection. Removal is not guaranteed physical
erasure from filesystem snapshots, SSDs or backups.

## Backup, restore and rotation

Stop all writers and preserve the entire owner directory, encrypted records and
owner/catalog identity together. The existing same-node snapshot helper now locks
and excludes `.project-secrets.lock` while copying the encrypted records. Its normal
same-installation limits still apply. It **does not include the external key**.
Keep a separately protected key backup with access limited to authorized operators;
do not distribute the data backup and key together to untrusted storage.

Verify backups and test restore in an isolated private directory using the same
owner/catalog identity and matching key. A new process acquires a new writer lock,
authenticates existing environment records, and can render metadata without showing
values. Never restore over current writes just to recover a forgotten value.
Lost keys make records unrecoverable; replacing the file is not recovery.

Replacing a secret value is supported rotation of the stored value only. Installation
key re-encryption/rotation and secret export are not implemented. Preserve the
original key and ciphertext; do not overwrite a key to attempt rotation. Upstream
credential rotation and any later runtime revision adoption need their own steps.

## Verification

Rust tests cover authenticated round trips, restart readback, ciphertext scope
binding, fresh nonces, stale writes, tampering/wrong keys, unsafe files and key
locations, competing writers, bounds, uncertain publication and retained-directory
identity. HTTP tests cover real guards, project/environment isolation, no-store
responses, metadata-only reads and failed-write preservation. The disposable
browser fixture can enable real encrypted storage with `DEVOPTION_BROWSER_SECRETS=1`;
its fixed key and sample values are test-only and never provisioned on a real host.
