# Profile

A signed-in owner can open **Profile** from the account menu or visit
`/settings/profile`. Enter a display name and choose **Save name**. Leave the field
blank and save to return to the **Account** fallback. The menu shows initials from
the first and last word containing letters or numbers; names without them keep the
person icon. Owner remains a role label, separate from the display name.

Names support Unicode, up to 128 UTF-8 bytes. Leading/trailing whitespace, control
characters and explicit bidirectional overrides/isolates are rejected. A character
can use several bytes. The server preserves accepted text; it does not normalize
names or treat them as unique identifiers. Names appear as text, including literal
markup characters. This does not change the password, account identity or permissions.

The form stays disabled until the session and saved name are loaded. A successful
save updates the menu. If a save cannot be confirmed, **Reload saved name** checks
what reached storage before another attempt; an expired or uncertain session
links to **Check account**. Reloading replaces the field with the saved value.
JavaScript is required for editing.

## Storage and HTTP contract

On Unix, `profile.bin` is an optional private record in the same startup-selected
data directory as `owner.bin`, anchored to that directory's open file descriptor.
No record means an unset name. The record contains an eight-byte `DVPROFIL` magic,
a version byte (`1`), the existing 16-byte account ID and the UTF-8 name. It contains
no password or session material. Validation bounds it to 153 bytes and requires a
regular, singly linked file owned by the service OS user with no group/other access.

Saves use a private temporary file, file sync, atomic rename and directory sync.
Requests within the server serialize; simultaneous processes use atomic replacement
with the last completed save winning. A failure after rename may have saved the name,
so clients reload instead of assuming rollback. Invalid existing records fail closed;
startup stops before binding, and runtime access returns unavailable. Records are
never reset automatically. This is not a boundary against the owning OS user.

`GET`/`HEAD /account/profile` requires the current browser session and returns the
name as plain UTF-8 text, with `Cache-Control: no-store`. Unauthenticated reads
redirect to `/sign-in`. `POST /account/profile` requires the existing same-origin,
cookie and CSRF checks, and accepts a single `text/plain;charset=UTF-8` body of at
most 128 bytes. Empty text clears the name. A validated, durable save returns `204`;
invalid names return `400`, oversize bodies `413`, unsupported media `415`, stalled
bodies `408`, and unavailable storage `503`. Server-owned identity extensions bind
access to the existing owner; client identity or role headers grant no authority.

Keep this optional record with the owner and catalog in the
[documented offline backup](private-linux-install.md#restart-backup-and-restore). Tests
cover actual sign-in, CSRF/origin failures, invalid names, save/clear/restart,
unchanged credentials, unsafe/corrupt storage, concurrent publication and browser
state races. No account administration, password reset, uploads or SSO is added.
