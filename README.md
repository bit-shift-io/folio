# Folio

Fast, single-binary local web file explorer written in Rust.

Folio is a full-filesystem browser served over plain HTTP on `127.0.0.1`.
It starts in the directory you give it (default: current), and from there you
can navigate anywhere — `..` walks up level by level and an editable path bar
jumps to any path. Lazy directory loading, instant filename search, inline
previews, and rename/delete actions — all in a single self-contained binary
with zero external dependencies at runtime.

## Features

- Browse the whole filesystem over localhost; `--root` just sets where you
  start (default: current directory)
- Folder tree: expandable tree of folders only (lazy-loaded, rooted at `/`),
  click a row to open that folder — it stays highlighted as you move around
- Files list: the middle pane lists just the files of the open folder, with an
  icon and the file name; click to preview
- Editable path bar: type an absolute path, `~`, or a relative path (`..`
  works too) and press Enter to jump straight there
- Debounced filename search from the current directory (case-insensitive; from
  `/` it skips junk dirs like `.git`, `target`, `node_modules` and pseudo-roots
  like `/proc`, `/sys`, `/dev`, `/run`) — results land in the files list
- Inline previews: text, images (raw bytes), and binary sizing
- Dolphin-style folder view: when no file is selected the preview pane shows
  the open folder as an icon grid (icon + name per item, real image
  thumbnails for pictures) — **Ctrl + mouse wheel** zooms the grid from 16 px
  to 160 px (default 80 px)
- Rename and delete from a small actions menu in the preview header
- Live updates: a `notify` file watcher broadcasts change hints over a
  WebSocket channel, so every open browser refreshes on its own — the watcher
  follows the folder you're viewing — plus a manual refresh button
- KDE `breeze-dark` icon theme for files and folders, served at runtime from
  the vendored theme under `res/icons/breeze-dark` (swap the folder to switch
  themes)
- Single binary: the frontend is embedded with `rust-embed`; only the icon
  theme is read from disk at runtime

## Why not …

- **No authentication, no JWT.** Folio binds to `127.0.0.1` only. It is a
  localhost tool, not a multi-user web app.
- **No command execution.** Unlike some file web UIs, folio never runs
  commands from a web request — browsing can't secretly escalate to a shell.
- **No uploads/editor/share in v1.** Ideas tracked for later versions.

## Build

```bash
# Debug
cargo build

# Release
cargo build --release
```

## Run

```bash
# Serve the current directory
cargo run

# Start in a project folder on a custom port
cargo run -- --root /home/me/Projects --port 9000
```

Then open http://127.0.0.1:4000 in a browser. The path bar shows where you
are; files change on disk and the tree updates live through a WebSocket
change-hint channel (HTTP is always the source of truth — the WS channel only
says *what changed*, it never carries file content).

## Tests

```bash
cargo test
```

Runs unit tests for the filesystem core and integration tests that boot the
real router on an ephemeral port and drive it over HTTP and WebSocket, against
a throwaway temp directory.

## License

MIT