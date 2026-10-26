# Using the current preview

DevOption is intended to be a deployable software factory for individuals and teams.
The current preview supports one owner and a loopback HTTP listener. An explicit
[HTTPS proxy mode](public-linux-install.md) supports one public origin; see the
[fresh cloud acceptance evidence and limits](public-cloud-acceptance.md). Start with the
[quick start](../README.md#quick-start) or the [private Ubuntu installation guide](private-linux-install.md).

On Unix, `serve --data-dir /absolute/private/directory` optionally validates an
existing owner-data directory before listening. It must belong to the current OS
user, have no group/other permission bits, and use a path with no symlink components
or `..`. Invalid owner records stop startup. This option retains the opened
directory but creates no directories or accounts and changes no permissions.
When a valid owner already exists at startup, it enables the HTTP sign-in and
sign-out routes described below. A missing owner leaves them closed. Without this
option, the preview selects no data path.

## Create the first owner locally or over SSH

On Unix, run the following on the server as the unprivileged OS account that will
run DevOption, after preparing an existing private data directory:

```sh
cargo run --locked --offline -- setup --data-dir /absolute/private/directory
cargo run --locked --offline -- serve --data-dir /absolute/private/directory
```

`setup` asks for a password and exact confirmation with terminal echo disabled.
Use a unique passphrase of at least 15 characters, up to 1024 UTF-8 bytes; control
characters are rejected. Spaces and Unicode are preserved without normalization.
This is a minimum-length policy, not a compromised-password check. Passwords are
never accepted through command arguments, environment variables, or redirected
input. For SSH, allocate a terminal (`ssh -t user@server`), then run setup there;
do not include a password in the SSH command. No root privileges are needed.

The command validates storage before prompting, changes no directory permissions,
and never replaces an owner. It persists only the generated identity and salted
Argon2id password hash. Start or restart `serve` with the same directory afterward.
Open `/sign-in`, or **Sign in** from the profile menu, and enter the owner password.
The dedicated authentication page uses this browser's saved theme and has no
workspace navigation. It keeps the password field disabled while checking the
session, reports failed attempts inline, and focuses the field for retry.
After a successful sign-in and confirmed session cookie, DevOption opens Overview
automatically. A direct visit to `/sign-in` while already signed in shows Account
with **Open workspace** and **Sign out**; the workspace profile menu also offers
**Sign out**. Reloading or navigating checks the
current session again; a session lasts at most one hour and ends on server restart
or another sign-in. JavaScript is required. Without an owner, the Sign in page
explains that CLI setup is required. See [setup limits](local-enrollment.md).

For private Ubuntu source installation, see the
[user-service and recovery guide](private-linux-install.md). It includes an
unprivileged systemd example, same-port SSH forwarding, persistent owner storage,
and the scope of [CI and disposable Ubuntu VM validation](linux-rehearsal.md).

For HTTPS administrative access, an optional
[device-certificate gate](public-linux-install.md#optional-protected-administrative-access)
can reject unauthorized connections before sign-in content is served. A permitted
device must still sign in as the owner. This proxy policy does not add app users,
team roles, browser SSO or agent credentials to the server.

## HTTP authentication in the current preview

This local preview binds to `127.0.0.1` and accepts only `127.0.0.1` or `localhost`
at its listening port. Unsafe requests require a matching Origin, the current
session cookie and an explicit matching `X-CSRF-Token` header, apart from the
protected authentication routes described below. The mutation guard rejects
any `Authorization` header, including a browser token presented as Bearer or
mixed with valid cookie/CSRF credentials. There is no API or agent credential
transport; transport-neutral Rust session APIs do not enable one over HTTP.
Missing or rejected mutation credentials receive an empty, non-cacheable 401
without a Bearer challenge. Without a configured owner, all
unsafe requests remain denied. With an owner configured at startup, Overview, Projects, Releases, Infrastructure,
Appearance and Profile GET/HEAD requests require the current session cookie. Missing,
invalid or expired sessions receive a non-cacheable redirect to `/sign-in` without
page content. Sign in stays public; the unconfigured preview stays public. This
startup choice cannot be changed by a request or by removing the owner record from
a running server. Page reads never renew sessions and do not replace the CSRF
requirement on mutations. TLS is not implemented in the backend. Use only the explicit
[public-origin proxy configuration](public-linux-install.md) for HTTPS access;
never expose the HTTP listener itself.

For an existing owner, `POST /auth/challenge` issues an HttpOnly pre-session cookie
and a separate `X-CSRF-Token` header. `POST /auth/sign-in` requires that matching
pair and one URL-encoded `password` field; it consumes the challenge before reading
the form and issues a fresh session cookie and CSRF header only after verification.
`POST /auth/sign-out` requires the current session cookie/header pair.
`POST /auth/session` recovers only a live session's CSRF token using its cookie
and a matching Origin; it never issues, rotates, or extends a session. All four
routes enforce Host/Origin and return empty, non-cacheable responses. Password
failures return 401, invalid pre-session pairs 403, and admission limits 429/503
with bounded retry hints. Storage/internal failures return 503 without details.

Pre-sessions last five minutes with at most 16 pending pairs. Full capacity returns
503 with `Retry-After` rounded up to the earliest expiry (1–300 seconds); consuming
a challenge may free capacity sooner. Each submission needs
a fresh challenge, including retries after a failed password or invalid form.
Sign-in is limited to one worker and five admitted attempts per minute. One active
session is retained in memory for at most an hour; another sign-in replaces it,
and restarting loses it. Tokens must not enter URLs, logs, or browser-local storage.
The browser keeps CSRF tokens only in page memory and clears the password field
on submission. A stale tab's logout is rejected without ending a newer session;
reload that tab to check its current account. Secure-cookie support has been
verified in Chrome on loopback, not every browser. If a session check fails, use
**Check account** to retry recovery without submitting the password again.
Development checks create owners only in [temporary fixture stores](browser-session-check.md).

## Projects and repositories

Create repositories through **New repository** on a project's Code page after an
administrator selects [managed storage](managed-repositories.md). DevOption creates
and registers each empty Git checkout on the server. Existing-checkout attachment
is an optional inspection/import path. Source editing, initial commits and Git hosting remain unavailable. The selected
web app has an opt-in [Linux build workflow](web-app-builds.md) and
[private same-node deployment](web-app-deployments.md).

Projects displays saved names, repository names and counts from the
[catalog](project-catalog.md). Open a project name to see its repositories,
or use **Filter projects by name** to find a case-insensitive name fragment in the
current list. **Clear filter** restores all rows and focuses the search field.
Filtering stays in the page; it sends no request and saves no browser preference.
Without JavaScript, the full list remains visible. Opening a project enters its
own workspace at `/projects/{id}`. **Overview** lists its repositories with links
to their files and guidance for code, builds and releases. Expand **Checkout location** on a repository to see its saved
path, or use **Manage repositories** to open Code.
**Code** opens a folder tree and previews a tracked root README when present.
Source files include line numbers and syntax highlighting; Markdown offers
**Preview** and **Source** views. Repository details and inspection remain below;
**Settings** edits the project
name. **Builds** offers [source preparation checks](build-preparation.md) for an
attached repository: observed branch/commit, Git changes, tracked Cargo inputs
and binary source. **Refresh source** reads current state; no build is run or approved.
When the runner is configured, a clean Git observation offers **Continue to build
review**; that page separately verifies the supported web-app source before
approval. **Releases**
saves successful builds and reviews deployment when the Linux app runtime is explicitly
enabled; see [deployment setup and recovery](web-app-deployments.md). **Packages**
retains its unavailable state.
The same read-only checks are available locally or over SSH through
[`devoption check-source`](build-preparation.md#check-from-a-local-terminal-or-ssh),
with explicit catalog/root paths, saved IDs and documented exit statuses.
The section links use ordinary URLs (`/projects/{id}/code`, for example), so
refresh, bookmarks and browser Back/Forward keep the selected project and section.

Signed-in owners can open the **New project** dialog to create an empty
project with a name of 1–128 UTF-8 bytes, without surrounding whitespace or control
characters. Names need not be unique; the catalog holds up to 64 projects. A
successful save opens the newly created project without restarting the server.
When repository attachment is configured, **Attach repository** opens its labelled
dialog on the project’s Code page. If a save cannot be confirmed, reload the list
before retrying to avoid duplicate names.
An absent catalog shows an empty list; invalid state stops startup or fails the
current request without resetting records. The unconfigured preview retains its
placeholder. Open the project’s **Settings**, edit its name and select
**Save name**. A confirmed save reloads that project with the same ID, repository
associations and checkout paths. If saving cannot be confirmed, reload before
retrying. Project deletion is not implemented. The installation
guide's [offline backup procedure](private-linux-install.md#restart-backup-and-restore)
preserves the owner, optional profile and optional catalog together; repository contents require a
separate backup.

On Unix, an administrator may explicitly add `--repository-root ABSOLUTE_PATH` to
`serve --data-dir ABSOLUTE_PATH`. It requires an existing owner and a trusted,
existing root separate from application data; no root is selected by default.
Each project’s Code page has an **Attach repository** dialog that accepts a relative checkout path under
that root. Opening the form loads a picker of supported immediate child checkouts.
Choose one to fill the path, or enter a nested path manually. Discovery checks at
most 64 root entries; a partial-list notice explains when the limit is reached.
Empty or failed lists keep manual entry available, and **Refresh** retries discovery.
Attachment validates the selected path again. A confirmed save reloads its
repository list; duplicate attachments
preserve the existing default, and uncertain saves ask you to inspect the list
before retrying. Without a configured root, Code explains why attachment is
unavailable while retaining saved repository summaries.
Projects support multiple repositories, linked worktrees share a repository ID,
and each project/repository association keeps one default checkout. Attachment and
picker discovery run no Git commands and grant no build execution permission.
Code immediately lists tracked paths and opens bounded text previews from
the saved checkout. These are current working-tree contents, including uncommitted
changes; untracked files are excluded. Browsing needs no managed creation permission
and does not edit source or approve builds. See [file browsing limits](repository-files.md).

On Code, **View status** reads that saved checkout's branch, commit and
working-tree counts; **Refresh status** updates them in place. **Last checked**
is the browser-local time a valid response was received, not continuous monitoring.
Detached HEAD and branches with no commits have explicit labels. Git must be available on the server's PATH.
Status uses private temporary metadata and controlled Git settings. Unsupported
layouts, changed attachments and failed or bounded-out reads show unavailable
feedback, never a guessed clean result. See [status limits](project-catalog.md#repository-status)
for supported configurations and the meaning of the counts.
See [root policy, endpoint and catalog compatibility](project-catalog.md#repository-attachment-backend)
before enabling it.

Project dialogs support keyboard focus, Escape and Cancel. Closing a dialog retains
its entered values; it does not cancel a submitted request. Reopen it to inspect
pending or failed feedback. Navigation closes dialogs and rechecks the session
before another mutation.

## Deferred browser enrollment

A [local enrollment authorization core](local-enrollment.md) supports a
locally armed, expiring, single-use browser attempt with bounded audit evidence.
Only temporary fixtures use this browser-attempt core. The initial
installation flow is local/SSH CLI setup followed by ordinary browser sign-in;
browser installation is deferred. The preview exposes no first-owner setup route.


## Profile

Open the account menu and choose **Profile** to save a display name or clear it.
The name and initials appear in the menu across devices and service restarts.
An unset name uses **Account**; **Owner** is the role. See the [Profile guide](profile.md)
for validation, session recovery and storage details.

## Optional build-history recovery

`serve --data-dir ABSOLUTE_PATH --build-history-dir ABSOLUTE_PATH` opens an
explicitly selected existing private history directory and marks unfinished saved
attempts interrupted before HTTP starts. An existing configured owner is required.
The server retains its exclusive writer lock; unsafe/busy storage or recovery
failure prevents startup. Omit the option to leave history untouched.

This does not enable builds or reconcile leftover processes. See the
[history startup contract](build-history.md#explicit-server-startup) for storage,
checkpoint and recovery limits.

## Saved build attempts

When build history is configured, open a project → **Builds** to inspect its saved
attempts. **View attempt** opens the recorded state, source identifiers and last
stdout/stderr checkpoint. **Source and attempt details** expands exact identities;
**Refresh history** and **Reload saved attempt** perform fresh reads.

These are saved records, not live process status. Interrupted attempts will not
resume. Logs show retained bytes and truncation separately; missing checkpoints
and failed reads have distinct messages. No artifact download is offered.
With the [Linux build runner](web-app-builds.md) explicitly enabled, this page also
opens preparation, review, approval and cancellation for the selected trusted web
app. Otherwise source checks remain available below the history list. See the [history contract](build-history.md#authenticated-project-history)
for authentication, bounds and remaining integration work.


## Explicit HTTPS proxy mode

`serve --public-origin https://platform.example.com --data-dir ABSOLUTE_PATH`
requires an existing owner and accepts only that configured DNS authority (default
HTTPS port 443). Configuration is lowercase ASCII with no port/path/query/fragment;
requests may spell the same authority with explicit `:443`. The backend stays on
127.0.0.1. All routes validate Host; unsafe methods require the configured HTTPS
Origin in addition to their existing session/CSRF or preauthentication checks.
Absolute-form targets must agree with the configured HTTPS authority.

Public mode rejects Forwarded, every X-Forwarded-* header and X-Real-IP at the
backend. The local proxy must remove its own and incoming forwarding metadata,
preserve Host/Origin, and terminate TLS. There is no inferred scheme, trusted IP
header, CORS bypass or unauthenticated enrollment. Cookies remain host-only Secure,
HttpOnly and SameSite=Strict. Restart discards sessions but retains validated owner
and project data. The existing shared sign-in rate limits are unchanged.

Follow the [public installation guide](public-linux-install.md) for the dedicated
service account, exact proxy example, checks and unverified cloud-hosting limits.
Omitting `--public-origin` preserves the existing private Host/Origin policy.

For a separate app URL on the same server, follow [app hostnames](app-hostnames.md).
The project Releases page shows the configured URL and local TLS/routing status;
public DNS and external reachability still need operator verification.

## DevOption updates

On a configured server, Overview shows the running DevOption version and any
reviewed upgrade observation selected by the server administrator. Expand
**Review upgrade and recovery steps** for the operator's SSH actions. Refresh
reloads the saved status; it does not check a remote release feed or start an
upgrade. A stale or unavailable observation needs an administrator check. See
[platform upgrades](platform-upgrades.md#owner-notification-and-operator-action)
for support limits, full command arguments and recovery/data-loss guidance.

## Project environment secrets

Project **Settings → Manage environment secrets** provides separate Development,
Preview and Production scopes when an operator explicitly enables encrypted
storage. You can add, replace and remove values; the page lists metadata only.
Saving alone grants no runtime access. An operator can configure an exact-revision
[HTTPS binding](app-https.md), then the owner reviews and approves deployment.
Builds and guest environment variables never receive values.
See [project secrets](project-secrets.md) for configuration, limits, key custody,
backup/recovery and the remaining runtime boundary.
