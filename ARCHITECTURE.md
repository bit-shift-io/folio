# ARCHITECTURE.md — System Architecture & Codebase Map (Folio)

> **Purpose:** This document provides a structural map, architectural guidelines, and module breakdown for both human developers and AI assistants. Keep this file updated as key modules, traits, or data flows evolve.

---

## 1. Executive Overview

**Project Goal:** A fast, single-binary local web file explorer written in Rust. It browses the whole filesystem over plain HTTP on `127.0.0.1`, starting at one directory, with a minimal browser-only frontend (no build step, no framework).

### Key Technology Stack

* **Language & Runtime:** Rust (distributor 1.98+), Tokio async runtime (the
  features actually used: `rt-multi-thread`, `macros`, `net`, `sync`, `time`,
  `fs`, `signal`)
* **HTTP Server:** `Axum` (0.8, with `ws`, `tokio`, `http1` features)
* **Static Asset Embedding:** `build.rs` + `include_bytes!` — a generated table
  in `src/assets.rs` covering `web/dist/**` and the `res/icons` fallback copy
* **FileSystem Watching:** `notify` (v8)
* **CLI Engine:** none — hand-rolled `src/cli.rs` (see below)
* **Desktop Entries:** none — hand-rolled `read_fields` in `src/apps.rs` (see below)
* **Serialization:** `serde` & `serde_json`
* **Logging/Tracing:** `tracing` & `tracing-subscriber`

### Core Design Principle: HTTP is Truth

Folio's WebSocket channel is **hints-only**. It never carries file content,
tree snapshots, or authentication state. Clients load everything over plain
HTTP (`/filetree`, `/filecontent`, `/filesearch`, `/rename`, `/delete`) and the
WS channel only broadcasts `{type: "changed", path: "<parent-dir>"}` hints when
the filesystem moves. Any client reconnect, refresh button, or stale hint is
benign: the next HTTP fetch just re-reads the current disk state. This keeps
the watcher, the HTTP layer, and every browser loosely coupled with a single
source of truth (the filesystem).

A corollary: **hints are for dirs, not files.** Mutations and watch events are
reduced to the *parent directory* of what changed, so a client can refetch the
smallest affected listings. Hint paths are absolute; `path: "/"` means the
filesystem root.

---

## 2. Directory & Module Hierarchy

```text
.
├── Cargo.toml               # Project manifest (single bin + lib, no workspace)
├── build.rs                 # Walks web/dist + res/icons, emits the asset table
├── README.md                # User-facing overview
├── ARCHITECTURE.md          # This document
├── src/
│   ├── main.rs              # Entry point: parse args, build state, bind, serve
│   ├── lib.rs               # Library crate: exposes fs + server to integration tests
│   ├── cli.rs               # Hand-rolled arg parser (--root/--port/--help/--version)
│   ├── assets.rs            # GENERATED asset table + lookup tests
│   ├── fs/                  # Pure filesystem operations (unit-tested, no axum)
│   │   └── mod.rs
│   └── server/
│       ├── mod.rs           # AppState, routes, handlers, WS hint socket, watcher
│       └── static_files.rs  # Static file serving over the generated asset table
├── res/
│   └── icons/
│       └── breeze-dark/      # Vendored icon theme, served at runtime via /icons
│           ├── mimetypes/64/ # 35 file-type SVGs (verbatim from the theme)
│           ├── places/96/    # 11 folder SVGs (folder, folder-git, …)
│           └── actions/24/   # go-up.svg
├── tests/
│   └── integration.rs       # Boots real router on :0, HTTP + WS assertions
└── web/dist/                # Frontend (embedded at build time)
    ├── index.html           # Topbar + folder-tree/list/preview panes + dropdown root
    ├── app.js               # Vanilla JS: tree, search, preview, WS client, menu
    └── style.css            # Dark theme CSS
```

---

## 3. Data Flow

### Listing and preview (read path)

```text
Browser ── GET /filetree?path=             ──► server ── spawn_blocking ──► fs::list_dir(abs dir)
Browser ── GET /filecontent?path=&raw=     ──► server ── spawn_blocking ──► fs::get_file_content(abs path)
Browser ── GET /filesearch?q=&path=        ──► server ── spawn_blocking ──► fs::search_files(base, q, limit)
Browser ── GET /info                        ──► { root, home }
```

There is **no root confinement**. Every handler interprets its `path` argument
with `resolve_path(raw, base)` where `base` is the initial root: an empty value
resolves to `base`, a leading `/` is used as-is, and anything else is joined
onto `base`. `..` resolves through the OS exactly as it would in a shell. A
missing directory on `/filetree` is `404`, a non-directory is `400`, and io
failures are `500`.

### Mutations (write path)

```text
Browser ── POST /rename {path, to}     ──► fs::rename_path ──► broadcast hint(parent(path))
Browser ── POST /delete {path}         ──► fs::delete_path ──► broadcast hint(parent(path))
Browser ── POST /paste {sources,dest,cut} ──► fs::copy_path / fs::move_path per source
                                          ──► broadcast hint(dest) + hint(parent(src)) for cuts
```

Mutations are plain HTTP POSTs (they are the writer acting on truth). After a
successful mutation the server broadcasts change hints for the affected parent
directories, so other open browsers refresh — and the originating client also
refreshes locally (double-refetch is harmless).

`/paste` is the one-shot form of the clipboard: the **client** holds the single
`{path, cut}` entry and the server keeps no state at all. Copies are
uniquified so they can never overwrite (`unique_copy_dest`); cuts keep the
original name and refuse a collision with `409`, leaving the source untouched.
The whole loop runs inside one `spawn_blocking`; the response is the last
destination path, or `{ok:false}` with the mapped status.

### Change detection (watch path)

```text
notify event ──► absorb() ──► debounce queue (200 ms) ──► broadcast hint(watched abs dir)
```

The watcher **follows the viewed directory**: on connect and after every
navigation the client sends `{type: "watch", path: "<abs dir>"}` over the WS;
the server unwatches the previous directory and subscribes `notify` over the
new one. Noise is kept out at three levels:

- The filesystem root `/` is watched **non-recursively** (only direct children
  surface), and the pseudo-filesystems `/proc`, `/sys`, `/dev` — reachable from
  `/` — are **not watched at all** (a project folder named `proc` anywhere else
  is still watched).
- Incoming events are filtered to `Create` / `Modify` / `Remove` / `Any`, made
  absolute, and only kept when the event's **parent directory equals the
  currently watched dir** — so deep churn under subdirectories (caches,
  build dirs) is dropped entirely.
- Surviving events are reduced to the watched dir path, coalesced in a
  `BTreeSet` over a 200 ms debounce window, and broadcast as one hint per
  watched dir per burst.

---

## 4. Module Breakdown

### `src/fs` — filesystem core (pure, axum-free, unit-tested)

| Function | Purpose |
|---|---|
| `list_dir(dir)` | Direct children of an absolute dir; dirs first, then files, case-insensitive alpha; symlinks to dirs list as dirs; `NotFound`/`NotADirectory`/`Io` errors surfaced |
| `search_files(base, q, limit)` | Case-insensitive substring DFS over absolute paths; skips `SKIP_DIRS`; from `/` also skips pseudo-roots (`/proc`, `/sys`, `/dev`, `/run`) |
| `get_file_content(path)` | Preview payload for an absolute path: text, binary detection, image flag; errors folded into `error` string |
| `delete_path(path)` | `remove_file` vs `remove_dir_all` (via `symlink_metadata`); rejects `/` |
| `rename_path(from, to)` | `std::fs::rename`; rejects `/` as source |
| `copy_path(src, dest)` | Recursive copy: files byte-exact with permissions, dirs mirror structure, symlinks recreated as links; a partial destination is cleaned up on error |
| `move_path(src, dest)` | `std::fs::rename`; refuses `/`, refuses a dir into its own subtree, falls back to copy + delete across devices |
| `unique_copy_dest(dir, name)` | Free destination name for a copy: as-is, else `name (copy).ext`, `name (copy 2).ext`, … |
| `mime_for_path(path)` / `mime_for_entry(path, name)` | Extension tables (`TEXT_EXTS`, `IMAGE_EXTS`) plus a magic-byte `sniff_mime` fallback for extensionless files; also the single source of `Content-Type` for embedded assets |

Shared constants: `SKIP_DIRS = [.git, target, node_modules, dist, build, .venv, .idea, .vscode, .DS_Store]`, `PSEUDO_ROOTS = [proc, sys, dev, run]`.

Wire types `FileTreeEntry` and `FileContent` are defined here and re-exported
through the server.

### `src/server` — HTTP, WS, watcher

* **`AppState`** — `{ root, home, tx, watch_tx, watch_rx }`. `root` is the
  initial directory supplied via `--root`; `home` is `$HOME` (for `~` in the
  path bar); `tx` is a `broadcast::channel(256)` of `ChangeHint`; the watch
  channel (`mpsc::unbounded`) carries `{type:"watch", path}` commands from WS
  clients to the watcher task.
* **Routes** — all 15, as registered in `build_router`:

  | Route | Method | Purpose |
  |---|---|---|
  | `/info` | GET | Initial directory and `$HOME` (for `~` in the path bar) |
  | `/icons/{theme}/{*path}` | GET | Icon-theme asset from disk at `res/icons`, embedded copy as fallback |
  | `/filetree` | GET | List one directory (`?path=`) |
  | `/filecontent` | GET | Preview payload for a file (`?path=`), or raw bytes with `&raw=true` |
  | `/fileinfo` | GET | Metadata + media header info for the properties panel |
  | `/filesearch` | GET | Recursive name search (`?q=`, optional `?path=`) |
  | `/apps` | GET | Installed launchable applications, for the open-with dropdown |
  | `/open` | POST | Launch an enumerated app on a path; records it as the default for that MIME type |
  | `/defaultapp` | GET | Last-used app id for a MIME type (`?mime=`) |
  | `/ws` | GET | Change-hint WebSocket (server pushes, client retargets the watcher) |
  | `/rename` | POST | Rename or move `{path, to}` |
  | `/delete` | POST | Delete `{path}`, recursively for a directory |
  | `/paste` | POST | Copy or cut `sources` into `dest` (`cut` flag); one shot, server is stateless |
  | `/` | GET | App shell (`index.html`) |
  | `/{*path}` | GET | Any other embedded asset under `web/dist` |

* **`hint_socket`** — per-connection WS relay, now **bidirectional**. It
  subscribes to `tx`, drains the pre-subscribe backlog with a **non-blocking**
  `try_recv()` loop (important: `recv().await` never returns "empty" and would
  stall the socket forever), then `tokio::select!`s between incoming hints and
  client frames; a client `{type:"watch", path}` message is forwarded to the
  watcher task to retarget it, other messages are ignored, and a close tears
  the connection down.
* **`spawn_watcher`** — one async task owns the `notify` watcher. `build_watcher`
  creates it and its event channel; the task then sets the initial watch and
  `select!`s between filesystem events (absorbed, then handed to `debounce`,
  which batches until 200 ms of quiet then flushes) and watch commands
  (`set_watch`).
* **`static_files`** — reads the `build.rs`-generated asset table in
  `src/assets.rs` (`web/dist/**` plus the `res/icons` fallback copy, one
  `include_bytes!` entry each, keyed by forward-slash project-relative path);
  `index.html` for the root, `Content-Type` from `fs::mime_for_path`, and any
  new asset under `web/dist/` is served with zero code changes.
* **`icon_handler`** — serves icon-theme assets from disk at
  `/icons/{theme}/{*path}` (rooted at `res/icons`), so a theme swap is just a
  directory swap under `res/icons` with no rebuild. The theme is validated to a
  single path component and the subpath is screened against `ParentDir`/root
  components — traversal → 400, missing → 404. `AppState.themes_dir`
  defaults to `$PWD/res/icons`; when the file is absent there the lookup falls
  through to the same `assets::get` table used for the frontend.

### `src/apps` — desktop entries and launching

Folio ships its own reader for the `[Desktop Entry]` group rather than pulling
in a full freedesktop parser; the surface actually needed is small and fully
covered by unit tests.

* **`Fields`** — the extracted keys folio cares about: `type`, `name`, `exec`,
  `try_exec`, `hidden`, `terminal`, `no_display`, `alias_for`, and
  `mime_types`.
* **`read_fields(path)`** — reads only the `[Desktop Entry]` group (localised
  names and every other group are ignored). `Name` must be the unlocalized
  `Name=` key; `Hidden`/`Terminal`/`NoDisplay` are true only on the literal
  `"true"`; `MimeType` is a `;`-separated list. The appid is the filename stem,
  which is what `alias_for` is matched against.
* **`list_apps()`** — scan → merge → sort, over `app_dirs()`
  (`$XDG_DATA_HOME/applications`, then the system dirs). Each file is vetted by
  `scan_entry` into `Scanned::{Keep, Helper, Skip}`: it must be
  `Type=Application`, visible, non-terminal, have a non-empty `Exec`, and not be
  a `flatpak run` shim. Launchability is checked against `TryExec` when present
  and otherwise against the first `Exec` token. A file that accepts files or
  URLs also gets `inode/directory`. `merge_alias_helpers` then folds hidden
  `AliasFor` helpers into their targets' MIME lists. Sorted case-insensitively
  by name for a stable dropdown.
* **`split_exec` / `substitute_codes` / `expand_exec`** — freedesktop `Exec`
  parsing: quoting, the `%f`/`%F`/`%u`/`%U` field codes, and `%%`.
* **`open_with(id, target)`** — resolves `id` against the *same* list
  `list_apps` produced, re-derives the argv, substitutes the target as a single
  field, and spawns it detached — never through a shell, and never a program the
  server did not enumerate. On success it records the id in
  `$XDG_CONFIG_HOME/bitshift/folio/config.json` as the default for the target's
  MIME type.

### `src/cli` — argument parsing

A ~100-line hand-rolled parser, unit-tested, that supports exactly the flags
folio documents.

* **`Args`** — `{ root: PathBuf, port: u16 }`, defaulting to `.` and `4000`.
* **`parse_from(args)`** — returns `Outcome::{Run, Help, Version}`. Accepts
  `--root DIR`, `--port N`, `--help`/`-h`, `--version`/`-V`, and the
  `=`-joined forms. Help and version are pre-scanned and win over any other
  error, matching the behaviour users expect from `cargo`/`git`; a flag-like
  value is rejected as a missing value rather than silently becoming a path.
  Errors are plain strings printed alongside `USAGE`.
* **`USAGE` / `VERSION`** — the help text, and the version string built from
  `CARGO_PKG_VERSION` at compile time.

### `web/dist/app.js` — vanilla JS frontend (no framework, no build)

* **State** — `currentDir` (absolute path string, always present; `"\"` means
  filesystem root), `dirCache` (Map: abs dir path → children), `expandedDirs`
  (Set of abs dirs expanded in the tree), `selectedFile`,
  `searchResults[]/searchMode`, `homeDir` (from `/info`, for `~` expansion).
* **Three panes** — the layout is a folder tree (left), a files-only list
  (middle), and the preview (right):
  - **Folder tree (`#file-tree`)** — folders only, an expandable lazy tree
    rooted at `/` (`renderTreeNode` recurses over `expandedDirs`). A caret
    (`▸`/`▾`) toggles expansion; clicking the row navigates into the folder.
    `revealTree` walks from `/` to `currentDir`, fetching and expanding every
    ancestor so the current folder is always visible and highlighted
    (`.current`). Each row is indented by depth and shows the vendored folder
    icon.
  - **Files list (`#file-list`)** — the files (no folders) of `currentDir` in
    the middle pane, rows of icon + name; clicking previews the file. Search
    results render here too (`renderFileList` in search mode shows flat match
    rows with full paths, searched from the current directory), and the
    subtitle under the list shows `N files` / `N matches`.
  - **Preview** — see below.
  Icons come from the vendored theme (`iconEl`; URLs are built from the
  `ICON_DIR` constants — `icons/breeze-dark/…` — and served by
  `icon_handler`, folders using per-name icons like `folder-git`/
  `folder-development`, files via `NAME_ICONS`/`EXT_ICONS` e.g.
  `Cargo.toml`/`cargo.lock`/`*.toml` → `application-toml`, dot-config files →
  `text-x-script`).
  An editable `#path-input` in the top bar shows the current directory; Enter
  jumps to any typed path (absolute, `~/...`, or relative to the current dir),
  Escape reverts.
* **Preview** — `selectFile` branches: error / image (`raw=true` img) / binary
  ("Binary file (N bytes)") / text (`pre`). The header hosts the filename plus
  an actions menu (`…`) with Rename and Delete, built on the `#dropdown` fixed
  container with outside-click + scroll dismissal.
* **Folder view** — when no file is selected, `showFolderView` renders the
  current folder in the preview pane as a Dolphin-style icon grid: each item is
  `gridIconEl` (a breeze-dark type icon, or the *actual image* served via
  `raw=true` for pictures, lazy-loaded) over its name. **Ctrl + mouse wheel**
  zooms the grid: `gridZoom` (1–10, default 5) drives `--icon/--name/--gap`
  CSS custom properties on the `.icon-grid` element (16–160 px icons); the
  header shows live `items · px`. Clicking a grid item reuses `selectEntry`
  (dirs navigate, files preview).
* **Live updates** — `connectSocket` reconnects on close (2 s backoff) and
  sends `{type:"watch", path: currentDir}` on connect and after each
  navigation so the server watcher follows the viewed folder. On
  `{type:"changed", path}` hints (absolute parent dirs), messages are debounced
  ~150 ms client-side then `onFileChanged` clears the dir cache, refetches the
  current directory, re-runs an active search, and re-previews the selected
  file if it is affected.

---

## 5. Error Handling & Security Model

* **No root confinement.** The browser can navigate the entire filesystem, and
  path arguments are resolved by the OS (absolute paths used as-is, relative
  ones joined onto the resolved base). The safety boundary is *binding to
  `127.0.0.1` only* — a localhost tool, with no authentication surface and no
  CSRF-exposed endpoints beyond what a same-machine user already controls.
* **Destructive operations are guarded:** deleting or renaming `/` is refused
  (`400`). Delete uses `symlink_metadata` so a symlink is removed as a link,
  never dereferenced into a `remove_dir_all` of its target. Duplicate/other
  io failures surface as `409`/`500`; the bodies never leak file content.
* **Mutations** return `200` with `{ok:true}` on success, `400` for invalid
  input, `409`/`500` on io or task failures.
* **Reads** fold access errors into `FileContent.error` (200) so previews can
  show a friendly message; a missing raw file is `404`.
* **No command execution:** no web request ever shells out or runs code on the
  host (a `filebrowser` failure mode deliberately avoided). The one deliberate
  exception is `/open`, and it is narrow: the app id must be one the server
  itself enumerated via `/apps`, the argv is built server-side from the
  `.desktop` file's own `Exec` line, the target is substituted as a single
  argv element, and the process is spawned detached with no shell. Browsing
  itself can't reach it.
* **No secrets:** the app only ever reads the filesystem the user pointed it
  at; nothing is persisted or transmitted except over this local channel.

---

## 6. Design Decisions (and trade-offs)

| Decision | Rationale |
|---|---|
| Hints-only WS, HTTP-is-truth | File browsing is read-heavy; a full state channel duplicates truth and races with disk |
| Watcher follows the viewed dir | Only the folder you're looking at is watched — no wasted inotify watches across the whole filesystem |
| Absolute paths, no confinement | The browser IS the filesystem (per the spec); `127.0.0.1` binding is the whole security story |
| Notify watcher + HTTP mutations | Mutations are authoritative writers; the watcher keeps *other* tabs honest about external changes (e.g. edits made in an IDE) |
| Debounce both sides (200 ms server, 150 ms client) | Collapses bursty events (git checkouts, save sprees) into a handful of refetches |
| `127.0.0.1` bind, no auth | Localhost tool by design; avoids the auth/CSRF surface of remote file browsers (`filebrowser` lessons) |
| Folder tree + files list + per-dir cache | The left pane is an expandable lazy folder tree (whole-fs from `/`); the middle pane lists only the files of the open folder; `revealTree` keeps the open folder visible & highlighted; the path bar jumps anywhere instantly |
| Generated `include_bytes!` table for the frontend | One binary, no CDN, no `dist/` install step, and no proc-macro/derive dependency in the hot path |
| Hand-rolled CLI parser and desktop-entry reader | `clap` and `freedesktop-desktop-entry` were the two heaviest trees in the graph (147 → 110 production crates); folio needs ~40 lines of each, so it owns them instead |

---

## 7. Testing Strategy

* **Unit tests** (`src/fs`, `src/server`) — dir ordering, symlink-to-dir
  classification, search limit/skip logic (incl. pseudo-root skipping from
  `/`), rename/delete guards (incl. rejection of `/`), path resolution and
  handler status codes via in-process `oneshot` calls.
* **Integration tests** (`tests/integration.rs`) — build the real `AppState` +
  router, bind `127.0.0.1:0`, and drive it with a small raw
  `tokio::net::TcpStream` HTTP helper (a dev-only HTTP client would have been
  a heavier dependency tree than the tests needed) and `tokio-tungstenite`
  over WebSocket against a `tempfile` directory. Assert absolute-path
  listings, mutation round-trips, change hints with absolute parent paths, and
  that a `{type:"watch", path}` message retargets the watcher (a change in the
  new dir produces a hint, a change in the old dir does not).

> These tests must stay in-process: `AppState::spawn_watcher` requires a Tokio
> runtime, so tests are `#[tokio::test]` and the router is served from inside
> the same runtime.