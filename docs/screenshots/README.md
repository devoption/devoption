# Page screenshots

These images show the real local app, including its current empty placeholders.
Projects shows creation, repository attachment and the temporary fixture's example metadata,
not production records.
Keep one default-mode image per main page at a stable filename; Git records earlier
versions. `appearance-light.jpg` additionally shows Appearance following a light
device preference. `appearance-custom.jpg` shows a custom primary and its warning.
`appearance-dracula.jpg` and `appearance-dracula-light.jpg` show Dracula in Dark
and the adapted Light mode.
`appearance-tokyo-night.jpg` and `appearance-tokyo-night-light.jpg` show Tokyo
Night in Dark and Light modes.

Use each path below with the fixture's printed origin and port.

| Page | Path | Capture state | File |
| --- | --- | --- | --- |
| Home | `/` | Signed in | `home.jpg` |
| Projects | `/projects` | Signed in, first attachment form expanded with `tools` selected; 1120 × 1080 | `projects.jpg` |
| Project details | `/projects/{id}` | Signed in, open Example application from Projects, attachment form expanded with `tools` selected; 1120 × 900 | `project-details.jpg` |
| Empty project details | `/projects/{id}` | Create Example new project; automatic attachment form expanded with `tools` selected; 1120 × 900 | `project-empty.jpg` |
| About | `/about` | Signed in | `about.jpg` |
| Appearance | `/settings/appearance` | Signed in | `appearance.jpg` |
| Sign in | `/sign-in` | Signed out | `sign-in.jpg` |
| Account | `/sign-in` | Signed in | `account.jpg` |
| Check account | `/sign-in` | Session verification blocked | `sign-in-check.jpg` |

Use the [temporary browser fixture](../browser-session-check.md) for all captures,
including the Appearance variants below. Start it from the repository root after
installing the [normal prerequisites](../../README.md#try-it-locally):

```sh
cargo test -p devoption-server --lib --locked --offline manual_cookie_browser_fixture -- --ignored --nocapture
```

The fixture creates a temporary owner, catalog and Git metadata trees, and stops
after ten minutes. Use its
printed URL, not the persistent preview on port 3000. Open `/sign-in` in a fresh
browser context and capture `sign-in.jpg` before signing in. Then use the fixture
password `browser transport fixture only`, reload `/sign-in` for `account.jpg`,
and capture Home, Projects, About and all Appearance images while signed in.
Configured pages redirect to Sign in without that session. Rerun the fixture if
it expires; use the new printed port and sign in again. Never provision a real
owner just to take screenshots.

For `sign-in-check.jpg`, sign out, begin a new fixture sign-in, and block its
post-sign-in session check in the browser tool. Capture the resulting **Check
account** state at the same viewport.
Remove the block and verify recovery without another sign-in after capturing it.

1. If source or assets changed, rerun the fixture command and use its new URL.
   The HTML, CSS, SVG, and JavaScript are compiled into the executable.
2. Set the browser content viewport to **1120 × 520 pixels**, at **100% zoom**.
   For Projects, use **1120 × 1080** and expand the first project's **Attach
   repository** form. Wait for the checkout list and select `tools` without
   submitting, to show the relative path
   alongside both fixture projects and the controls. For Project details, open
   **Example application**, use **1120 × 900**, and expand **Attach repository**
   with `tools` selected without submitting. For the empty-project variant,
   create **Example new project** and select `tools` in its already open form
   without submitting, at the same 1120 × 900 viewport.
   A fresh context starts with **Nord**, **Dark**, and **Purple** for the signed-out
   capture. After sign-in, follow the profile popup's **Appearance** link and confirm
   these choices for the main-page captures. The browser remembers them.
3. Open each affected URL at the top of the page. Leave no hover, keyboard-focus
   outline, open menu, or developer overlay in the capture.
4. Capture the browser content viewport as JPEG, excluding browser/desktop chrome.
   Replace the corresponding file above. Do not include private data or use a mockup.
5. Inspect the saved images for readable text, complete content, and the correct
   active navigation link. Check their embeds in the root README before committing.

For `appearance-light.jpg`, open `/settings/appearance` and use browser developer
tools to emulate a light device color scheme. Select **Nord**, **System**, and **Blue**,
with the save confirmation visible and the profile popup closed. Capture the
same viewport, then clear device emulation and return to **Nord**, **Dark**, and **Purple**.
Refresh this image when the controls or light palette changes. Never capture
unrelated browser or profile data.

For `appearance-custom.jpg`, use **1120 × 700 pixels** to include the warning,
choose **Nord** and **Dark**, and set **Primary color** to `#2e3440` (the page background).
Capture the measured 1:1 warning with the save confirmation visible and no open
picker or focus outline. Return to **Dark** and **Purple** and clear temporary
viewport/device overrides after validation.

For `appearance-dracula.jpg`, use **1120 × 520 pixels** and select **Dracula**,
**Dark**, and **Purple**. For `appearance-dracula-light.jpg`, emulate a light
device preference and select **Dracula**, **System**, and **Purple**, including
the light-adaptation note. Capture both with the save confirmation visible.
Return to **Nord**, **Dark**, and **Purple** and clear temporary overrides.

For `appearance-tokyo-night.jpg` and `appearance-tokyo-night-light.jpg`, follow
the same Dark and System/light procedure with **Tokyo Night** and **Purple**.
Use **1120 × 520 pixels**, with the save confirmation visible. Return to **Nord**,
**Dark**, and **Purple** and clear temporary overrides.

The initial images were captured with Codex's browser screenshot tool at these
dimensions. Use an available browser capture tool with the same settings; this
repository does not install a screenshot runner or run visual regression CI.

Commit refreshed images with each UI change. Shared layout/theme changes refresh
all main pages. Non-UI changes retain the existing files, without duplicate captures
or timestamps in filenames. When a new main page is added, extend the table and
root README with its real screenshot.
