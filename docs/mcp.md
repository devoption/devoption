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

## Validation and limits

`cargo test -p devoption-server --lib server::mcp --locked --offline` covers
OAuth consent/exchange, code replay and PKCE/resource failures, expired/revoked
grants, disabled members, account isolation, hostile hosts/origins, ambiguous
credentials, read-only dispatch, unsafe configuration and audit failure.
UI escaping is covered in `devoption-ui`. The disposable browser fixture is
recorded in the [screenshot instructions](screenshots/README.md).

This slice has no deployment, live credential issuance or external harness
compatibility acceptance. The browser proof uses a synthetic proxy to exercise
real handlers; it does not establish public DNS/TLS or production readiness.
Client integration must be tested for the selected harness and callback before
operational enablement. Automatic registration, refresh grants, project-specific
scopes, source reads, and permissioned writes remain future work.
