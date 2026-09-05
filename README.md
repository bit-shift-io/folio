# Folio

Fast, single-binary local web file explorer written in Rust.

Folio serves one directory over plain HTTP on `127.0.0.1`, giving you a
browser-based file tree with lazy directory loading, instant filename search,
inline previews, and rename/delete actions — all in a single self-contained
executable with zero external dependencies at runtime.

## Features

- Serve a single root directory (default: current directory) on localhost
- Lazy directory tree: folders expand on demand, cached per session
- Debounced filename search across the whole tree (case-insensitive, skips
  common junk dirs like `.git`, `target`, `node_modules`)
- Inline previews: text, images (raw bytes), and binary sizing
- Rename and delete from a small actions menu in the preview header
- Live updates: a `notify` file watcher broadcasts change hints over a
  WebSocket channel, so every open browser refreshes on its own — plus a
  manual refresh button
- KDE `breeze-dark` icon theme for files and folders
- Single binary: static frontend is embedded with `rust-embed`

## Why not …

- **No authentication, no JWT.** Folio binds to `127.0.0.1` only and serves the
  mounted root. It is a localhost tool, not a multi-user web app.
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

# Serve a project folder on a custom port
cargo run -- --root /home/me/Projects --port 8080
```

Then open http://127.0.0.1:8080 in a browser. The root path is shown in the
top bar; files change on disk and the tree updates live through a WebSocket
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