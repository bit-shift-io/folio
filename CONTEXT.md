# Folio — Context

Shared vocabulary for working on this project together.

## Projects

- **folio** — this app: a fast, single-binary local web file explorer (Rust + Axum, vanilla JS, localhost only). Binary name `folio`.
- **grit** — the Rust git client folio's file-browser UI was extracted from (Iced desktop + Axum web daemon). The "grit-and-folio" pairing: whenever we mine UX or patterns, grit is the reference.
- **krust** — grit's embeddable web terminal (port 3000). Deliberately NOT part of folio.
- **filebrowser/filebrowser** (archived) — Go file explorer used only as a feature reference (breadcrumbs, context menu, multi-select + clipboard, uploads, editor, share, theming).

## App vocabulary

- **panes** (left → right): folder tree / file list / preview. "First pane" = tree, "second pane" = list. Keyboard arrows navigate the active pane; left/right switch panes.
- **path bar** (`#path-input`) — the editable location bar; Enter jumps, Escape reverts.
- **filter box** (`#file-filter`) — filename filter/search from the current dir; results land in the list pane.
- **icon theme** — served at runtime from `res/icons/<theme>` via `/icons/<theme>/<subdir>/<file>`; current theme `breeze-dark` (`places/96`, `mimetypes/64`, `actions/24`). Theme swap = swap the folder under `res/icons`.
- **folder view / icon grid** — the preview-pane grid of the open folder; **Ctrl + mouse wheel** zooms 16–160 px (default 80 px).
- **hints** — WS messages `{type:"changed", path}`. Principle: "HTTP is truth" — hints only trigger a refetch.
- **watch** — client sends `{type:"watch", path}` so the server watcher follows the viewed dir.

## Server

- Endpoints: `/info`, `/filetree`, `/filecontent`, `/filesearch`, `/rename` (POST), `/delete` (POST), `/ws`, `/icons/{theme}/{*path}`, embedded static `/`.
- Paths are **absolute everywhere**; `--root` only sets the starting dir.
- Mutation codes: 200 ok, 400 invalid, 404 missing, 409 in-use, 500 io.
- Watching rules: `/` non-recursive, top-level `/proc /sys /dev` skipped, events kept only when the parent is the watched dir.

## Working conventions

- Run: `./run.sh` or `cargo run -- --root DIR [--port N]` (default port 4000).
- Never git init/commit/push unless explicitly asked.
- Bar: zero compiler warnings; keep unit + integration tests green.
- No browser/js/node in this dev env — JS is verified by review + curl; the user eyeballs visuals.
- Security: the `127.0.0.1` binding is the boundary; never shell out from HTTP.