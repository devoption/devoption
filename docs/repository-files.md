# Browse repository files

Open a project's **Code** page to browse its files in an expandable folder tree.
It selects the first attached repository in saved order; when there are several,
choose another repository above the tree. Explicit repository links retain that
selection. The tree stays beside the selected file and opens its parent folders;
on narrow screens it stacks above the viewer and can be collapsed with **Files**.
Folder disclosures and file links support keyboard access.

A tracked root README opens automatically, preferring `README.md`,
`README.markdown`, `README.txt`, then `README` (case-insensitive, with lexical
ordering for ties). Nested and untracked READMEs are not selected automatically.
Markdown files open in **Preview**; select **Source** for their literal text.
Plain-text READMEs use the source viewer. If the chosen README is unavailable,
binary or oversized, its normal explanation is shown; other files remain browsable.

Source files show line numbers and syntax highlighting for common languages,
including Rust, JavaScript/TypeScript, Python, HTML/CSS, JSON, TOML and YAML.
Unknown extensions remain plain text. Highlighting uses a bundled library and
makes no CDN requests. File navigation, Markdown rendering and the Preview/Source
switch work without JavaScript; syntax colors and signing in require JavaScript.

Markdown supports headings, lists, tables, links and fenced code. Raw HTML is
shown as text. Images become text labels without fetching media. HTTP(S) links
are allowed; relative file links resolve only to tracked paths within this
repository. Unsupported links, including fragment-only links, remain text.
Markdown headings do not currently have linkable anchors.

Status and attachment/creation controls remain under **Repository details** below
the viewer. Ordinary links support bookmarks, refresh and browser history.

This is a read-only preview of files currently recorded in the Git index. It may
show uncommitted changes and is **not a pinned commit, source proof or build
approval**. Untracked files are never listed or opened. An empty repository shows
**No tracked files** until files are staged using Git outside DevOption. Source
editing, downloads, commits and build execution are not implemented.
Projects with no repositories show the existing setup actions. Missing roots,
unattached or unavailable checkouts show an explanation rather than an empty list.

## Configuration and bounds

Browsing uses the association's saved default checkout under the administrator's
configured repository root. Both `--repository-root` and
[`--managed-repository-root`](managed-repositories.md) support these reads; enabling
managed creation is not required. The browser cannot choose a root. Git must be
available on the server's PATH. The existing [worktree validation](worktree-validation.md)
and [status configuration limits](project-catalog.md#repository-status) apply.

- At most 256 tracked paths are listed. Paths must be UTF-8, at most 1024 bytes and
  16 components. Unsafe or unsupported index records reject the list, rather than
  silently presenting a partial result.
- The index is copied into private temporary metadata, subject to the existing
  1 MiB bound. A controlled `git ls-files` reads that copy with a private empty
  object directory, a three-second process deadline and a 64 KiB output limit.
  Sparse directory entries remain unexpanded; source Git objects are not needed.
- Text previews are at most 64 KiB, including a file exactly at that limit. UTF-8
  text is HTML-escaped. Binary data, unsupported control characters and directional
  override/isolate characters receive an unavailable-text explanation.
- Symlinks, submodules and sparse directory entries cannot be opened. Missing,
  replaced, unsafe or hard-linked working-tree files show an explicit unavailable
  state. Reads use the retained root and do not follow symlink components.
- Checkout identity, metadata and file fingerprints are rechecked during reads.
  A detected change fails the request. External filesystem writers remain trusted;
  this is not an atomic snapshot of a repository.

All list and file responses use `Cache-Control: no-store`. With an owner configured,
GET and HEAD require the existing session. Unknown project/repository associations
and untracked or invalid requested paths return 404. Changed attachments return
409; unavailable storage or unsupported/bounded-out inspection returns 503. A
known tracked file that cannot be previewed has a normal page explaining why.
No source contents are written to the owner store or catalog.

## Validation

Temporary Git fixtures cover tracked versus untracked membership, current content,
Unicode links, HTML escaping, empty/binary/oversized/missing files, unsafe paths,
symlinks, hard links, replaced checkouts, index limits and cross-project rejection.
HTTP tests cover authentication, HEAD, restart reads and unchanged catalog/index
bytes. The [screenshot fixture](screenshots/README.md) supplies staged sample source
for browser checks, including keyboard links, mobile widths, 200% text resizing,
deep-link reloads and browsing without JavaScript.

Implemented under [#169](https://github.com/devoption/devoption/issues/169), with
direct Code browsing under [#179](https://github.com/devoption/devoption/issues/179)
and the tree/README/source viewer under [#250](https://github.com/devoption/devoption/issues/250).
