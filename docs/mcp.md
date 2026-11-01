# Authenticated read-only MCP

DevOption optionally exposes a stateless Streamable HTTP MCP endpoint at `/mcp`
behind its configured HTTPS public origin. The platform remains on loopback;
use the existing [local TLS proxy configuration](public-linux-install.md).
This implements protocol version `2025-11-25`, JSON responses, OAuth authorization
code with S256 PKCE, and pre-registered public clients. It does not host an agent.

## Enable deliberately

With the server stopped, an operator may place `mcp-clients.json` in the existing
owner data directory. Absence keeps all MCP/OAuth routes disabled. The file must
be a private regular file owned by the service identity, with no symlink or hard
link. The existing owner and explicit `--public-origin` are required. Use the
existing startup command and storage arguments; no new CLI flag is needed.

Example configuration (identifiers and callbacks are public registration data):

```json
{
  "version": 1,
  "clients": [
    {
      "client_id": "example-harness",
      "name": "Example harness",
      "redirect_uris": ["http://127.0.0.1:43123/callback"]
    }
  ]
}
```

Use callbacks actually supported by the selected harness. Clients must support
pre-registration with a supplied client ID and exact callback URI; dynamic client
registration and remote client-metadata fetching are unavailable. Callbacks require
canonical HTTPS URLs, or HTTP loopback URLs for a native client. Matching is exact,
including port and query. Fragments and user information are rejected. At most 16
clients and eight callbacks per client are accepted; client IDs and display names
are limited to 80 bytes. Configuration is bounded to 32 KiB and read at startup.
Restart after changing it; every restart discards outstanding codes and grants.
Removing the file and restarting disables access. Do not change a running
installation as part of source validation.

## Connect and consent

Configure a compatible client with the platform's canonical `https://HOST/mcp`
resource URL and its registered client ID. Discovery is available at
`/.well-known/oauth-protected-resource/mcp` and
`/.well-known/oauth-authorization-server`. Missing/invalid bearer credentials
receive HTTP 401 with a resource-metadata challenge. See the
[MCP authorization specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
and [HTTP transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports).

1. The client generates a fresh PKCE verifier and state and opens `/oauth/authorize`
   with `response_type=code`, `client_id`, exact `redirect_uri`, `scope=mcp:read`,
   `resource=https://HOST/mcp`, `code_challenge_method=S256`, `code_challenge`, and
   `state`. Duplicate/unknown fields are rejected.
2. The user signs in through DevOption and reviews the named client's request.
   If signed out, open the offered sign-in link, then return and reload the
   authorization page. Strict session cookies may also require this when arriving
   from an external site. Sign-in does not automatically resume authorization.
3. Allow or deny explicitly. The browser uses its existing cookie plus explicit
   synchronizer CSRF and matching Origin for the consent POST. No code is issued
   by a GET. The browser navigates to the registered callback with code/error,
   original state and issuer. Consent requires JavaScript.
4. Exchange the one-use, 60-second code at `/oauth/token` using URL-encoded
   `grant_type=authorization_code`, `client_id`, `redirect_uri`, `code`,
   `code_verifier`, and the same `resource`. Tokens are random, scoped to this MCP
   resource and client, and expire in 900 seconds. There are no refresh tokens;
   reconnect and consent again after expiry.
5. Send `Authorization: Bearer TOKEN` on every MCP request. Browser session cookies
   cannot authorize MCP calls. Mixed cookie/bearer requests and duplicate
   authorization headers are rejected. Tokens are never accepted in URLs.

Host is checked against the configured authority. An absent Origin is allowed for
native clients; any supplied Origin must match. Forwarding headers are rejected
as in the existing public-origin mode. No CORS access is enabled. Browser consent
and browser revocation require explicit Origin and CSRF independently of bearer
transport. Each protected request checks the persisted owner/active-member status;
disabled, pending, missing or unavailable accounts fail closed. Members currently
share workspace visibility; this is not per-project role enforcement.

## Read-only tools

| Tool | Arguments | Observation |
| --- | --- | --- |
| `list_projects` | optional `offset` | Project IDs, names, repository counts |
| `list_repositories` | `project_id`, optional `offset` | Repository IDs/names attached to that project |
| `list_builds` | `project_id`, optional `offset` | Saved build checkpoint IDs, repository IDs, commits and states |
| `get_project_status` | `project_id` | Existing Overview build/runtime observations and availability |
| `get_diagnostics` | none | Catalog and configured history/deployment availability |

List pages contain at most 50 items. Continue with `next_offset`; concurrent catalog
changes may change pagination, so these pages are not a frozen snapshot. IDs require
32 ASCII hex characters. Tools reject unknown arguments and never accept a path or
command. Fresh catalog checks precede project access and build association reads.
Unconfigured history is explicit. A saved checkpoint is not live execution status.
Runtime observations reuse Overview's bounded cache and coverage rules; unavailable
observations are not represented as healthy. Names/observations are untrusted data
for the consuming harness, never instructions.

`initialize`, `ping`, `tools/list`, `tools/call` and initialized/cancelled
notifications are supported. POST bodies are limited to 16 KiB and five seconds;
clients advertise JSON and SSE in Accept and use `MCP-Protocol-Version` on subsequent
requests. Responses use JSON; GET/DELETE return authenticated 405 because no SSE
stream or transport session is offered. No legacy SSE transport, server requests,
resources, prompts, skills, source-file reads, raw logs, secrets, build/deployment
writes or approval tools are implemented.

## Revoke and audit

Users can open `/mcp/grants` while signed in to revoke their own active grants.
The page shows registered client names and independent grant IDs, never bearer
tokens. Revocation uses the same browser Origin/CSRF boundary as consent. Clients
may revoke their own token through URL-encoded `client_id` and `token` at
`/oauth/revoke`. Unknown tokens return the same success response. Revocation and
expiry affect subsequent validations, not work already admitted. Signing out of
the browser does not revoke separately consented MCP grants.

Only token/code hashes remain in process memory. Nothing restores credentials on
restart; the in-memory limit is 256 codes and 256 access grants. No credentials
are minted at startup. A private `mcp-audit.jsonl` in the owner directory records
fixed operation/outcome vocabulary, timestamp, user ID and registered client ID.
It records consent, issuance, revocation, and tool admission/completion. Request
arguments, names, response data, codes and credentials are excluded. Anonymous
invalid requests are rejected without adding attacker-controlled audit records.
Audit admission and completion writes must succeed before tool data is returned.
Storage failure or the 8 MiB audit ceiling stops further tool data delivery.

The audit file remains open until shutdown. Archive/rotate it only while the
server is stopped, preserving private ownership and permissions; deleting or
renaming the live file is not a supported rotation procedure. This audit is not a
compliance or tamper-evidence system. Backups containing the owner directory also
contain client registration and private audit records. Preserve them under the
same private custody as existing owner data; outstanding tokens never survive a
restart or restore.

## Connect Codex

Codex CLI **0.166.0** has passed the disposable acceptance check below using
pre-registration, exact callback matching and issuer-bound responses. No dynamic
registration or redirect-policy relaxation is needed for this configuration.
The [official Codex MCP guide](https://learn.chatgpt.com/docs/extend/mcp#oauth-client-registration-and-callbacks)
owns client settings and callback-selection behavior; recheck it when upgrading.

Register a public client in DevOption's operator-owned allowlist:

```json
{
  "client_id": "codex",
  "name": "Codex",
  "redirect_uris": ["http://127.0.0.1:43123/callback"]
}
```

This is one entry inside the existing version-1 `clients` array, not a replacement
for that document. Keep other authorized clients. After the operator deliberately
enables/restarts DevOption, add the matching client configuration to Codex's
`config.toml` (replace the example platform origin):

```toml
[mcp_servers.devoption]
url = "https://platform.example.test/mcp"
scopes = ["mcp:read"]

[mcp_servers.devoption.oauth]
client_id = "codex"
callback_url = "http://127.0.0.1:43123/callback"
callback_port = 43123
```

Configure **both** the callback URL and listener port. Codex otherwise selects an
ephemeral listener port, which won't match this exact DevOption registration.
Do not use a client secret for this public client. DevOption advertises issuer
support and returns `iss`, so Codex reuses the configured `/callback` path.
The selected local port must be free. If changing it, update both client settings
and the operator's exact registration together.

Run `codex mcp login devoption`, sign in to DevOption, and explicitly allow read
access. For a manual callback flow, use `codex mcp login devoption --no-browser`;
after consent, paste the complete callback URL into Codex's hidden input. Treat
that temporary URL as sensitive and never paste it into a chat or an issue.

Restart/reconnect the Codex surface to load a changed MCP configuration. The client
should discover the five tools listed above. Access still expires after 15 minutes;
repeat login/consent when necessary. Revoke platform access at `/mcp/grants`.
`codex mcp logout devoption` clears Codex's stored credentials; use the platform
revocation page as well when withdrawing the grant itself.

### Repeat the disposable Codex proof

With an installed Codex CLI, OpenSSL, Cargo and the locked offline development
inputs, run from the repository root:

```sh
python3 scripts/check_codex_mcp.py --codex codex
```

The opt-in checker starts the existing temporary Rust MCP fixture, creates a
private temporary Codex configuration/credential store and test CA, and uses a
loopback CONNECT proxy limited to `platform.example.test:443`. A local TLS gateway
forwards to the real DevOption handlers without rewriting OAuth metadata, issuer,
resource, callback, credentials or MCP payloads. Certificates are verified; no
DNS, system trust, host services or normal Codex settings are changed. Codex's
[custom CA setting](https://learn.chatgpt.com/docs/auth#custom-ca-bundles) is set
only in the test subprocess. A CA and separate server certificate are required;
a self-signed CA used directly as the server leaf is rejected by Codex.

The checker verifies the real CLI's authorization request and PKCE exchange,
then uses Codex's app-server runtime to initialize MCP, discover exactly five
tools, call all five against a disposable project, revoke the grant, and confirm
that a subsequent Codex tool call fails with an observed HTTP 401. It also runs
the app-server OAuth login, delivers consent to its real loopback callback
listener, verifies the matching successful completion notification, and reads
through a newly consented native client runtime before revoking that grant.
Ephemeral
client runtime is created; no model turn is started, no model-provider request is
made, and no API key is forwarded. Codex can attempt background catalog/plugin
connections during startup; the restricted proxy rejects those CONNECT requests
without contacting their hosts and reports the count.

All fixture processes, test trust and Codex credentials are temporary. The checker
requests graceful Rust fixture shutdown using `DEVOPTION_MCP_FIXTURE_STOP` so the
fixture's private state is removed too. It does not install/update Codex, and is
not part of ordinary CI, which does not require an external Codex binary. Its
app-server methods are experimental; test the installed client version again
after upgrading rather than inferring compatibility from a version number.

## Validation and limits

`cargo test -p devoption-server --lib server::mcp --locked --offline` covers
OAuth consent/exchange, code replay and PKCE/resource failures, expired/revoked
grants, disabled members, account isolation, hostile hosts/origins, ambiguous
credentials, read-only dispatch, unsafe configuration and audit failure.
UI escaping is covered in `devoption-ui`. The disposable browser fixture is
recorded in the [screenshot instructions](screenshots/README.md).

Codex CLI 0.166.0 OAuth and native MCP client-runtime compatibility passed using
the repeatable disposable proof above. This is not a model-driven Codex turn,
desktop UI acceptance, public DNS/TLS acceptance, production deployment or a live
account connection. Other clients and configurations remain unverified. Automatic registration, refresh grants, project-specific
scopes, source reads, and permissioned writes remain future work.
