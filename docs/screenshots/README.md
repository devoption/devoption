# Page screenshots

See the [screenshot gallery](gallery.md). These captures show the running
application using a disposable browser fixture; they are not production records
or mockups.

Use stable filenames; Git retains earlier captures. All default captures use
**1280 × 800**, 100% zoom, Nord / Dark / Purple, unless indicated below.

| Page or state | Path | Capture state | File |
| --- | --- | --- | --- |
| Overview | `/` | Signed in | `home.jpg` |
| Projects | `/projects` | Two original fixture projects; empty filter; dialogs closed | `projects.jpg` |
| New project | `/projects` | New project dialog; unsaved name Example new project | `project-create.jpg` |
| Project overview | `/projects/{id}` | Example application; repository list, checkout locations collapsed; 1280 × 900 | `project-details.jpg` |
| Project code | `/projects/{id}/code` | Frontend tree and default README preview; managed creation enabled; status read below viewer; 1280 × 900 | `project-code.jpg` |
| Repository files | `/projects/{id}/repositories/{id}/files` | frontend; three staged paths and README preview; 1280 × 900 | `repository-files.jpg` |
| Repository file | `/projects/{id}/repositories/{id}/files/src/main.rs` | frontend highlighted source with expanded src folder; 1280 × 900 | `repository-file.jpg` |
| Project builds | `/projects/{id}/builds` | Real local Wasm fixture attempts and enabled build action; 1280 × 1200 | `project-builds.jpg` |
| Build review | `/projects/{id}/builds/{repository}/run` | Real prepared fixture, input identities collapsed; 1280 × 1200 | `build-review.jpg` |
| Successful build | `/projects/{id}/builds/attempts/{id}` | Real local Wasm compile with saved diagnostics; 1280 × 1200 | `build-succeeded.jpg` |
| Build attempt | `/projects/{id}/builds/attempts/{id}` | Interrupted synthetic record, saved streams, source details collapsed; 1280 × 1200 | `build-attempt.jpg` |
| Truncated output | `/projects/{id}/builds/attempts/{id}` | Failed synthetic record with inert script/ANSI/bidi text; 1280 × 1200 | `build-output-truncated.jpg` |
| Source checks | `/projects/{id}/builds/{repository}` | backend, clean committed fixture; 1280 × 1400 | `build-source-checks.jpg` |
| Source changes | `/projects/{id}/builds/{repository}` | backend, one modified source file; 1280 × 1400 | `build-source-changes.jpg` |
| App hostname error | `/projects/{id}/releases` | Deliberately untrusted rehearsal certificate, with actionable status; 1280 × 1200 | `app-hostname-error.jpg` |
| Deployed app | Separate app HTTPS hostname | Real saved revision A served by the Wasm worker; 1280 × 1200 | `deployed-app.jpg` |
| Deployment review | `/projects/{id}/releases` | Exact saved release, hostname, health and runtime policy before approval; 1280 × 1500 | `deployment-review.jpg` |
| Project releases | `/projects/{id}/releases` | Real saved Wasm releases, healthy deployment and locally verified HTTPS hostname; 1280 × 1200 | `project-releases.jpg` |
| Project packages | `/projects/{id}/packages` | Unavailable state | `project-packages.jpg` |
| Project settings | `/projects/{id}/settings` | Saved fixture name, no edit | `project-settings.jpg` |
| Create repository | `/projects/{id}/code` | New repository dialog; sample-service entered, not submitted | `repository-create.jpg` |
| Created repository | `/projects/{id}/repositories/{id}/files` | sample-service created; No tracked files; main, No commits yet, Clean; 1280 × 900 | `repository-created.jpg` |
| Empty project details | `/projects/{id}` | Create Example new project; dialogs closed | `project-empty.jpg` |
| Attach repository | `/projects/{id}/code` | Example new project's attachment dialog; tools selected, not submitted | `repository-attach.jpg` |
| Releases | `/releases` | Unavailable state | `releases.jpg` |
| Infrastructure | `/infrastructure` | Unavailable state | `infrastructure.jpg` |
| Profile | `/settings/profile` | Save disposable name Alex Morgan; initials visible | `profile.jpg` |
| Appearance | `/settings/appearance` | Signed in; 1280 × 900 | `appearance.jpg` |
| Sign in | `/sign-in` | Signed out | `sign-in.jpg` |
| Account | `/sign-in` | Signed in | `account.jpg` |
| Check account | `/sign-in` | Session verification blocked | `sign-in-check.jpg` |
| Sign in, light | `/sign-in` | Signed out; Nord / Light / Purple | `sign-in-light.jpg` |
| Checking session | `/sign-in` | Hold the initial session response; fields disabled | `sign-in-loading.jpg` |
| Password error | `/sign-in` | Rejected fixture password; cleared input | `sign-in-error.jpg` |
| Owner setup | `/sign-in` | Separate unconfigured ephemeral preview; no owner | `sign-in-setup.jpg` |

Start the [temporary browser fixture](../browser-session-check.md) from the
repository root after installing the [normal prerequisites](../../README.md#quick-start):

```sh
DEVOPTION_BROWSER_MANAGED_ROOT=1 DEVOPTION_BROWSER_FILES=1 cargo test -p devoption-server --lib --locked --offline manual_cookie_browser_fixture -- --ignored --nocapture
```

Use its printed origin and port, not the persistent preview on port 3000. Managed
creation is enabled only for its temporary root. The fixture creates a temporary
owner, catalog, Git metadata trees, three staged frontend files (`README.md`,
`Cargo.toml`, `src/main.rs`) and an untracked note; it stops after ten minutes.
No real owner or repository is needed.
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
2. Capture Sign in, then sign in and verify automatic navigation to Overview.
   Visit `/sign-in` again to capture Account, then capture the main pages. Use the
   profile menu's Appearance link to confirm Nord / Dark / Purple. Capture Projects
   and Example application before mutating fixture records.
3. Capture Example application’s Overview and each project section. On Code, select the frontend's **View status**. Wait for branch
   `main`, **No commits yet**, **3 staged**, **1 untracked entry**, and **Last checked**. This time
   is when the browser received the valid response, not continuous monitoring.
   Code shows a folder tree and its README preview. Choose **frontend** above the list
   for the explicit repository URL capture, then expand `src` and open `main.rs` and capture
   the source. Return to Code before continuing.
4. On Projects, open **New project**, enter **Example new project** and capture
   the dialog. Submit, capture the new empty details page, then open Code and **Attach
   repository**. Wait for the picker, select `tools`, and capture without submitting.
5. For `sign-in-check.jpg`, sign out, start another fixture sign-in and block its
   post-sign-in `/auth/session` check. Capture **Check account**, then remove the
   block and verify recovery without another password submission.
6. On the new project’s Code page, capture **New repository** with **sample-service**
   entered. Submit, use **View status**, then capture the saved empty repository.
7. Capture the browser viewport as JPEG, excluding browser/desktop chrome. Keep
   scroll at the top, pointer off controls, and no focus outline or developer
   overlay. Only the named dialog captures should have a dialog open.
8. Capture Profile after saving **Alex Morgan** in the disposable fixture. Clear
   the name before other main-page captures so their account label remains **Account**.
9. Inspect the images, text, active navigation and gallery entries before
   committing. Restore browser preferences and clear temporary overrides.

For the Builds captures, start a separate fixture with
`DEVOPTION_BROWSER_PREPARATION=1` using the same Cargo command above. This adds
three committed backend files with fixed public fixture author/date metadata,
without running their Rust source. Open Example application → Builds, capture the
repository choices, then **Check source for backend**. Capture **Source checks
passed**, append a comment to the temporary backend `src/main.rs`, and use **Check
again** to capture **Changes needed** with one modified entry. Restore the temporary
file afterward. The fixture’s printed root identifies these disposable files;
do not change a real checkout or the persistent preview’s repository root.
The three browser flags can also be combined to capture the full set in one
disposable fixture. Check logo/breadcrumb vertical centers at desktop widths and
their shared left gutter in the stacked mobile layout; check every project section
at 320, 375, 768, 800 and 1280 pixels without horizontal page overflow.

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
existing files. Add a capture and [gallery](gallery.md) entry for each new main page.
No screenshot runner or visual regression service is installed by the repository.

For saved history captures, also set `DEVOPTION_BROWSER_HISTORY=1`. This seeds
synthetic Interrupted (recovered from Queued), Failed and Succeeded records for
Example application and a separate Cancelled record for Example documentation.
These are **historical display fixtures, not executed builds**. No artifact is
served. Capture Builds, open the Interrupted attempt for `build-attempt.jpg`,
and open Failed for `build-output-truncated.jpg`. Keep source details collapsed
in the captures, but verify expansion and exact identifiers separately. Confirm
truncated script/control/bidi text remains inert and no horizontal overflow at
320, 375, 768, 800 and 1280 pixels. Source checks retain their existing captures.

For the real runner captures, follow [web-app build setup](../web-app-builds.md)
in a disposable local Ubuntu VM. Use the public fixture and a disposable owner.
Capture the prepared review before approval, then a real successful attempt with
saved compiler output. Existing interrupted/truncated screenshots remain synthetic
display tests. Never substitute seeded success for real build evidence.

For deployment captures, follow [private app deployment](../web-app-deployments.md)
in the same disposable Ubuntu VM. Save real successful builds, capture the exact
review before approval, then capture the healthy release with saved app logs.
These screenshots show the private runtime; public app ingress is not configured.

The app hostname captures use two local test hostnames on the Ubuntu VM and a
disposable Caddy CA. The isolated browser accepts only that CA intermediate key;
separate protocol checks verify the full chain. No system trust or public DNS
changes are involved. Restore the valid trust file after capturing the intentional
certificate failure.
