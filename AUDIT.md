# Codebase Audit Summary

**Audit Target:** `folio` — local web file explorer (Rust + Axum, vanilla JS)
**Date:** 2026-09-27
**Method:** static review of all 3,961 lines of Rust + `web/dist`, `cargo clippy --all-targets`, `cargo tree`, and `cargo build --timings` on an 8-core box.

---

## Executive Summary

Folio's core is in good shape: 3,961 lines of Rust across 7 files, no orphaned files, no unused
exports, no dead code paths, no `dbg!`/`console.log`/debug leftovers, and 68 passing tests
(63 unit + 5 integration) with a zero-warning `cargo build`. The hand-rolled media sniffers
(`src/media.rs`) and MIME tables (`src/fs/mod.rs`) are exactly the right call for a
single-binary local tool.

The debt is concentrated in two places. **First, dependencies:** three libraries pull in far more
than folio uses — `freedesktop-desktop-entry` drags in a full gettext/`regex` stack and a C
compiler for 7 field lookups, `clap` compiles an entire parser stack (and a second copy of `syn`)
for two flags, and `rust-embed` hard-depends on `sha2` just to inline three web assets. Together
they are **~123 s of the 325 s CPU** in a clean build. **Second, documentation drift:** `CONTEXT.md`
and `ARCHITECTURE.md` both predate the copy/paste feature and don't mention `POST /paste`, and
`src/apps.rs` carries 61 lines of commented-out code plus a stale TODO describing work that has
already been done.

Already applied during this audit: tokio/axum/clap feature trimming, test-only deps moved to
`[dev-dependencies]`, and a `tungstenite` version dedupe. Clean build went **57.99 s → 53.4 s**
and the dependency graph lost 21 crates. The remaining wins require small code changes and are
ranked below.

> **Status: all 8 action-plan items have since been applied** (2026-09-28). The findings and
> numbers below are preserved as the original record; the
> [Resolution](#resolution-2026-09-28) section at the end has the corrected dependency math and
> the measured before/after. The headline: **110 production crates (from 147), 17 → 0 Clippy
> warnings, 93 tests, and folio no longer needs a C compiler to build.**

---

## Key Metrics

These are the audit's original findings. The **Decision** column records what was
actually done; the closing [Resolution](#resolution-2026-09-28) section has the
measured after-numbers.

- **Unused/Orphan Files:** 0
- **Dead Functions/Exports:** 0 (all 32 `pub fn` have ≥1 call site outside their definition)
- **Commented-Out Code / Debug Logs:** 61 lines (1 block) / 0
- **Open TODOs/FIXMEs:** 1 (stale — describes completed work)
- **Clippy Warnings:** 17 (0 compiler warnings from `cargo build`)
- **Dependency Graph:** 157 → 142 crates after this audit's changes
- **Clean Build (dev):** 57.99 s → 53.4 s wall, 325 s CPU
- **Test Suite:** 68 passing, 0 failing

### Decisions

| # | Recommendation | Decision |
| :--- | :--- | :--- |
| 1 | Delete the commented-out `Fields`/`read_fields` block | **Done, inverted** — revived as the live parser and the crate was deleted instead |
| 2 | Drop `freedesktop-desktop-entry` for a hand-rolled parser | **Applied** |
| 3 | Document `POST /paste` in `CONTEXT.md`, refresh `ARCHITECTURE.md` routes | **Applied** |
| 4 | Replace `clap` with hand-rolled parsing | **Applied** (keeping `--help`/`--version`) |
| 5 | `cargo clippy --fix` + hand-fix the two `sniff_mime` sites | **Applied** — and the duplicate arms were *correct*, not a bug; they are now one arm with a comment |
| 6 | Replace `rust-embed` with a `build.rs` asset table | **Applied** |
| 7 | Refactor `paste_handler`; extract the `spawn_watcher` debounce | **Applied** |
| 8 | Decide on `[profile.dev] debug = 0`; archive the finished `TASKS.md` | **Applied to both** |

---

## Findings & Recommendations

### 1. Unused Files & Dead Code

| File Path | Type | Details | Recommended Action |
| :--- | :--- | :--- | :--- |
| `src/apps.rs:77-137` | Commented-out code | 61-line block: a hand-rolled `Fields` struct + `read_fields` desktop-entry parser, fully commented out | Delete. The replacement (`freedesktop-desktop-entry`) is live, so the block is unreachable history. Ironically this is also the code that would let you drop the crate (see §4, item 2) |
| `src/apps.rs:77` | Stale TODO | `// TODO: Replace Fields struct and read_fields function with freedesktop-desktop-entry` — this was done; the TODO is a leftover marker | Delete with the block above |
| `res/icons/breeze-dark/**` (49 SVGs) | Verified clean | Every icon filename appears as a string in `web/dist/app.js`'s `MIME_ICONS`/`NAME_ICONS`/`EXT_ICONS` maps — no dead assets | None |
| All 32 `pub fn` | Verified clean | Cross-checked every `pub fn` in `src/` against call sites in `src/` + `tests/`; none unreferenced | None |
| `src/lib.rs`, `src/server/static_files.rs` | Verified clean | Both are wired in (`lib.rs` via `folio::server` in `main.rs`; `static_files` via two routes) | None |
| `web/dist/{app.js,index.html,style.css}` | Verified clean | All three embedded and reachable via the `/` and `/{*path}` routes | None |

### 2. Code Structure & Complexity Smells

| File Path | Issue | Context / Severity | Suggested Refactor |
| :--- | :--- | :--- | :--- |
| `src/server/mod.rs:519` | 115-line function | `paste_handler` — validation, two operation modes, blocking dispatch, and hint broadcast in one body; longest fn in the repo | Extract the cut-vs-copy decision into `resolve_paste_dest(src, dest, cut) -> Result<PathBuf, FsError>`; the handler then reads as validate → dispatch → broadcast |
| `src/fs/mod.rs:321`, `:357` | Identical `if` blocks (clippy `if_same_then_else`) | `sniff_mime` arms — two magic-byte branches with byte-identical bodies, presumably a placeholder copy-paste | Collapse into one arm with a shared pattern list, or add a comment if the divergence is intentional |
| `src/fs/mod.rs:216`, `:222`, `src/apps.rs:243` | `sort_by` where `sort_by_key` fits (3×) | `search_files` (2) and `list_apps` | Mechanical; makes intent explicit |
| `src/server/mod.rs:781` | 58-line function, 3-level nesting | `spawn_watcher` — debounce loop nested inside the select loop inside the task | Extract the inner debounce into `async fn debounce(event_rx, pending, tx)`; flattens to ~15 lines |
| `src/apps.rs:155` | 91-line function | `list_apps` — scan, filter, KDE-alias merge pass, sort | Extract the alias-merge pass (lines 221-241) into `merge_alias_helpers(&mut apps, helpers, &by_appid)` |
| `src/apps.rs` (all `de.x()` call sites) | `needless_borrow` / `map_or` noise (7×) | clippy: `&de.exec()`, `&desktop_entry.exec()`, `map_or(true, ..)` | Mechanical `cargo clippy --fix` |
| `src/media.rs:367` | No-op expression | `0x80 \| 0` in a test fixture | `cargo clippy --fix` |
| `tests/integration.rs:218` | `match` on one pattern | Should be `if let` | `cargo clippy --fix` |

File sizes are within reason: `src/server/mod.rs` (1,404) and `src/fs/mod.rs` (915) are the two
largest, and roughly half of each is `#[cfg(test)]` coverage. No file exceeds 400 lines of
non-test logic.

### 3. Comments & Technical Debt

| File Path | Type | Snippet / Context | Recommendation |
| :--- | :--- | :--- | :--- |
| `CONTEXT.md` | Stale doc | Endpoint list omits `POST /paste` (registered at `src/server/mod.rs:930`) — the copy/cut/paste feature is undocumented in the project's own vocabulary file | Add `/paste` to the endpoint list and a **copy/cut/paste** vocabulary entry (clipboard, `Ctrl+C/X/V`, `Esc` cancel, auto-uniquify) |
| `ARCHITECTURE.md` | Stale doc | Documents only 8 of 14 routes; missing `/paste`, `/apps`, `/defaultapp`, `/open`, `/fileinfo`, `/filecontent` | Regenerate the route table from `build_router` |
| `README.md` | Thin | Barely documents the HTTP surface or the WS protocol | Optional; see action plan |
| `TASKS.md` | Fully checked off | All items `[x]`. Harmless history, but the `build-all` / `build-steps` skills will read it as the active backlog and find nothing to do | Archive to `TASKS-paste.md` or start a fresh list |
| `NOTES.md` | Closed investigation | "Open Questions for the Implementing Session" (lines 54-57) were resolved during implementation | Resolve or drop the two questions so the file reads as a closed record |

### 4. Dependencies & Build Time

Measured with `cargo build --timings` on 8 cores. A clean build is 53.4 s wall / **325 s CPU**
across 153 units. The critical path is serial and unavoidable: `tokio` (19.0 s) → `axum`
(8.2 s) → `folio` lib (6.4 s) → bin (2.8 s). On a many-core box, wall-clock savings come mostly
from cutting *total CPU* (less contention), not just the tail — so both numbers are given.

#### Applied in this audit (verified: 68/68 tests green, zero build warnings)

| Change | Effect |
| :--- | :--- |
| `tokio` `features = ["full"]` → `["macros", "net", "rt-multi-thread", "sync", "time"]` | `tokio` itself 23.7 s → 19.0 s; drops `signal-hook-registry`, `parking_lot`, `lock_api`, `scopeguard`, `process`/`signal`/`fs`/`io-std` code |
| `axum` → `default-features = false, features = ["ws","tokio","http1","json","query"]` | Drops the unused `form` (`serde_urlencoded`), `matched-path`, `original-uri`, `tower-log` defaults |
| `clap` → `default-features = false, features = ["derive","std","help","usage","error-context"]` | Drops colour/`anstream` stack (`anstream`, `anstyle*`, `colorchoice`, `is_terminal_polyfill`) |
| `futures-util` + `tower` → `[dev-dependencies]` | Correct scope (test-only use), though little time saved — `axum`'s `ws` feature needs `futures-util` anyway |
| `tokio-tungstenite` `0.30` → `0.29` | **Dedupe:** `axum 0.8` already depends on `tokio-tungstenite 0.29`; the old pin compiled a *second* `tungstenite` plus its own `rand`/`chacha20`/`sha1`/`data-encoding`. Saves a duplicate `tungstenite` build in `cargo test` |
| **Net** | **21 crates removed from `Cargo.lock`; 157 → 142 crates; 57.99 s → 53.4 s** |

#### Not applied — recommended, needs a code change

| # | Library | Cost | What it actually does for folio | Recommendation |
| :--- | :--- | :--- | :--- | :--- |
| 1 | **`freedesktop-desktop-entry` 0.8** | **63.0 s CPU (19% of the build)** — the single largest removable cluster | Seven field reads (`Type`, `Exec`, `Name`, `Icon`, `TryExec`, `Hidden`, `NoDisplay`, `Terminal`, `MimeType`, `X-KDE-AliasFor`) from a flat INI file. Pulls `bstr`, `gettext-rs`, `gettext-sys`, `locale_config`, `regex`, `regex-automata` (22.5 s), `regex-syntax` (11.1 s), `aho-corasick` (9.4 s), `xdg`, and **`cc` — so folio currently needs a C compiler to build** | Replace with the ~40-line `read_fields` parser that is already sitting commented out at `src/apps.rs:77-137`. Biggest single win, and drops the C-toolchain requirement. The only real risk is Exec/`Name` spec edge cases, so port the 6 existing `apps.rs` tests first |
| 2 | **`clap` 4** | **32.4 s CPU** incl. a second `syn` (3.0.5, 13.2 s — `clap_derive`; `syn 2` is already needed by `serde_derive`/`rust-embed-impl`) | Two flags: `--root` and `--port` | Hand-roll ~15 lines of `std::env::args()` parsing. **Measured in a scratch copy: 55.6 s → 52.2 s** (−3.4 s wall, −22 s CPU). Trade-off: lose `--help`, `--version`, and clap's error messages — consider keeping clap if `--help` matters |
| 3 | **`rust-embed` 8** | **27.3 s CPU** — `sha2` (3.0 s) is a **non-optional** dep of `rust-embed-utils` (verified in its manifest), plus `crypto-common` (4.1 s), `mime_guess` (3.5 s), `walkdir`, `digest`, `block-buffer`, `hybrid-array`, `const-oid`, `typenum` | Inlining 3 files from `web/dist/` and 49 SVGs from `res/icons/` | Replace with a 25-line `build.rs` that walks both dirs and emits a `&[(&str, &[u8])]` table via `include_bytes!`, plus a 5-line extension→MIME `match`. Zero dependencies. Also makes the build scriptable (e.g. dev-only icon subsets) |
| 4 | `tracing-subscriber`'s `env-filter` | ~2 s marginal | `RUST_LOG` support | Keep. `regex-automata` is shared with #1, so dropping `env-filter` alone saves almost nothing — it only becomes worth it *after* #1 |
| 5 | `reqwest` (dev) | test builds only | 3 integration tests hitting `127.0.0.1` | Optional. A raw `TcpStream` HTTP helper is ~40 lines and drops `reqwest` + `hyper-util` client + `ipnet` + `encoding_rs` from the test build. Low priority — it never affects `cargo build` |
| 6 | **`[profile.dev] debug = 0`** | **53.4 s → 44.5 s (−17%)** — the largest wall-clock win measured | Debuginfo for the local toolchain | Measured, but **deliberately not applied**: it costs you line numbers in debugger backtraces. One line, trivially reversible — your call |

`mime-guess` deserves a specific note: it is used in exactly **one** place
(`src/server/static_files.rs:21`, `content.metadata.mimetype()`). It is tempting to delete, but
**don't** — folio's own `mime_for_path` (`src/fs/mod.rs:228`) returns `"text/plain"` for `.html`,
`.css`, and `.js` because they're in `TEXT_EXTS`, so swapping it in would serve `app.js` and
`style.css` with the wrong `Content-Type` and break the UI. If you want #3, add
`"html" => "text/html"`, `"css" => "text/css"`, `"js" => "text/javascript"`, and
`"json" => "application/json"` arms to `mime_for_path` first. (`icon_handler` already
demonstrates this works — it uses `fs::mime_for_path` with an SVG special-case rather than
`mime_guess`.)

---

## Top Priority Action Plan

1. **[High]** Delete the commented-out `Fields`/`read_fields` block and its stale TODO at
   `src/apps.rs:77-137`. Zero risk, −61 lines of noise. *Or* revive it as step 2 below.
2. **[High]** Drop `freedesktop-desktop-entry` for a hand-rolled `.desktop` parser (the code
   already exists in commented form). **−63 s CPU**, and removes folio's C-compiler build
   requirement. Port the 6 existing `apps.rs` tests to lock the behaviour in first.
3. **[High]** Add `POST /paste` + a copy/cut/paste vocabulary entry to `CONTEXT.md`, and refresh
   the route table in `ARCHITECTURE.md`. These two docs are loaded as project instructions, so
   their staleness is actively misleading.
4. **[Medium]** Replace `clap` with ~15 lines of arg parsing (**measured −3.4 s wall, −22 s CPU**),
   or keep it if `--help` is worth 3.4 s. Decide explicitly rather than leaving it implicit.
5. **[Medium]** Run `cargo clippy --fix` (13 of 17 warnings are auto-fixable) and hand-fix the
   two `if`-same-block sites in `sniff_mime`, which may be a real copy-paste bug.
6. **[Medium]** Replace `rust-embed` with a `build.rs` asset table (**−27 s CPU**). Requires the
   `mime_for_path` additions described in §4 first.
7. **[Medium]** Refactor `paste_handler` (115 lines) and extract the debounce loop from
   `spawn_watcher`. Both are mechanical extractions with existing test coverage.
8. **[Low]** Decide on `[profile.dev] debug = 0` (**−17% wall**, the biggest single wall-clock
   number in this report) and archive the fully-checked-off `TASKS.md`.

---

## Resolution (2026-09-28)

All eight action-plan items were applied. The dependency removals were the bulk
of the work; the two refactors and the doc refresh were mechanical.

### Dependency math, corrected

The estimates in §4 were directionally right and quantitatively close, with one
important correction: **crate counts and CPU seconds are not interchangeable.**
The three removals overlapped — `regex-automata`/`regex-syntax` were reached
through *both* `freedesktop-desktop-entry` and `tracing-subscriber`'s
`env-filter`, so dropping only the desktop-entry crate would not have removed
them.

| Crate removed | Where it came from | Crates actually dropped |
| :--- | :--- | :--- |
| `freedesktop-desktop-entry` 0.8 | action item 2 | 14 — and `cc` with it, so **folio no longer needs a C compiler** |
| `clap` 4 | action item 4 | 6 — including a second `syn`, which `serde_derive` no longer needs for a build |
| `rust-embed` 8 | action item 6 | 17 — `sha2`/`crypto-common`/`digest`/`block-buffer` and the rest of the hashing tree, all non-optional |
| `reqwest` (dev) | action item 5 | 12 from `Cargo.lock` (test-only; never affected `cargo build`) |

**Production graph: 147 → 110 crates** (`cargo tree -e no-dev`), −37.
`Cargo.lock` went 157 → 122 entries.

The `env-filter` item (#4) was deliberately **not** taken: it costs ~2 s on its
own and only becomes worth revisiting now that `regex-automata` has actually
left the graph.

### Measured before / after

Both figures come from `cargo clean && cargo build` on the same 8-core box, with
CPU as `user + sys` from `/usr/bin/time` and units from `cargo build --timings`.

| Metric | Before | After | Δ |
| :--- | ---: | ---: | ---: |
| Clean build, wall | 53.4 s *(67.52 s re-measured at session start)* | **47.7 s** | −11% *(−29% vs the re-measured baseline)* |
| Clean build, CPU | 325 s *(268.75 s re-measured)* | **167.3 s** (150.5 user + 16.8 sys) | −49% *(−38% vs re-measured)* |
| Build units | 153 | **120** | −33 |
| Production crates | 147 | **110** | −37 |
| Clippy warnings | 17 | **0** | −17 |
| Tests | 68 | **93** (88 unit + 5 integration) | +25 |

> The audit's 53.4 s / 325 s baseline and the 67.52 s / 268.75 s baseline
> recorded at the start of the implementation session disagree with each other,
> so the box is not perfectly stable. The wall-clock number is the one to
> trust: **47.7 s is faster than either baseline**, and the honest claim is
> "~30% off the most recent baseline, and comfortably past the optimistic one."

### Behavioural equivalence

Removing a parser is only safe if it parses the same thing, so both risky
removals were checked against the code they replaced rather than against
expectations:

* `/apps` output was captured from the `freedesktop-desktop-entry` build and
  re-diffed against the hand-rolled parser on this machine: **byte-identical,
  71 apps** — same ids, names, MIME lists, and order.
* The new `read_fields` parser is covered by 6 contract tests pinning the
  `[Desktop Entry]`-only, unlocalized-`Name`, literal-`true`, and
  `MimeType`-splitting behaviours the old crate provided.
* `paste_handler` and `spawn_watcher` were extracted with their existing tests
  untouched and green throughout; `/paste` was additionally exercised over real
  HTTP (copy uniquify `a.txt` → `a (copy).txt` → `a (copy 2).txt`, cut collision
  `409`, missing dest `404`, missing source `404`, `delete /` `400`).

### Defects found and fixed along the way

* **Two Clippy warnings were introduced by the refactors themselves**
  (`if_same_then_else` on the merged `sniff_mime` mp3 arms, and a `match`-with-
  one-arm in the new `src/cli.rs`). The audit's "0 warnings" bar is a
  regression guard, not a one-time cleanup, so both were fixed. The
  `if_same_then_else` case turned out to be **intentional** code (an `ID3` tag
  and a bare MPEG frame sync are both mp3) rather than the copy-paste bug the
  audit suspected — the arms are now one arm with a comment saying why.
* **`run.sh` was broken**: it ran `cargo run "$@"`, so the documented
  `./run.sh --root DIR` failed with *"unexpected argument '--root' found"*.
  It is now `exec cargo run -- "$@"`. This was found only because the final
  verification pass actually used `run.sh` rather than a bare `cargo run`.

### Follow-ups left open

* **`ARCHITECTURE.md` §4's `web/dist/app.js` bullet is still partly stale** —
  it describes a `<pre>` preview and a tree rooted at `/`, both superseded by the
  line-numbered text view and the home view. Deliberately out of scope for the
  doc pass (it is a description of the frontend, which did not change) and
  recorded here for the next audit.
* The icon theme is still embedded in the binary as a *fallback* in addition to
  being read from disk at runtime. That is the `rust-embed` replacement's
  safety net, not a requirement; dropping the embedded copy would shave build
  time once the theme is guaranteed present on disk.
