# Page screenshots

These images show the real local app, including its current empty placeholders.
Keep one default-mode image per main page at a stable filename; Git records earlier
versions. `appearance-light.jpg` additionally shows Appearance following a light
device preference. `appearance-custom.jpg` shows a custom primary and its warning.

| Page | Local URL | File |
| --- | --- | --- |
| Home | http://127.0.0.1:3000/ | `home.jpg` |
| Projects | http://127.0.0.1:3000/projects | `projects.jpg` |
| About | http://127.0.0.1:3000/about | `about.jpg` |
| Appearance | http://127.0.0.1:3000/settings/appearance | `appearance.jpg` |

From the repository root, start the app after the [normal setup](../../README.md#try-it-locally):

```sh
cargo run --locked --offline -- serve
```

1. If source or assets changed, stop and restart the server, then refresh the page.
   The HTML, CSS, SVG, and JavaScript are compiled into the executable.
2. Set the browser content viewport to **1120 × 520 pixels**, at **100% zoom**.
   Follow the profile popup's **Appearance** link. Choose **Dark** and **Purple** for
   the main-page captures. The browser remembers these choices.
3. Open each affected URL at the top of the page. Leave no hover, keyboard-focus
   outline, open menu, or developer overlay in the capture.
4. Capture the browser content viewport as JPEG, excluding browser/desktop chrome.
   Replace the corresponding file above. Do not include private data or use a mockup.
5. Inspect the saved images for readable text, complete content, and the correct
   active navigation link. Check their embeds in the root README before committing.

For `appearance-light.jpg`, open `/settings/appearance` and use browser developer
tools to emulate a light device color scheme. Select **System** and **Blue**,
with the save confirmation visible and the profile popup closed. Capture the
same viewport, then clear device emulation and return to **Dark** and **Purple**.
Refresh this image when the controls or light palette changes. Never capture
unrelated browser or profile data.

For `appearance-custom.jpg`, use **1120 × 700 pixels** to include the warning,
choose **Dark**, and set **Primary color** to `#2e3440` (the page background).
Capture the measured 1:1 warning with the save confirmation visible and no open
picker or focus outline. Return to **Dark** and **Purple** and clear temporary
viewport/device overrides after validation.

The initial images were captured with Codex's browser screenshot tool at these
dimensions. Use an available browser capture tool with the same settings; this
repository does not install a screenshot runner or run visual regression CI.

Commit refreshed images with each UI change. Shared layout/theme changes refresh
all main pages. Non-UI changes retain the existing files, without duplicate captures
or timestamps in filenames. When a new main page is added, extend the table and
root README with its real screenshot.
