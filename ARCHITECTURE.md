# ARCHITECTURE.md — System Architecture & Codebase Map (Folio)

> **Purpose:** This document provides a structural map, architectural guidelines, and module breakdown for both human developers and AI assistants. Keep this file updated as key modules, traits, or data flows evolve.

---

## 1. Executive Overview

**Project Goal:** A fast, single-binary local web file explorer written in Rust. It serves one directory over plain HTTP on `127.0.0.1` with a minimal browser-only frontend (no build step, no framework).

### Key Technology Stack

* **Language & Runtime:** Rust (distributor 1.98+), Tokio async runtime (`full` features)
* **HTTP Server:** `Axum` (0.8, with `ws`, `tokio`, `http1` features)
* **Static Asset Embedding:** `rust-embed` (embeds `web/dist/` into the single compiled binary)
* **FileSystem Watching:** `notify` (v8)
* **CLI Engine:** `clap` (v4 with `derive`)
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
smallest affected listings (`path: ""` means the root).

---

## 2. Directory & Module Hierarchy

```text
.
├── Cargo.toml               # Project manifest (single bin + lib, no workspace)
├── README.md                # User-facing overview
├── ARCHITECTURE.md          # This document
├── src/
│   ├── main.rs              # CLI entry point: parse args, build state, bind, serve
│   ├── lib.rs               # Library crate: exposes fs + server to integration tests
│   ├── fs/                  # Pure filesystem operations (unit-tested, no axum)
│   │   └── mod.rs
│   └── server/
│       ├── mod.rs           # AppState, routes, handlers, WS hint socket, watcher
│       └── static_files.rs  # RustEmbed-backed static file serving
├── tests/
│   └── integration.rs       # Boots real router on :0, HTTP + WS assertions
└── web/dist/                # Frontend (embedded at build time)
    ├── index.html           # Topbar + tree pane + preview pane + dropdown root
    ├── app.js               # Vanilla JS: tree, search, preview, WS client, menu
    ├── style.css            # Dark theme CSS
    └── icons/               # breeze-dark SVGs (copied from the KDE theme)
```

---

## 3. Data Flow

### Listing and preview (read path)

```text
Browser ── GET /filetree?path=     ──► server ── spawn_blocking ──► fs::list_dir(root, rel)
Browser ── GET /filecontent?path=&raw=
Browser ── GET /filesearch?q=
Browser ── GET /info
```

Every handler resolves the requested relative path against the single mounted
root through `fs::safe_join`, which rejects absolute paths and any `ParentDir`
(`..`) component. Cross-root reads return `400` before touching the disk.

### Mutations (write path)

```text
Browser ── POST /rename {path, to} ──► fs::rename_path ──► broadcast hint(parent(path))
Browser ── POST /delete {path}       ──► fs::delete_path ──► broadcast hint(parent(path))
```

Mutations are plain HTTP POSTs (they are the writer acting on truth). After a
successful mutation the server broadcasts change hints for the affected parent
directories, so other open browsers refresh — and the originating client also
refreshes locally (double-refetch is harmless).

### Change detection (watch path)

```text
notify event ──► absorb() ──► debounce queue (200 ms) ──► broadcast hint(parent dir)
```

The watcher subscribes `notify` recursively over the root. Incoming events are
filtered to `Create` / `Modify` / `Remove` / `Any`, made root-relative, reduced
to their parent directory, and coalesced in a `BTreeSet` over a 200 ms debounce
window before a single final broadcast per burst.

---

## 4. Module Breakdown

### `src/fs` — filesystem core (pure, axum-free, unit-tested)

| Function | Purpose |
|---|---|
| `safe_join(root, rel)` | Path-traversal guard; `None` on absolute or `..` paths |
| `list_dir(root, rel)` | Direct children; dirs first, then files, case-insensitive alpha; empty on unreadable/traversal |
| `search_files(root, q, limit)` | Case-insensitive substring DFS over names; skips `SKIP_DIRS`; dirs-first |
| `get_file_content(root, rel)` | Preview payload: text, binary detection, image flag; errors folded into `error` string |
| `delete_path(root, rel)` | `remove_file` vs `remove_dir_all` (via `symlink_metadata`) |
| `rename_path(root, from, to)` | Both targets `safe_join`-guarded |
| `mime_for_path` / `is_image_path` | Extension tables (`TEXT_EXTS`, `IMAGE_EXTS`) |

Shared constants: `SKIP_DIRS = [.git, target, node_modules, dist, build, .venv, .idea, .vscode, .DS_Store]`.

Wire types `FileTreeEntry` and `FileContent` are defined here and re-exported
through the server.

### `src/server` — HTTP, WS, watcher

* **`AppState`** — `{ root, root_marker, tx }`. `root_marker` is the canonical
  path used by watcher/handlers; `tx` is a `broadcast::channel(256)` of
  `ChangeHint`.
* **Routes** — `/info`, `/filetree`, `/filecontent`, `/filesearch`,
  `/rename` (POST), `/delete` (POST), `/ws`, and catch-all static serving.
* **`hint_socket`** — per-connection WS relay. Subscribes to `tx`, drains the
  pre-subscribe backlog with a **non-blocking** `try_recv()` loop (important:
  `recv().await` never returns "empty" and would stall the socket forever),
  then `tokio::select!`s between incoming hints and client frames; any client
  message or close tears the connection down.
* **`spawn_watcher`** — one async task owns the `notify` watcher; events flow
  over an mpsc channel into the debounce/flush loop.
* **`static_files`** — `RustEmbed` over `web/dist/`; `index.html` for the root,
  MIME through `mimetype_guess`; any new asset under `web/dist/` (e.g. the
  `icons/` folder) is served with zero code changes.

### `web/dist/app.js` — vanilla JS frontend (no framework, no build)

* **State** — `rootEntries[]`, `expandedDirs` (Set), `dirChildren` (Map keyed
  by dir path), `selectedFile`, `searchResults[]/searchMode`.
* **Tree** — `renderTree` walks the lazy cache; each row is padded by depth and
  rendered with a breeze-dark icon (`iconEl`), dirs open to `folder-open.svg`.
  Search mode flattens match rows and shows full paths.
* **Preview** — `selectFile` branches: error / image (`raw=true` img) / binary
  ("Binary file (N bytes)") / text (`pre`). The header hosts the filename plus
  an actions menu (`…`) with Rename and Delete, built on the `#dropdown` fixed
  container with outside-click + scroll dismissal.
* **Live updates** — `connectSocket` reconnects on close (2 s backoff). On
  `{type:"changed", path}` messages, hints are debounced ~150 ms client-side
  then `onFileChanged` clears/refetches affected dir caches, refetches the
  root, re-runs an active search, and re-previews the selected file if it is
  affected.

---

## 5. Error Handling & Security Model

* **Root confinement:** every user-supplied path passes through `safe_join`.
  Traversal attempts get `400`. There is no symlink escape analysis beyond this
  (a symlink inside the root that points outside is followed — acceptable for
  a localhost-only tool, signature as delivered matches `grit`).
* **Mutations** return `400` on escape attempts, `409`/`500` on io/task
  failures, `200` with `{ok:true}` on success; the body never exposes
  filesystem content.
* **Reads** fold access errors into `FileContent.error` (200) so previews can
  show a friendly message; escape attempts are `400`.
* **No secrets:** the app never reads config files, env secrets, or anything
  beyond the mounted root.

---

## 6. Design Decisions (and trade-offs)

| Decision | Rationale |
|---|---|
| Hints-only WS, HTTP-is-truth | File browsing is read-heavy; a full state channel duplicates truth and races with disk |
| Notify watcher + HTTP mutations | Mutations are authoritative writers; the watcher keeps *other* tabs honest about external changes (e.g. edits made in an IDE) |
| Debounce both sides (200 ms server, 150 ms client) | Collapses bursty events (git checkouts, save sprees) into a handful of refetches |
| Single root, `127.0.0.1` bind, no auth | Localhost tool by design; avoids the auth/CSRF surface of remote file browsers (`filebrowser` lessons) |
| Lazy tree + per-dir cache | Scales to large trees; only visible dirs are fetched |
| `rust-embed` frontend | One binary, no CDN, no `dist/` install step |

---

## 7. Testing Strategy

* **Unit tests** (`src/fs`, `src/server`) — path traversal rejection, skip-dir
  filtering, search limit/ordering, rename/delete guards, handler status codes
  via in-process `oneshot` calls.
* **Integration tests** (`tests/integration.rs`) — build the real `AppState` +
  router, bind `127.0.0.1:0`, and drive it with `reqwest` over HTTP and
  `tokio-tungstenite` over WebSocket against a `tempfile` root directory.
  Assert traversal `400`s, mutation round-trips, and live change hints
  (`inner/created.txt` produces a hint whose `path` is its parent `inner`).

> These tests must stay in-process: `AppState::spawn_watcher` requires a Tokio
> runtime, so tests are `#[tokio::test]` and the router is served from inside
> the same runtime.