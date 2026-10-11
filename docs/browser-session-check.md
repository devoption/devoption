# Browser cookie transport fixture

The unconfigured preview cannot issue browser sessions. This optional manual check
uses an ignored Rust test, a temporary private owner store with a fixture password,
and an ephemeral `127.0.0.1` listener. It changes no real account or preview settings.
It uses production authentication handlers with only a temporary fixture owner.

Run from the repository root:

```sh
cargo test -p devoption-server --lib --locked --offline manual_cookie_browser_fixture -- --ignored --nocapture
```

Open the printed URL in a fresh browser profile/context. The fixture stops after
three minutes; rerun it if necessary. Use browser developer tools or an existing
browser automation tool to make these same-origin requests:

1. `POST /auth/challenge`: expect 204, an empty body, `Cache-Control: no-store`,
   a `__Host-devoption_preauth` cookie, and a separate `X-CSRF-Token` response header.
2. `POST /auth/sign-in` without the header must return 403. Then submit the header
   and `Content-Type: application/x-www-form-urlencoded` with the fixture-only body
   `password=browser+transport+fixture+only`. Expect 204 and a fresh
   `__Host-devoption_session` cookie and `X-CSRF-Token` header. Reusing the pre-session
   token must return 403. This known password belongs only to the ignored fixture.
3. Confirm both cookies are host-only, `Secure`, `HttpOnly`, `SameSite=Strict`, and
   scoped to `/`. The authenticated cookie has no persistent expiry; the pre-session
   has a five-minute maximum age. `document.cookie` must not expose either cookie.
4. `POST /probe` without the authenticated CSRF header must return 401; with that
   header it must return 204. Keep tokens only in temporary test memory.
5. Request another challenge and sign in again. The previous authenticated CSRF
   token must receive 401 on both `/probe` and `/auth/sign-out`, preserving the
   new session. `POST /auth/sign-out` with the current token must return 204 and
   clear the session cookie. A subsequent probe must receive 401.
6. Repeat on `localhost` at the same printed port to check the other allowed host.
   Close the isolated browser context afterward. Do not save or print tokens.

The fixture shares the five-attempt authentication limit; its initial owner setup
uses one attempt. The two sign-ins on each hostname above use the other four.
Rerun the fixture or wait for the rolling minute if further attempts return 429.

Keep browser security defaults. Unsupported Secure-cookie behavior must fail
closed; never remove `Secure` or use an insecure-browser flag to make the test pass.
Chrome 153.0.8010.48 passed this check on macOS for both loopback hostnames during
#79 validation against the production challenge/sign-in/sign-out routes. Other
browsers have not been verified; this is not a compatibility claim for all browsers.
This verifies a transport adapter, not a complete sign-in/enrollment flow.
The automated fixture uses the production auth router to test a single-use
pre-session cookie/header pair before password parsing and sign-in, including
replay rejection and fresh authenticated credentials. Run it with:

```sh
cargo test -p devoption-server --lib --locked --offline preauth
cargo test -p devoption-cli --test server --locked --offline authentication
```

Pre-sessions grant no identity or enrollment permission. At most 16 pairs are
pending, each for five minutes; full capacity fails closed until a pair is consumed
or expires. Every admitted submission consumes its pair even if the password/form
is invalid. The client must request another challenge to retry. Consumed cookies
are inert and the next challenge replaces them. Existing-owner HTTP routes require
an explicitly selected store with a valid owner at startup. Browser forms, CSRF
recovery after page reload, and explicit enrollment authorization remain required.
The CLI tests above create only temporary fixture stores and exercise real HTTP
sign-in, rotation, logout, attempt limits, and storage errors. They do not provision
an owner in the user's preview.

There is no TLS in this loopback preview. Cookies are not isolated by port, so
other services on the same host remain inside the trust boundary. Host/Origin
checks and explicit synchronizer-token validation remain required even with
`SameSite=Strict`. The server's absolute session expiry remains authoritative;
browser cookie retention does not renew it.

References: [cookie attributes and prefixes](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie),
[synchronizer CSRF tokens](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html#synchronizer-token-pattern).
The separate pre-session follows the
[login CSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html#possible-csrf-vulnerabilities-in-login-forms).
