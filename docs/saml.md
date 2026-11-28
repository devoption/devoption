# SAML single sign-on

An installation with an existing owner and an explicit HTTPS `--public-origin`
can enable SAML 2.0 single sign-on. Configure it under **Profile → Manage workspace
users → SAML single sign-on**. Organization identities, metadata, certificates,
hostnames and group assignments belong in the installation's private configuration,
not this repository. Existing TLS/client-certificate access gates still apply.

Every admitted SSO identity has **Administrator** access, including account,
authentication and registry administration. Admission is a trust decision. Local
members retain their existing access; this is not project-specific RBAC. SSO
identities are distinct from the owner and local members, even when email addresses
match. Each identity has an independent profile and rotating browser session.

## Register the application with an identity provider

Use the exact canonical public origin displayed by the platform:

| Field | Value for an illustrative origin |
| --- | --- |
| SP Entity ID | `https://platform.example.test/auth/saml/metadata` |
| ACS URL | `https://platform.example.test/auth/saml/acs` |
| Start URL | `https://platform.example.test/sign-in` |
| Response binding | HTTP-POST |
| NameID format | `urn:oasis:names:tc:SAML:2.0:nameid-format:persistent` |
| Signing | Signed outer Response, RSA-SHA256/384/512; RSA key at least 2048 bits |

Assign the application only to authorized users/groups at the provider. Use a
stable, unique, never-reassigned NameID value. An administrator-managed immutable
employee identifier is suitable; a reusable email address is not. Changing a
NameID or provider Entity ID creates a distinct platform identity and profile.

Download the IdP metadata from its trusted administrator console. Paste the XML
and its exact IdP Entity ID into platform settings. This is an authenticated,
operator-pinned metadata import: unsigned metadata is accepted because the
operator supplies it directly. The platform never fetches metadata or follows
metadata URLs. Do not import untrusted metadata. Updating the metadata pins its
replacement signing certificates; old successful verification is invalidated.

Optionally configure a group attribute and a required group value. Set both or
neither. When configured, the signed assertion must contain an attribute with
that exact name and at least one exactly matching value. This is additional to
provider-side app assignment. A name or email domain alone never grants access.

For providers such as Google Workspace, create a custom SAML app, enable signed
responses, configure persistent NameID mapping, and restrict **User access** to
the appropriate access groups. Group claims require explicit attribute mapping.
See [Google's custom app instructions](https://knowledge.workspace.google.com/admin/apps/set-up-your-own-custom-saml-app)
and [group membership mapping](https://knowledge.workspace.google.com/admin/apps/about-group-membership-mapping).
Google documents assignment propagation of up to 24 hours. Verify both permitted
and unassigned accounts after changes.

## Enable and verify before disabling passwords

1. Keep local authentication enabled and retain local/SSH recovery access.
2. Save the IdP configuration with SAML enabled.
3. Start at the platform's **Sign in with SSO** button in a separate browser profile.
4. Verify the assigned user can administer the workspace. Verify an unassigned
   account is rejected. With a required group, also test an assertion without it.
5. Return to authentication settings. Successful activation of an SSO session
   marks this saved configuration and public origin verified. Only then can local authentication
   be disabled in a subsequent save.

The initial integration is **SP-initiated only**. The platform must issue the
request before the IdP returns a response. Unsolicited IdP/app-launcher callbacks
and an IdP's standalone “Test SAML login” are rejected. Start at the platform URL.

Disabling local authentication denies owner/member password sign-in and closes
local registration and invitation acceptance. It does not delete passwords,
accounts, approvals, projects or profiles. Existing local sessions retain their
existing absolute expiry. Saving settings revokes all SSO sessions and pending
SAML requests, including the saving administrator's SSO session. Reload/check
saved state before retrying an uncertain write. Edits use a revision to reject
stale saves. Changes to the public origin, provider, metadata, group restriction or enabled state
invalidate verification; keep local authentication enabled while changing them. Startup
refuses a different public origin while local authentication is disabled; use the
local/SSH recovery procedure first.

## Sessions, revocation and offboarding

SSO sessions have a 15-minute absolute maximum, shortened by a provider's earlier
`SessionNotOnOrAfter`. There is no sliding renewal. Signing in again rotates only
that identity's session. **Revoke all SSO sessions** removes existing SSO sessions
and pending login requests. Restart also discards sessions and pending requests.

Remove provider app assignment or suspend the upstream account when offboarding.
This blocks future authentication once the provider applies the change; it does
not push a revocation into existing platform sessions. Until propagation completes,
the provider may still permit a new sign-in. For immediate platform-wide denial,
disable SAML while retaining local recovery; this revokes SSO sessions and prevents
new SAML attempts. There is no per-SSO-user suspension list in this first version.

Delegated MCP grants belonging to SSO identities additionally require that
identity to retain a live SSO administrator session on each identity check. SSO
expiry, sign-out, settings changes and all-session revocation therefore stop
subsequent use; they do not cancel already-authorized work. Other credentials,
host SSH access and app-specific sessions require their own revocation paths.

SAML Single Logout, SCIM provisioning, Directory synchronization, immediate
provider-driven revocation and encrypted assertions are not configured by this
integration. Do not promise those capabilities as part of account offboarding.

## Request and storage boundaries

`saml.json` is a versioned, private `0600`, singly linked file in the retained owner
directory. Corrupt, unsupported, symlinked, overly large or permissive files fail
closed; startup never resets them. Writes use a private temporary file, file sync,
atomic rename and directory sync. The account directory's retained writer lock
and authentication worker serialize access. No migration of `owner.bin` occurs.

All ordinary settings and sign-in-start mutations retain the exact Host/Origin
and session/CSRF or preauthentication-CSRF guards. SAML start shares the bounded
five-attempts-per-minute authentication budget; completion uses the serialized
worker without spending another attempt. Crypto/storage work runs off the async
runtime thread; busy requests fail without queueing unbounded work.

Only `POST /auth/saml/acs` accepts a cross-site POST. It still checks the configured
Host and rejects forwarding metadata. It requires a random, five-minute,
single-use request and RelayState, plus a separate host-only Secure/HttpOnly
`SameSite=None` SAML flow cookie. That cookie grants no application session and is
cleared on success. Ordinary session cookies remain `SameSite=Strict`.

The SAML library verifies the pinned signature, issuer, audience restriction,
destination, recipient, bearer confirmation, request correlation and time bounds.
The platform requires a signed outer Response and strict RSA-SHA2 profile with
local signature references and constrained transforms. It consumes a matching
flow before verification, including failures, and uses a bounded in-memory replay
cache. Pending flows are capped at 16; restart destroys them, so pre-restart
responses cannot match a new request. Request bodies and parsing are bounded.
Authentication failures expose no assertions, identities, metadata or storage paths.

The implementation pins `saml-rs` and its pure-Rust XML security dependencies in
`Cargo.lock`. The library is pre-1.0 and states that it has not had an external
security audit. Handler/cryptographic fixture checks are not an independent audit
or live identity-provider acceptance. Review this dependency and perform live
positive/negative account tests before enabling production SSO.

## Local/SSH recovery

Stop the server before editing configuration. Back up the entire owner data
directory without changing its permissions. In the existing private `saml.json`,
set `local_auth` to `true`, `enabled` to `false`, `verified` to `false`, and increment
`revision`. Preserve `version: 1`, every other field and the original owner data.
Save as an owner-owned regular `0600` file with no symlink or extra hard links.
Restart using the same data directory and sign in with the original owner password.
Never delete `owner.bin`, replace the directory, or reset its contents to recover.

If the record is corrupt, restore a known-good private backup and apply those
recovery values. An invalid record deliberately stops startup. Retest SAML with
local authentication enabled before disabling passwords again.

## Validation and captures

`cargo test -p devoption-server --lib saml --locked --offline` covers genuine signed
responses, independent administrator sessions, rejected issuer/audience/recipient/
destination/correlation/time values, unsigned/tampered assertions, wrong group and
browser bindings, replay, expired flows, configuration changes, local-auth gating,
authorization and private storage rejection.

For the disposable browser fixture:

```sh
cargo test -p devoption-server --lib manual_saml_browser_fixture --locked --offline -- --ignored --nocapture
```

It creates only temporary data and uses the included publicly known test signing
key. A browser automation proxy maps the synthetic HTTPS hosts
`platform.example.test` and `idp.example.test` to its printed loopback address,
preserving Host/Origin and cookies. Start at the platform sign-in page, follow SSO,
then choose **Continue synthetic sign-in** at the fixture IdP. This exercises the
real cross-site POST and production handlers; it does not contact Google or prove
public DNS/TLS, mTLS proxy configuration, certificate rotation or live assignment.
