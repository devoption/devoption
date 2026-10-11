# Browser cookie transport fixture

The normal preview cannot issue browser sessions yet. This optional manual check
uses an ignored Rust test, a temporary private owner store with a fixture password,
and an ephemeral `127.0.0.1` listener. It changes no real account or preview settings.
The fixture's activation route is test scaffolding, not a product sign-in route.

Run from the repository root:

```sh
cargo test -p devoption-server --lib --locked --offline manual_cookie_browser_fixture -- --ignored --nocapture
```

Open the printed URL in a fresh browser profile/context. The fixture stops after
three minutes; rerun it if necessary. Use browser developer tools or an existing
browser automation tool to make these same-origin requests:

1. `POST /fixture/activate`: expect 204, an empty body, `Cache-Control: no-store`,
   a `__Host-devoption_session` cookie, and a separate `X-CSRF-Token` response header.
2. Confirm the cookie is host-only, `Secure`, `HttpOnly`, `SameSite=Strict`, and
   scoped to `/`, without a persistent expiry. `document.cookie` must not expose it.
3. `POST /probe` with the browser's cookie but no CSRF header: expect 401. Repeat
   with the returned CSRF token in `X-CSRF-Token`: expect 204. An incorrect token
   must receive 401. Keep the token only in temporary test memory.
4. Activate again. The previous CSRF token must receive 401; the new token must
   receive 204. `POST /logout` with the current token must return 204 and clear
   the cookie. A subsequent probe must receive 401.
5. Repeat on `localhost` at the same printed port to check the other allowed host.
   Close the isolated browser context afterward. Do not save or print tokens.

Keep browser security defaults. Unsupported Secure-cookie behavior must fail
closed; never remove `Secure` or use an insecure-browser flag to make the test pass.
Chrome 153.0.8010.48 passed this check on macOS for both loopback hostnames during
#73 validation. Other browsers have not been verified; this is not a compatibility
claim for all browsers.
This verifies a transport adapter, not a complete sign-in/enrollment flow.
The manual activation route does not exercise pre-authentication CSRF. A separate
automated fixture now tests a single-use pre-session cookie/header pair before
password parsing and sign-in, including replay rejection and fresh authenticated
credentials. Run it with:

```sh
cargo test -p devoption-server --lib --locked --offline preauth
```

Pre-sessions grant no identity or enrollment permission. At most 16 pairs are
pending, each for five minutes; full capacity fails closed until a pair is consumed
or expires. Every admitted submission consumes its pair even if the password/form
is invalid. The client must request another challenge to retry. Consumed cookies
are inert and the next challenge replaces them. Production route/UI wiring and
explicit enrollment authorization remain required.

There is no TLS in this loopback preview. Cookies are not isolated by port, so
other services on the same host remain inside the trust boundary. Host/Origin
checks and explicit synchronizer-token validation remain required even with
`SameSite=Strict`. The server's absolute session expiry remains authoritative;
browser cookie retention does not renew it.

References: [cookie attributes and prefixes](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie),
[synchronizer CSRF tokens](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html#synchronizer-token-pattern).
The separate pre-session follows the
[login CSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html#possible-csrf-vulnerabilities-in-login-forms).
