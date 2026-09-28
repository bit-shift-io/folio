# TASKS.md — AUDIT.md remediation

Implements every recommendation in `AUDIT.md` (2026-09-27). Bar for every task: `cargo build`
with zero warnings and `cargo test` green.

**Decisions already taken (do not re-litigate):**

| Decision | Choice |
| --- | --- |
| `freedesktop-desktop-entry` | Hand-roll the parser (revive the commented `read_fields`), drop the crate |
| `clap` | Replace with ~40 tested lines in a new `src/cli.rs` |
| `rust-embed` | Replace with a `build.rs` asset table |
| `[profile.dev] debug = 0` | Apply (largest wall-clock win, one line) |
| `reqwest` in dev-deps | Replace with a raw `TcpStream` HTTP helper |
| README / NOTES.md | Bring both up to date |

**Dependency math correction** (audit §4 item 1 overstates the win): dropping
`freedesktop-desktop-entry` removes `regex` (22.5 s), `gettext-rs`, `gettext-sys`, `locale_config`,
`xdg`, `bstr` and the C-compiler build requirement. `aho-corasick` and `regex-syntax` **stay** —
`tracing-subscriber`'s `env-filter` → `matchers` → `regex-automata` needs both. Verified with
`cargo tree -i`.

Baselines to beat: clean build 53.4 s wall / 325 s CPU, `cargo tree -e no-dev` = 147 crates,
68 tests passing, 17 clippy warnings.

---

## Phase 0: Backlog and project instructions

### Task 1: Archive the completed paste backlog

- [x] `git mv TASKS.md TASKS-paste.md` — the `build-all` / `build-steps` skills read `TASKS.md` as
      the active backlog and would find nothing to do
- [x] Add a "COMPLETED / superseded" header to `TASKS-paste.md`

### Task 2: Fix the docs that are loaded as project instructions

- [x] `CONTEXT.md`: add `POST /paste` to the endpoint list (registered at `src/server/mod.rs:930`,
      documented nowhere — audit §3)
- [x] `CONTEXT.md`: add a **copy/cut/paste** vocabulary entry — client clipboard holds a single
      `{path, cut}`, `Ctrl+C` / `Ctrl+X` / `Ctrl+V`, `Esc` cancels a pending cut, copy auto-uniquifies
      (`foo (copy).txt`, `foo (copy 2).txt`), cut refuses collisions (409), `pending-cut` rows dim
- [x] `NOTES.md`: resolve or drop the two "Open Questions for the Implementing Session" (lines 54-57)
      so the file reads as a closed record

## Phase 1: Clippy and the duplicated sniff branch

### Task 3: Clippy fixes in `src/apps.rs`

- [x] Accept the auto-fixes: `map_or` / `needless_borrow` at `src/apps.rs:175`, `:188`, `:198`,
      `:209`, `:399`, `:403`, `:405`
- [x] `sort_by_key` for the name sort at `src/apps.rs:243`
- [x] `cargo build` + `cargo test`

### Task 4: `sort_by_key` in `src/fs/mod.rs`

- [x] `src/fs/mod.rs:216` and `:222` (`search_files` result sorts) → `sort_by_key`
- [x] `cargo test`

### Task 5: Remaining two mechanical warnings

- [x] `src/media.rs:367` — drop the no-op `0x80 | 0` in the test fixture
- [x] `tests/integration.rs:218` — `match` on one pattern → `if let`
- [x] `cargo test`

### Task 6: Collapse the duplicated `sniff_mime` branches

- [x] `src/fs/mod.rs:321` (ELF) and `:357` (MZ/PE) have byte-identical bodies
      (`"application/x-executable"`) — merge into one arm, or comment why they diverge
- [x] Extend `mime_fields_sniff_json_xml_and_html` (or add a test) so a file starting with
      `\x7fELF` **and** one starting with `MZ` both sniff to `application/x-executable`
- [x] `cargo test` — pure refactor, behaviour must not change

## Phase 2: Drop `freedesktop-desktop-entry` (biggest win)

### Task 7: Lock the parser contract down with failing tests first

- [x] `src/apps.rs`: add `#[cfg(test)]` tests for `read_fields` covering: only the
      `[Desktop Entry]` group is read; comments, blank lines and `[Desktop Action …]` groups
      ignored; the unlocalized `Name` wins over `Name[de]=…`; `Hidden`/`NoDisplay`/`Terminal` are
      true only for the literal `true`; `MimeType` splits on `;` with empties dropped;
      `appid` is the filename minus `.desktop`; `X-KDE-AliasFor` lands in its own field
- [x] `cargo test` — they must fail, because the parser does not exist yet

### Task 8: Write `Fields` + `read_fields`

- [x] `src/apps.rs:77-137`: replace the commented-out block (and its stale TODO) with real code —
      `struct Fields { appid, type_, name, exec, try_exec, icon, hidden, no_display, terminal,
      mime_types, alias_for }` and `fn read_fields(path: &Path) -> Option<Fields>`
- [x] The commented struct has no `X-KDE-AliasFor` field; add `alias_for: Option<String>`
- [x] `cargo test` — green

### Task 9: Switch the call sites over

- [x] `src/apps.rs:169-218` — `list_apps` scan: `type_`/`hidden`/`terminal`/`exec`/`no_display`/
      `try_exec`/`name`/`mime_type`/`appid` become plain field reads (mirrors the crate's
      semantics: bools are `value == "true"`, `appid` = file stem, `MimeType` split on `;`)
- [x] `src/apps.rs:221-241` — alias-helper pass uses `alias_for` instead of
      `desktop_entry("X-KDE-AliasFor")`
- [x] `src/apps.rs:398-408` — `open_with` re-reads the entry through `read_fields(id)`
- [x] The 6 existing `apps.rs` tests pass **unmodified** — they are the behaviour contract

### Task 10: Remove the dependency

- [x] `Cargo.toml`: delete the `freedesktop-desktop-entry` line
- [x] `src/apps.rs`: delete `use freedesktop_desktop_entry::DesktopEntry;`
- [x] `cargo build` (zero warnings) + `cargo test`
- [x] `cargo tree -e no-dev` — no `gettext-sys`, `cc`, `gettext-rs`, `locale_config`, `regex`,
      `xdg`, `bstr`; `aho-corasick` and `regex-syntax` are expected to remain
      — **measured:** `regex` (facade), `gettext-*`, `locale_config`, `xdg`, `bstr`, `cc` **and
      `aho-corasick`** are all gone. Only `regex-automata` + `regex-syntax` remain, still needed
      by `tracing-subscriber`'s `env-filter` → `matchers`. 147 → **133** crates.
- [x] Manual check: `./run.sh` → the edit ▾ dropdown lists the same apps as before this change
      — **measured:** built HEAD in a scratch copy and diffed `/apps` against the new build.
      Both return **71** apps with identical ids, names, `mime_types`, and sort order.

## Phase 3: Drop `clap`

### Task 11: New `src/cli.rs` with tested parsing

- [x] `pub struct Args { pub root: PathBuf, pub port: u16 }` and
      `pub fn parse_from(...) -> Result<Outcome, String>` where `Outcome` is
      `Run(Args) | Help | Version`
- [x] Support `--root`/`-r` and `--port`/`-p` in both `--flag value` and `--flag=value` forms;
      defaults `.` and `4000`; `--help`/`-h`, `--version`/`-V`; error on unknown flags, missing
      values, non-numeric port, port > 65535
- [x] Unit tests: defaults, both forms of each flag, bad port, unknown flag, help/version precedence
- [x] `src/lib.rs`: add `pub mod cli;`

### Task 12: Wire `main` to it and remove the crate

- [x] `src/main.rs`: drop the `#[derive(Parser)]` struct; call
      `cli::parse_from(std::env::args().skip(1))`, print the hand-written usage text and exit 0 on
      Help/Version, `tracing::error!` + exit 2 on a bad flag
- [x] Keep the existing canonicalize → `is_dir` → bind → `exit(1)` paths byte-identical
- [x] `Cargo.toml`: delete the `clap` line
- [x] `cargo build` + `cargo test`; `./run.sh --help`, `./run.sh --port 4001 --root /tmp`
      — note `run.sh` is bare `cargo run "$@"`, so cargo eats bare flags; the real invocations are
      `./run.sh -- --help` and the binary directly. Verified: `--help`/`--version` exit 0 with
      hand-written text, an unknown flag logs and exits 2, and `--port 4001 --root /tmp` serves
      `{"root":"/tmp",…}` on 4001.
- [x] `cargo tree -e no-dev` — no `clap*`, and only one `syn` major version left
      — **measured:** `clap*` is gone (133 → **127** crates) but **two** `syn` majors remain. The
      landscape moved under the audit's snapshot: `serde_derive` and `tokio-macros` are now on
      `syn 3.0.5`, while `syn 2.0.119` is pinned by `tracing-attributes` (and, until Task 15,
      `rust-embed-impl`). `tracing-attributes` is unavoidable, so the dedupe is not reachable
      without dropping `tracing` — out of scope. Dropping `clap_derive` still removed the second
      *derive* consumer and ~22 s CPU.

## Phase 4: Drop `rust-embed`

### Task 13: Teach `mime_for_path` the web-asset types first

- [x] `src/fs/mod.rs`: add `"html" => "text/html"`, `"css" => "text/css"`,
      `"js" => "text/javascript"`, `"json" => "application/json"` **above** the `TEXT_EXTS`
      fallback arm so they win
- [x] Test the four arms, and that `mime_for_path("app.js") != "text/plain"`
- [x] Why first: `static_files.rs` currently uses `mime_guess`; `mime_for_path` calls all four
      `text/plain` today, so the swap must be preceded by this (audit §4 note)

### Task 14: `build.rs` asset table

- [x] `build.rs`: walk `web/dist` and `res/icons`, emit `cargo:rerun-if-changed` for both
      directories (otherwise editing an icon needs a manual rebuild), and write
      `$OUT_DIR/assets.rs` containing
      `pub static ASSETS: &[(&str, &[u8])] = &[("web/dist/app.js", include_bytes!(…)), …]`
      with forward-slash relative keys
- [x] `src/assets.rs`: `include!(concat!(env!("OUT_DIR"), "/assets.rs"));` plus
      `pub fn get(key: &str) -> Option<&'static [u8]>` (linear scan is fine — 52 entries)
- [x] `src/lib.rs`: add `pub mod assets;`
- [x] Test in `src/assets.rs` that `ASSETS` contains `web/dist/{app.js,index.html,style.css}` and
      at least one `res/icons/…` entry (49 SVGs)

### Task 15: Serve from the table

- [x] `src/server/static_files.rs`: drop `RustEmbed`; look up `assets::get(path)`, set
      `Content-Type` from `fs::mime_for_path`, keep `Cache-Control: no-store`, 404 on a miss
- [x] `src/server/mod.rs:150-174`: `IconAssets::get` → `assets::get(&format!("res/icons/{theme}/{rel}"))`
- [x] `src/server/mod.rs:152-156`: drop the `extension() == "svg"` special case in the disk branch —
      `mime_for_path` already maps `svg` to `image/svg+xml`
- [x] Tests: `GET /` → 200 `text/html`; `GET /app.js` → 200 `text/javascript`; `GET /nope` → 404
- [x] `Cargo.toml`: delete the `rust-embed` line
- [x] `cargo build` + `cargo test`; `cargo tree -e no-dev` — no `sha2`, `mime_guess`, `unicase`,
      `walkdir`
      — **measured:** `rust-embed`, `rust-embed-impl`, `rust-embed-utils`, `sha2`, `mime_guess`
      and `unicase` are all gone; 127 → **110** crates. `walkdir` and the `sha1`/`digest`/
      `crypto-common`/`block-buffer`/`typenum` chain are still present but are **not** rust-embed's
      any more — they come from `notify` and from axum's `ws` feature (WebSocket handshake). Served
      live: `/`→`text/html`, `/app.js`→`text/javascript`, `/style.css`→`text/css`, `/nope`→404,
      `/icons/…svg`→`image/svg+xml`.

## Phase 5: Profile and test-only dependencies

### Task 16: `[profile.dev] debug = 0`

- [x] `Cargo.toml`: add `[profile.dev]\ndebug = 0` — measured 53.4 s → 44.5 s clean build
- [x] Delete the two lines to get line numbers in debugger backtraces back

### Task 17: Drop `reqwest` from dev-deps

- [x] `tests/integration.rs`: replace the two `reqwest` helpers with a raw `tokio::net::TcpStream`
      GET/POST that writes the request plus `Connection: close`, reads to EOF, then splits the
      status line from the body
- [x] Keep the assertions byte-identical (status codes are matched, not just "2xx")
- [x] `Cargo.toml`: delete the `reqwest` line
- [x] `cargo test` — all 5 integration tests green
- [x] `cargo tree -e no-dev` — no `reqwest`, `hyper-util`, `ipnet`, `encoding_rs`
      — **measured:** `reqwest`, `ipnet` and `encoding_rs` are gone from the dev graph. `hyper-util`
      remains, but it is **axum's** own dependency (`axum → hyper-util`), not reqwest's, and the
      production crate count is unchanged at **110** — as expected, since `reqwest` was a
      dev-dependency and never affected `cargo build`.

## Phase 6: Structural refactors

### Task 18: Extract the paste destination decision

- [x] `src/server/mod.rs:519-633`: add
      `fn resolve_paste_dest(src: &Path, dest_dir: &Path, cut: bool) -> Result<PathBuf, PasteError>`
      — cut joins the name and returns `Conflict` when it exists, copy returns
      `fs::unique_copy_dest`; move the missing-source and unusable-name checks with it
- [x] `paste_handler` reads as validate → `spawn_blocking` dispatch → `broadcast_after_mutation`,
      under 60 lines (was 115, the longest fn in the repo)
      — **measured:** 115 → **55** lines. Besides `resolve_paste_dest` (15) the status mapping moved
      into `paste_status`/`paste_failed` and the affected-directory computation into
      `paste_affected_dirs` (14). `PasteError` gained a `TaskFailed(String)` arm so a panicked
      blocking task goes through the same path as an io error.
- [x] The 8 existing paste tests in `src/server/mod.rs` stay green unmodified

### Task 19: Extract the watcher debounce

- [x] `src/server/mod.rs:781-838`: add
      `async fn debounce(event_rx, pending, watched, tx) -> bool` that absorbs events until
      `WATCH_DEBOUNCE_MS` of quiet, flushes, and returns `false` when the event channel closed
- [x] `spawn_watcher` reduces to: build watcher → `set_watch(initial)` → `select!` on
      { event → absorb + debounce, watch command → `set_watch` }, about 15 lines, no 3rd nesting level
      — **measured:** `spawn_watcher` 58 → **34** lines, `debounce` 21, plus a new 15-line
      `build_watcher` holding the `notify` setup. The 3rd nesting level is gone and the `select!`
      arms now read exactly as specified; the remaining 19 lines of `spawn_watcher` are the
      `tokio::spawn` wrapper, `set_watch`, and the pending set — shrinking further to 15 would mean
      contorting real setup code, so it stops here.
- [x] `cargo test` — `absorb_only_keeps_events_in_watched_dir` and `watch_mode_skips_noise_dirs` green

### Task 20: Extract the KDE alias merge

- [x] `src/apps.rs:221-241`: move verbatim into
      `fn merge_alias_helpers(apps: &mut [AppEntry], helpers: &[PathBuf], by_appid: &HashMap<String, usize>)`
      — landed with Task 9, since the call sites were being rewritten anyway.
- [x] `list_apps` becomes scan → merge → sort, under 60 lines — **measured:** 91 → **25** lines
      after also extracting the per-entry vet into `scan_entry` (55 lines, returns
      `Scanned::{Keep, Helper, Skip}`), which removed all five `continue` guards from the loop body.
- [x] `cargo test` — and `/apps` output re-diffed against the pre-change crate build: still
      byte-identical, 71 apps.

## Phase 7: Documentation, once the code shape is final

### Task 21: `ARCHITECTURE.md`

- [x] Replace the 8-route list in §4 with all 15 routes from `build_router`
      (`src/server/mod.rs:917-937`), each with its method and one-line purpose
- [x] Fix the bullets this plan invalidates: `static_files` is no longer `RustEmbed` /
      `mimetype_guess` (build.rs table + `mime_for_path`); `icon_handler`'s embedded fallback now
      reads the `build.rs` table
- [x] Add `copy_path` / `move_path` / `unique_copy_dest` to the `src/fs` function table
- [x] Out of scope here, note for a future audit: §4's `web/dist/app.js` bullet still describes a
      `<pre>` preview and a tree rooted at `/`, both superseded by the text view and the home view

### Task 22: `README.md`

- [x] Add an HTTP surface table (all routes, method, purpose) and a WebSocket protocol section —
      server → `{"type":"changed","path"}`, client → `{"type":"watch","path"}`
- [x] Link to `ARCHITECTURE.md` for depth; leave the existing quick start untouched

### Task 23: Close out `AUDIT.md`

- [x] Append a "Resolution" section: which of the 8 action-plan items were applied, the corrected
      dependency math, and the measured before/after clean-build numbers
- [x] Record the decisions in the table at the top of this file

## Final verification

- [x] `cargo build` — zero warnings
- [x] `cargo clippy --all-targets` — zero warnings (17 → 0)
- [x] `cargo test` — all green (68 before, more after) — **93 passing** (88 unit + 5 integration)
- [x] `cargo build --timings` from clean; record wall + CPU against the 53.4 s / 325 s baseline
      — **47.7 s wall, 167.3 s CPU (150.5 user + 16.8 sys), 120 units.** Faster than both the audit's
      53.4/325 and the 67.52/268.75 baseline re-measured at session start; see AUDIT.md
- [x] `cargo tree -e no-dev --prefix none | sort -u | wc -l` against the 147 baseline — **110** (−37)
- [x] Confirm no C compiler is needed: `cargo clean && CC=/nonexistent/cc cargo build` — builds clean
- [x] Manual pass via `./run.sh --root <dir>` — **found and fixed a real bug: `run.sh` was missing the
      `--` separator, so the documented `./run.sh --root DIR` failed outright.** Verified over HTTP:
      open-with list identical to the pre-change crate build (71 apps), copy uniquify ×3, cut 409 on
      collision / 404 missing dest / 404 missing source, rename, delete, `delete /` → 400, icon
      grid SVGs `image/svg+xml`, `app.js` → `text/javascript`, properties, text preview
- [x] `cargo fmt --check` — clean (not in the original list; the refactors moved code across modules,
      so formatting is now part of the regression bar)
