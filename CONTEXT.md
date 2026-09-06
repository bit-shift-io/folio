# Folio — Context

Shared vocabulary for working on this project together.

## Projects

- **folio** — this app: a fast, single-binary local web file explorer (Rust + Axum, vanilla JS, localhost only). Binary name `folio`.
- **grit** — the Rust git client folio's file-browser UI was extracted from (Iced desktop + Axum web daemon). The "grit-and-folio" pairing: whenever we mine UX or patterns, grit is the reference.
- **krust** — grit's embeddable web terminal (port 3000). Deliberately NOT part of folio.
- **filebrowser/filebrowser** (archived) — Go file explorer used only as a feature reference (breadcrumbs, context menu, multi-select + clipboard, uploads, editor, share, theming).

## App vocabulary

- **panes** (left → right): folder tree / file list / preview. "First pane" = tree, "second pane" = list. Up/Down move the cursor in the active pane; Right enters/expands a folder in the tree, and on the already-open folder moves to the list pane; Right in the list moves to the preview pane; Left in the list returns to the tree; Left in the preview returns to the list; Left in the tree collapses a folder or goes up a level; Tab/Shift+Tab or Ctrl+Left/Right switch panes.
- **home view** — the tree opens rooted at the home dir (everything outside home hidden); the `..` row at the top toggles out to the full-filesystem tree.
- **path bar** (`#path-input`) — the editable location bar; Enter jumps, Escape reverts.
- **filter box** (`#file-filter`) — filename filter/search from the current dir; results land in the list pane.
- **icon theme** — served at runtime from `res/icons/<theme>` via `/icons/<theme>/<subdir>/<file>`; current theme `breeze-dark` (`places/96`, `mimetypes/64`, `actions/24`). Theme swap = swap the folder under `res/icons`.
- **folder view / icon grid** — the preview-pane grid of the open folder; **Ctrl + mouse wheel** zooms 16–160 px (default 80 px).
- **edit / open-with** — the preview header's ✎ button (with a ▾ caret) launches the file in the app last used for its MIME type; the caret (or first use) opens a dropdown of apps matching the file type. The last-used choice is persisted to `$XDG_CONFIG_HOME/bitshift/folio/config.json` (`/defaultapp` reads it, `/open` updates it). The `⋯` button is the Rename/Delete menu.
- **text preview** — files render as a line-numbered table (`#preview-content .text-view`), not a raw `<pre>`; the header shows only the basename (full path in the title tooltip).
- **properties** — the Preview ⋯ menu's Properties item splits the preview pane top/bottom, showing file details from `/fileinfo` (`name/path/size/modified/mode/mime`); media files add `media` info — image dimensions and duration for wav/flac/mp3/mp4 (hand-rolled header sniffers in `src/media.rs`). Selecting it again toggles the panel away; it also closes when the preview leaves the file.
- **hints** — WS messages `{type:"changed", path}`. Principle: "HTTP is truth" — hints only trigger a refetch.
- **dotfiles** — dot-prefixed names are hidden by default; **Ctrl+H** toggles visibility. Non-hidden filtering happens client-side at render time.
- **watch** — client sends `{type:"watch", path}` so the server watcher follows the viewed dir.

## Server

- Endpoints: `/info`, `/filetree`, `/filecontent`, `/fileinfo`, `/filesearch`, `/apps`, `/defaultapp`, `/rename` (POST), `/delete` (POST), `/open` (POST), `/ws`, `/icons/{theme}/{*path}`, embedded static `/`.
- Paths are **absolute everywhere**; `--root` only sets the starting dir.
- Mutation codes: 200 ok, 400 invalid, 404 missing, 409 in-use, 500 io.
- Watching rules: `/` non-recursive, top-level `/proc /sys /dev` skipped, events kept only when the parent is the watched dir.
- `/apps` lists installed desktop apps (filtered to launchable, non-terminal, visible `Type=Application` entries); `/open` launches **only** an app the server enumerated — Exec parsed per freedesktop spec, target passed as one argv element, detached, never through a shell.

## Working conventions

- Run: `./run.sh` or `cargo run -- --root DIR [--port N]` (default port 4000).
- Never git init/commit/push unless explicitly asked.
- Bar: zero compiler warnings; keep unit + integration tests green.
- No browser/js/node in this dev env — JS is verified by review + curl; the user eyeballs visuals.
- Security: the `127.0.0.1` binding is the boundary. Only exception to "no spawning": `/open` may launch a user-picked installed app (verified `.desktop` id, argv built server-side, no shell).