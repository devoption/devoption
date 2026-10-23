# Named static website

This standalone trusted WASIp2 app uses package `sample-site`, library
`sample_site`, and embedded HTML/CSS/JavaScript. It is deliberately separate from
the original `web-app` fixture. It needs no frontend bundler, runtime filesystem,
database or outbound network capability.

Follow [the build guide](../../docs/web-app-builds.md#application-identity-and-assets)
to commit this directory as a standalone repository, prepare verified offline
inputs, and build through DevOption. Do not attach the platform workspace as the
application repository. The platform review shows the app's package and library;
Cargo emits `wasm32-wasip2/release/sample_site.wasm`.

The handler explicitly maps `/`, `/assets/site.css`, `/assets/site.js` and
`/healthz`; it does not map arbitrary URL paths to files. Unknown paths return 404
and methods other than GET return 405. The JavaScript button updates the greeting.
Edit and commit the embedded assets before requesting a fresh build review.

For a direct toolchain check, after separately acquiring the pinned target and
locked dependencies:

```sh
cargo build --manifest-path examples/static-site/Cargo.toml --release --locked --offline --target wasm32-wasip2
```

This direct command does not exercise platform approval or Linux sandboxing.
On an explicitly disposable Linux systemd host, the repository's
`scripts/check_named_web_app.py --disposable-system-host --runtime PATH`
checks the installed broker/compiler worker, exact CSS/JavaScript responses and
A → B → saved A with the locally compiled runtime. It temporarily installs build
helpers/configuration and refuses an existing build installation. It is not a
production installation or full UI/release/reboot acceptance test.
