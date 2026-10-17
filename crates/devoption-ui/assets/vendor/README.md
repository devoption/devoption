# Bundled syntax highlighter

`highlight.min.js` is the unmodified common-language browser build of Highlight.js
11.12.0, from the upstream `highlightjs/cdn-release` tag `11.12.0`:
https://github.com/highlightjs/cdn-release/blob/11.12.0/build/highlight.min.js

Its BSD-3-Clause license is retained in `highlight.LICENSE`, from:
https://github.com/highlightjs/highlight.js/blob/11.12.0/LICENSE

It is embedded only on repository file pages. No CDN, package install or network
request is required at runtime. `../code-browser.js` chooses known languages by
extension (or Markdown fence) and leaves unsupported source literal. Theme-aware
syntax colors are defined in `../theme.css`.

To update, verify the upstream release tag, replace both files from that tag,
check that the script has no literal closing script tag, and run the repository
checks including `scripts/test_code_browser.cjs`. Refresh affected screenshots
if rendering changes.
