# TASKS.md for copy/cut/paste (per NOTES.md)

Client-side clipboard (single `{path, cut}`) + server `POST /paste`. Copy auto-uniquifies
(`foo (copy).txt`, `foo (copy 2).txt`); cut refuses collisions; ESC cancels a pending cut.

## Phase 1: Filesystem layer — src/fs/mod.rs

### Task 1: Recursive copy with permission + symlink handling
- [x] Add `pub fn copy_path(src: &Path, dest: &Path) -> Result<(), FsError>`: files copy byte-exact with perms via `set_permissions`; dirs recurse (create dirs first, then children); symlinks recreated with `std::os::unix::fs::symlink`, never followed; on any error remove the partially built dest and return the io error
- [x] Add unit tests: byte-exact file, recursive dir preserving structure, symlink copied as a link (target intact, not dereferenced), permissions preserved
- [x] Run `cargo test`

### Task 2: Copy destination uniquifier
- [x] Add `pub fn unique_copy_dest(dest_dir: &Path, name: &str) -> PathBuf`: if `dest_dir/name` is free return it, else `name (copy).ext`, `name (copy 2).ext`, … (existence-checked; handles names without an extension)
- [x] Add unit tests: no collision, one collision, several collisions
- [x] Run `cargo test`

### Task 3: Guarded move with cross-device fallback
- [x] Add `pub fn move_path(src: &Path, dest: &Path) -> Result<(), FsError>`: refuse `src == "/"`; refuse moving a directory into its own subtree; try `std::fs::rename`; on `EXDEV` (cross-device) fall back to `copy_path` + `delete_path` of src
- [x] Add unit tests: plain move within a dir; moving a dir into its own subtree refused (`InvalidInput`); renaming `/` refused
- [x] Run `cargo test`

## Phase 2: Server endpoint — src/server/mod.rs

### Task 4: Paste handler + route
- [x] Add `PasteRequest { sources: Vec<String>, dest: String, cut: bool }` and `PasteResponse { ok: bool, path: Option<String> }` (serde derive)
- [x] Add `paste_handler`: reject empty `sources`/`dest` and `source == "/"` (400); resolve absolute via `resolve_path`; missing source → 404; copy → dest = `unique_copy_dest(dest_dir, name)`; cut → refuse if dest exists (409) and refuse dir-into-own-subtree (400); run in `tokio::task::spawn_blocking`; on success broadcast hints (source parent when cut + dest dir) and return 200 with the final path in `path`
- [x] Register `.route("/paste", post(paste_handler))` in `build_router`
- [x] Run `cargo check`

### Task 5: Server integration tests
- [x] Add tests in src/server/mod.rs: copy file → 200 + content present + `path` set; copy collision → uniquified path returned; cut → moved, source gone; cut collision → 409; cut dir into own subtree → 400; missing source → 404; missing dest → 404/400 as mapped
- [x] Run `cargo test`

## Phase 3: Client — web/dist/app.js + style.css

### Task 6: Clipboard state, actions, menu items
- [x] Add `let clipboard = null;` holding `{path, cut}`; `clearClipboard()`; `copyEntry(path)` sets clipboard, subtitle `Copied: <name>`, re-renders list; `cutEntry(path)` sets clipboard with `cut: true`, subtitle `Cut: <name>`, dims matched rows
- [x] Add `pasteClipboard()`: POST `/paste` `{sources:[clipboard.path], dest: currentDir, cut: clipboard.cut}`; on success clear clipboard, subtitle `Pasted → <dir>`, refetch (reuse `onFileChanged`), `selectFile(result.path)`; on 409 and other failures show a clear message
- [x] Add `Copy` and `Cut` buttons to the preview ⋯ dropdown (before Rename) operating on the previewed path; Paste stays keyboard-only
- [x] Toggle `pending-cut` class on list rows whose path matches a pending cut (refresh in `renderFileList`)
- [x] Review by eyeballing; no JS test runner in this repo

### Task 7: Keybindings in boot()
- [x] Before the `if (e.ctrlKey || e.metaKey || e.altKey) return;` guard in `boot()`: `Ctrl+C` → `copyEntry(selectedFile)`, `Ctrl+X` → `cutEntry(selectedFile)`, `Ctrl+V` → `pasteClipboard()` (each `e.preventDefault()`; no-op when `selectedFile`/clipboard missing); `Escape` → cancel pending cut (clear clipboard + dimming) when no modifier held
- [x] Keep existing INPUT/TEXTAREA early-return and pane/focus behavior intact
- [x] Manual key review

### Task 8: Dimmed pending-cut style
- [x] Add `.tree-item.pending-cut { opacity: 0.45; }` (dimmed rows) to web/dist/style.css
- [x] Confirm no existing rule conflicts

## Final verification
- [x] `cargo build` with zero warnings
- [x] `cargo test` all green
- [x] Manual pass via `./run.sh --root <dir>`: copy/cut/paste, collision-uniquify, collide-cut refusal, ESC cancel