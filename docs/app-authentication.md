# Hosted-app session transport

Hosted apps can implement their own browser sessions through the generated HTTPS
app proxy and WASIp2 runtime. This [#294](https://github.com/devoption/devoption/issues/294)
transport supplies no user directory, identity vendor, session database or roles.
App members do not become platform users. Existing stateless apps require no
change and receive no new host capability, secret, environment or outbound access.

## Browser and application contract

An app sets exactly this cookie name, with its own opaque session value:

```http
Set-Cookie: __Host-devoption-app-session=OPAQUE_VALUE; Path=/; Secure; HttpOnly; SameSite=Strict
```

The value must contain 1–512 ASCII letters, digits, underscores or hyphens. Use
an unpredictable session reference or an authenticated, audience-bound token;
the platform does not generate or validate this value. Empty values are allowed
for clearing the cookie. `SameSite=Lax` is also supported. Optional `Max-Age` is
an unsigned 32-bit integer; `Max-Age=0` clears the session. Other attributes,
including `Domain`, `Expires`, duplicate attributes, alternate paths and
`SameSite=None`, are refused by the runtime. Each guest `Set-Cookie` header is
checked independently; invalid cookies are omitted. Other app cookies are not
supported by this bounded transport. The host's separate activity cookie remains
host-owned and unavailable to guests.

The generated app proxy reconstructs Cookie from only this cookie's bounded
value, using Caddy's [cookie placeholder](https://caddyserver.com/docs/caddyfile/concepts)
and [map](https://caddyserver.com/docs/caddyfile/directives/map) directives. All
other cookies are discarded before reaching the worker. A missing/invalid value
produces no usable session. The runtime filters Cookie again, rejecting malformed
or duplicate app values rather than selecting an ambiguous session. The proxy
selects the browser cookie value before reconstructing a single header; apps
must not rely on receiving raw duplicate cookie headers.

| Input | Guest transport |
| --- | --- |
| `__Host-devoption-app-session` | Only supported app cookie, subject to bounds |
| Platform session, pre-session, activity and other cookies | Removed |
| `Authorization`, `Proxy-Authorization`, `X-CSRF-Token`, `X-DevOption-*` | Removed |
| `X-App-CSRF-Token`, `Origin`, app Host | Preserved (Host becomes WASI request authority) |
| Forwarded client/proxy identity headers | Removed by generated proxy |

Use app routes such as `/account/login`, `/account/session` and `/account/logout`.
The app proxy still returns 404 for `/auth`, `/auth/*`, `/sign-in`, `/setup` and
`/setup/*`. It never routes app traffic to the platform session API. Bearer/header
authentication via `Authorization` remains unsupported; a platform credential
must never double as an app credential.

## Origin, CSRF and session responsibilities

For requests carrying a supported nonempty app cookie, the runtime requires a
single `Origin` exactly equal to `https://` plus the request Host on every method
except GET, HEAD and OPTIONS. Missing/null, HTTP, cross-origin and duplicate Origin
values return 403 before invoking the guest. Explicit ports must match. Generated
HTTPS routing validates SNI/Host and forwards the app authority; do not expose the
private worker as a public HTTP listener.
The WASI request scheme still describes the private HTTP hop; construct the
expected external Origin from the HTTPS app authority, not that backend scheme.

The application must validate its own CSRF token (for example in
`X-App-CSRF-Token`) and login Origin, including requests that have no existing
session cookie. It must keep safe methods free of side effects. The transport's
Origin check is not a replacement for those checks. Cross-origin login/SSO
callbacks requiring different cookie attributes need a separately reviewed flow;
they are outside this first contract.

Browsers enforce the Secure `__Host-` cookie's host-only root-path scope, keeping
sibling app and control-plane cookies separate. Applications must independently
validate session audience, expiry and server-side revocation and rotate session
references after login. They must not trust caller-supplied host/identity headers
as authentication. A deliberately replayed token is still untrusted input: the
receiving app must reject another app's token even when a client bypasses browser
cookie rules. Sign-out clears the browser cookie and must apply the application's
own revocation policy. A fresh guest per request supplies no durable session state;
use an explicitly supported persistence mechanism or audience-bound signed tokens.

No platform credentials enter the guest. Apps remain responsible for avoiding
session/CSRF values in their own output and logs; guest stdout/stderr is captured
under the existing bounded-output policy. Sessions grant no platform owner access
and do not alter the platform's cookie/CSRF, client-certificate or OAuth policy.

## Installation and validation

This requires both the updated runtime and a newly generated app Caddyfile from
`scripts/configure_app_hostname.py`. Review/install that configuration through
the existing [hostname workflow](app-hostnames.md); this change does not modify
an installed proxy or deploy any runtime. An old cookie-stripping proxy continues
to discard app sessions. A new proxy with an old runtime cannot establish a usable
app session; upgrade the coordinated components before enabling app login.

Ordinary runtime tests cover token/attribute bounds, mixed credential filtering,
Origin matching, duplicate Origin and unsafe-method refusal. The ignored trusted
acceptance uses a real compiled WASIp2 fixture, two independent runtime workers,
the generated Caddy configuration with temporary local TLS, a cookie-aware HTTP
client and the actual disposable platform server:

```sh
mkdir -p /tmp/devoption-auth-fixture/src
cp examples/web-app/Cargo.toml examples/web-app/Cargo.lock /tmp/devoption-auth-fixture/
cp scripts/fixtures/runtime-app-auth.rs /tmp/devoption-auth-fixture/src/lib.rs
cargo fetch --manifest-path /tmp/devoption-auth-fixture/Cargo.toml --locked
cargo build --manifest-path /tmp/devoption-auth-fixture/Cargo.toml --release --target wasm32-wasip2 --locked --offline --target-dir /tmp/devoption-auth-target
cargo build --workspace --locked --offline
DEVOPTION_TEST_CADDY=/absolute/path/to/verified/caddy \
DEVOPTION_TEST_RUNTIME="$PWD/target/debug/devoption-runtime" \
DEVOPTION_TEST_AUTH_COMPONENT=/tmp/devoption-auth-target/wasm32-wasip2/release/devoption_web_fixture.wasm \
cargo test -p devoption-cli --test server --locked --offline hosted_app_authentication -- --ignored --nocapture
```

Use disposable empty fixture/output directories and the installed WASIp2 target.
CI runs this acceptance with the pinned proxy and release runtime. It covers
sign-in, authenticated reads, sign-out, two independent cookie scopes,
cross-app replay refusal, control-cookie/header exclusion, unsafe Origin and app
CSRF refusal, invalid guest cookie scope and independent real platform owner login.
The fixture uses fixed public test tokens and no real accounts or contacts; it is
not a production authentication implementation or durable revocation proof.
Cookie-aware HTTP acceptance is not a browser-engine or public DNS/ACME test.
