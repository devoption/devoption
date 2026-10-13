# Page screenshots

These captures show the real local application, using the disposable browser
fixture's owner and catalog. They are not production records or mockups.
The complete visual pass in [#153](https://github.com/devoption/devoption/issues/153)
was implemented freshly against the already-built interface preview. Compare the
[preceding app captures](https://github.com/devoption/devoption/tree/17c29c22c309cb7c5b936a0e88a904873c48a5f5/docs/screenshots)
with the current images. The private reference and prototype source are not included.

Use stable filenames; Git retains earlier captures. All default captures use
**1280 × 800**, 100% zoom, Nord / Dark / Purple, unless indicated below.

| Page or state | Path | Capture state | File |
| --- | --- | --- | --- |
| Overview | `/` | Signed in | `home.jpg` |
| Projects | `/projects` | Two original fixture projects; empty filter; dialogs closed | `projects.jpg` |
| New project | `/projects` | New project dialog; unsaved name Example new project | `project-create.jpg` |
| Project details | `/projects/{id}` | Example application; frontend status read with Last checked visible; 1280 × 900 | `project-details.jpg` |
| Empty project details | `/projects/{id}` | Create Example new project; dialogs closed | `project-empty.jpg` |
| Attach repository | `/projects/{id}` | Empty project's attachment dialog; tools selected, not submitted | `repository-attach.jpg` |
| Releases | `/releases` | Unavailable state | `releases.jpg` |
| Infrastructure | `/infrastructure` | Unavailable state | `infrastructure.jpg` |
| About | `/about` | Signed in | `about.jpg` |
| Appearance | `/settings/appearance` | Signed in; 1280 × 900 | `appearance.jpg` |
| Sign in | `/sign-in` | Signed out | `sign-in.jpg` |
| Account | `/sign-in` | Signed in | `account.jpg` |
| Check account | `/sign-in` | Session verification blocked | `sign-in-check.jpg` |
| Sign in, light | `/sign-in` | Signed out; Nord / Light / Purple | `sign-in-light.jpg` |
| Checking session | `/sign-in` | Hold the initial session response; fields disabled | `sign-in-loading.jpg` |
| Password error | `/sign-in` | Rejected fixture password; cleared input | `sign-in-error.jpg` |
| Owner setup | `/sign-in` | Separate unconfigured ephemeral preview; no owner | `sign-in-setup.jpg` |

Start the [temporary browser fixture](../browser-session-check.md) from the
repository root after installing the [normal prerequisites](../../README.md#try-it-locally):

```sh
cargo test -p devoption-server --lib --locked --offline manual_cookie_browser_fixture -- --ignored --nocapture
```

Use its printed origin and port, not the persistent preview on port 3000. The
fixture creates a temporary owner, catalog, Git metadata trees and an untracked
frontend README; it stops after ten minutes. No real owner or repository is needed.
The fixture password is `browser transport fixture only`.

The dedicated auth layout was added in [#155](https://github.com/devoption/devoption/issues/155).
Its seven captures retain the dimensions above. For the light variant, save Nord /
Light / Purple on Appearance before signing out. Hold the initial `/auth/session`
response to capture checking; return an actual rejected fixture sign-in for the
password-error state. Never capture a real password or account. For setup guidance,
run a separate `devoption serve --port 0` without a data directory and use its
printed ephemeral origin. Do not create an owner for that capture.

1. Rebuild/restart the fixture after source changes: HTML, CSS, SVG and scripts
   are embedded in the executable. Start with a fresh browser profile/context.
2. Capture Sign in, then sign in and capture Account and the main pages. Use the
   profile menu's Appearance link to confirm Nord / Dark / Purple. Capture Projects
   and Example application before mutating fixture records.
3. On Example application, select the frontend's **View status**. Wait for branch
   `main`, **No commits yet**, **1 untracked entry**, and **Last checked**. This time
   is when the browser received the valid response, not continuous monitoring.
4. On Projects, open **New project**, enter **Example new project** and capture
   the dialog. Submit, capture the new empty details page, then open **Attach
   repository**. Wait for the picker, select `tools`, and capture without submitting.
5. For `sign-in-check.jpg`, sign out, start another fixture sign-in and block its
   post-sign-in `/auth/session` check. Capture **Check account**, then remove the
   block and verify recovery without another password submission.
6. Capture the browser viewport as JPEG, excluding browser/desktop chrome. Keep
   scroll at the top, pointer off controls, and no focus outline or developer
   overlay. Only the two named dialog captures should have a dialog open.
7. Inspect the images, text, active navigation and root README embeds before
   committing. Restore browser preferences and clear temporary overrides.

All Appearance variants use **1280 × 900** and the save confirmation:

| File | Choices |
| --- | --- |
| `appearance-light.jpg` | Nord, System following an emulated light device, Blue |
| `appearance-custom.jpg` | Nord, Dark, Primary color `#2e3440`; measured low-contrast warning visible |
| `appearance-dracula.jpg` | Dracula, Dark, Purple |
| `appearance-dracula-light.jpg` | Dracula, System following light, Purple; adaptation note visible |
| `appearance-tokyo-night.jpg` | Tokyo Night, Dark, Purple |
| `appearance-tokyo-night-light.jpg` | Tokyo Night, System following light, Purple |

The mode/theme/accent choices are buttons. Use **Reset to Purple** after the custom
capture. The custom warning checks accent/background pairs only and is not a full
accessibility audit.

Commit refreshed images in the same commit as visible UI changes. Shared layout
changes require every main page and affected variants. Non-UI changes retain
existing files. Add a capture and root README embed for each new main page.
No screenshot runner or visual regression service is installed by the repository.
