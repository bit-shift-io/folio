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
- Single binary: the frontend is embedded at build time via a generated
  `include_bytes!` table; only the icon theme is read from disk at runtime

## Why not …

- **No authentication, no JWT.** Folio binds to `127.0.0.1` only. It is a
  localhost tool, not a multi-user web app.
- **No arbitrary command execution.** Browsing never runs anything. The single
  exception is the explicit *Open with* action: it launches an application you
  pick from the list of apps folio itself enumerated, with the target passed as
  one argv element and no shell involved.
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

## HTTP surface

Every path is **absolute**; `--root` only sets the starting directory, and
nothing is confined to it. See [ARCHITECTURE.md](ARCHITECTURE.md) for how the
pieces fit together.

| Route | Method | Purpose |
|---|---|---|
| `/info` | GET | Starting directory and `$HOME` (for `~` in the path bar) |
| `/filetree?path=` | GET | List one directory |
| `/filecontent?path=[&raw=true]` | GET | Preview payload for a file, or the raw bytes |
| `/fileinfo?path=` | GET | Metadata and media info for the properties panel |
| `/filesearch?q=[&path=]` | GET | Recursive case-insensitive name search |
| `/apps` | GET | Installed launchable applications, for the open-with dropdown |
| `/defaultapp?mime=` | GET | Last-used app id for a MIME type |
| `/open` | POST | Launch an enumerated app on a path, and remember it as the default |
| `/rename` | POST | Rename or move `{path, to}` |
| `/delete` | POST | Delete `{path}`, recursively for a directory |
| `/paste` | POST | Copy or cut `sources` into `dest` (`cut` flag) |
| `/icons/{theme}/{*path}` | GET | Icon-theme asset |
| `/ws` | GET | Change-hint WebSocket (below) |
| `/`, `/{*path}` | GET | The embedded frontend |

Mutations answer `200` on success, `400` for invalid input, `404` for a missing
path, `409` for an in-use destination (a cut that would overwrite), and `500`
for io failures.

## WebSocket protocol

The socket at `/ws` carries **hints only** — never file content, tree
snapshots, or auth state. It is bidirectional in the sense that both ends send,
but each direction has exactly one message type:

| Direction | Message | Effect |
|---|---|---|
| server → client | `{"type":"changed","path":"/abs/dir"}` | That directory's listing changed; refetch it over HTTP |
| client → server | `{"type":"watch","path":"/abs/dir"}` | Retarget the file watcher at that directory |

A client sends `watch` on connect and again after every navigation, so the
server only ever watches the folder currently on screen. Hints are debounced
(~200 ms server-side, ~150 ms client-side), so a burst of disk activity
collapses into a single refetch. If the socket drops, the client reconnects
with a 2 s backoff; a stale or missed hint is harmless because the next HTTP
fetch always re-reads the filesystem.

## Tests

```bash
cargo test
```

Runs unit tests for the filesystem core and integration tests that boot the
real router on an ephemeral port and drive it over HTTP and WebSocket, against
a throwaway temp directory.

## License

MIT