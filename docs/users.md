# Shared workspace users

The existing first owner remains the installation administrator. Members share
all projects and the same configured repository, build, secrets and deployment
capabilities. Membership is a trust decision: there is no project-specific access
or read-only role. User management and registration settings belong only to the
owner. Each account has its own display profile.

## Invite someone

Sign in as owner by leaving the email field blank and entering the existing owner
password. Open **Profile → Manage workspace users**, enter the recipient's email
address, and select **Create invitation**. Copy the code shown once and send it
privately with this server's `/join` address. DevOption does not send email.

The recipient opens **Create account or accept an invitation** from Sign in,
provides the invited email, chooses and confirms a unique password of at least
15 characters (up to 1024 UTF-8 bytes, without control characters), and pastes the
code. An accepted invitation activates membership directly. It is valid for 24
hours, including across server restarts, and can be redeemed only once. Expired
or cancelled invitations can be removed from Users before creating a new one.
Codes are credentials: do not put them in URLs, issue reports or screenshots.

Members sign in with their email and password. Email matching is ASCII and
case-insensitive; this preview does not verify mailbox ownership. Possession of
the privately shared invitation authorizes its redemption. The original owner
continues using password-only sign-in and is never converted into a member.

## Open registration and approve applicants

Registration starts closed. On **Users**, choose **Open — owner approval required**
and **Save registration**. People can then apply at `/join` without an invitation.
Applications create pending accounts; pending applicants receive no session and
cannot access projects. Review the address through your own trusted communication
channel, then select **Approve** or **Reject**. Closing registration stops new
applications; it does not remove existing applications or approved members.

Submissions receive generic acknowledgement, including duplicate addresses,
closed registration and invalid/expired invitations. This avoids revealing which
addresses already have accounts. Acknowledgement alone does not prove membership;
the applicant can sign in only when active.

Select **Disable** to end a member's session and block subsequent sign-in. You can
then **Remove** the inactive entry. Disabled accounts are not reactivated by
approval; remove the entry and issue a fresh invitation if access is needed again.
The owner is not a removable entry and members cannot promote themselves or
change registration. The store accepts at most 128 entries across invitations,
applications, active and disabled members. Reject/remove inactive entries to free
space. There is no password reset, account promotion, mail service, per-user audit
trail, or hosted-application authentication integration in this change.

## Storage and request boundary

`owner.bin` and its identity/password remain unchanged. `users.json` stores a
versioned, bounded account snapshot next to it, with private permissions and atomic
replacement. It contains independent random identities and salted Argon2id password
hashes. Invitation codes are stored only as salted Argon2id hashes. Keep this file
and member `profile-<identity>.bin` files with the existing data-directory backups.
A retained advisory directory lock excludes another account writer/server for the same data directory;
unsafe, corrupt or unsupported account storage prevents startup. No file permission
repair, account migration or externally delivered credential is performed at startup.

The existing Host/Origin, cookie/CSRF, no-store, body-size and read-timeout boundaries
also protect account routes. `POST /auth/user-sign-in` and `/auth/register` consume
a fresh `/auth/challenge` pair before reading a bounded JSON object carried as
`text/plain;charset=UTF-8`. These share the owner's one-worker/five-attempts-per-minute
budget. Registration never issues a session. `GET /auth/registration` reveals only
the open/closed setting. `GET /auth/account` reveals the current account's role.
Owner-only `GET /auth/users` lists addresses/statuses; CSRF-protected `POST /auth/users`
handles invitations, registration, approval, disabling and removal. `GET /settings/users`
is independently owner-checked. Roles are determined from the persisted owner ID,
never browser-provided claims. No CORS or bearer-token alternative is enabled.

One session per account is retained in memory for at most one hour; signing in
again rotates only that account's session. Logout affects only its matching token
pair. Disabling and member session issuance are serialized together so a stale
login cannot reactivate access after disabling. Revocation affects subsequent
requests, not work already authorized. Restart loses all sessions but preserves
accounts, settings, approvals and unexpired invitations. Existing build/deployment
approval records still use installation authority; they are not a per-member audit
trail. Linux workload capabilities remain separately installed and configured.

Tests cover invitation expiry/replay, persistence, pending/disabled denial,
owner-only management, member project/profile access, independent sessions,
CSRF/origin rejection, storage limits, unsafe files and writer exclusion. Local
browser checks use disposable fixture data, not real invitations or applications.
