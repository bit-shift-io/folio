# Investigation: Copy/Paste for folio

Grill-confirmed requirements (for the file-browser copy/cut/paste feature).

## Confirmed Behavior

- **Scope:** filesystem copy/cut/move of a single highlighted entry. Directories
  copy recursively; files copy byte-exact.
- **Selection:** operates on the single highlighted entry (`selectedFile`);
  multi-select is out of scope.
- **Paste target:** the directory the list pane currently shows (the
  `#path-input` location), never the highlighted folder.
- **Name collisions:**
  - Copy: auto-uniquify (`foo (copy).txt`, `foo (copy 2).txt`, …).
  - Cut/move: refuse with a clear error; the user renames or deletes first.
- **Clipboard lifecycle:** client-side state (single entry + cut flag).
  - A new copy/cut replaces the current clipboard.
  - A successful paste clears the clipboard.
  - ESC (or starting another copy/cut) cancels a pending cut without pasting.
- **Feedback:** subtitle shows `Copied: <name>` / `Cut: <name>` on keystroke and
  `Pasted → <dir>` on success; rows whose path is on a pending cut are dimmed in
  the list until pasted or cancelled.
- **Surface:** Ctrl+C / Ctrl+X / Ctrl+V / Esc in the panes; the preview ⋯
  dropdown (Properties/Rename/Delete) gains Copy & Cut; Paste stays keyboard-only
  since its target is the current directory, not the previewed file.

## Architecture Defaults (not user-verifiable, my call)

- One endpoint: `POST /paste { sources: [absolute path], dest: absolute dir, cut: bool }`
  (client holds the clipboard; the server stays stateless, matching "HTTP is truth").
  Reuse the existing mutation response pattern (200/400/404/409/500 JSON `{ok}`).
- Recursive copy preserves file permissions; symlinks are copied as links, never
  followed (consistent with `delete_path` in `src/fs/mod.rs`).
- Refuse moving a directory into its own subtree; refuse operations on `/`.
- Cross-device move falls back to copy + delete (std::fs::rename fails across
  devices otherwise).
- On any error mid-copy, clean up the partially written destination.
- After a successful paste, select the pasted result; pasting into the same
  directory as the source produces a duplicate (auto-uniquified).
- Keys: `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Esc`; insert before the existing
  `if (e.ctrlKey || e.metaKey || e.altKey) return;` guard in `boot()`
  (`web/dist/app.js`), after the pane/focus checks. No mac/metaKey support.

## Blocker Findings Inspected

- Fs layer already has `delete_path` / `rename_path` in `src/fs/mod.rs`; a
  `copy_path(sources, dest)` + guarded move will sit beside them.
- Mutation handlers and status-code shapes live in `src/server/mod.rs`
  (`rename_handler`, `delete_handler`) and the `⋯`/Rename/Delete flow in
  `web/dist/app.js` — the copy/paste endpoint and menu items mirror these.
- Mutations use `tokio::task::spawn_blocking`; recursive copy is blocking I/O and
  must do the same so the axum runtime stays responsive.

## Open Questions for the Implementing Session

- Whether the rename dialog/prompt is reused anywhere for paste collision fixes.
- `..` row / home-view boundary interaction with a paste into `/` (parent of children).