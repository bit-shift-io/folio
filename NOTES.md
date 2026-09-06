# Investigation: freedesktop-desktop-entry Integration for folio

## Current State
folio currently uses a custom .desktop file parser in `src/apps.rs` (lines 75-139) consisting of:
- `Fields` struct to hold parsed data
- `read_fields()` function that manually parses .desktop files
- Extracts: id (file path), name, exec, try_exec, icon, hidden/no_display/terminal flags, mime_types
- Implements filtering (Type=Application, visibility checks, flatpak detection)
- Handles executable resolution and launching logic separately

## Grit's Usage
grit uses freedesktop-desktop-entry (v0.8) in its Cargo.toml and primarily uses it in `src/actions.rs` to:
- Find terminal executors by parsing .desktop files with "TerminalEmulator" category
- Access fields like `exec()`, `try_exec()`, `name()`, `hidden()`, `no_display()`, `categories()`

## Analysis
### What freedesktop-desktop-entry likely provides:
Based on the freedesktop spec and grit's usage:
- `DesktopEntry::from_path(path, None)` to load files
- Accessors for standard fields: `exec()`, `try_exec()`, `name()`, `hidden()`, `no_display()`
- Access to categories via `categories()`
- **Very likely** provides MIME types access (standard MimeType key in .desktop files) - though grit doesn't use this feature since it's focused on terminals

### What would still be needed in folio:
Even with freedesktop-desktop-entry, folio would still require:
- Field code expansion (%f, %u, %F, %U, %c, %k, etc.) - the crate likely doesn't provide this
- Flatpak detection (checking if exec starts with "flatpak run")
- Executable resolution (PATH lookup, file checks)
- App launching logic (process spawning, detaching, etc.)
- The `AppEntry` struct and `open_with()` function would remain largely unchanged

## Recommendation
**Yes, folio should consider using freedesktop-desktop-entry to replace its custom .desktop file parsing.**

### Benefits:
1. **Eliminates ~65 lines of custom parsing code** (Fields struct + read_fields function)
2. **Uses a well-tested, standardized parser** - reduces edge case bugs in .desktop file handling
3. **Less maintenance** - benefit from upstream fixes and improvements
4. **Aligns with grit's approach** - potential for code sharing patterns
5. **Should provide clean MIME types access** - which folio needs for its "open with" functionality

### Implementation Approach:
1. Add `freedesktop-desktop-entry = "0.8"` to folio's Cargo.toml
2. Replace the `Fields` struct and `read_fields()` function with direct use of `DesktopEntry::from_path`
3. In `list_apps()`:
   - Use `DesktopEntry::from_path(path, None)` instead of `read_fields(&path)`
   - Check `entry.type_() == "Application"` (or equivalent)
   - Use `entry.hidden()`, `entry.no_display()` for filtering
   - Get `entry.name()` (with fallback to filename if empty)
   - Get `entry.exec()`, `entry.try_exec()`
   - Get `entry.mimetypes()` (or equivalent method)
   - Keep existing flatpak detection: `entry.exec().map(|e| e.starts_with("flatpak run")).unwrap_or(false)`
   - Keep existing executable resolution and AppEntry construction logic
4. Keep `split_exec()`, `substitute_codes()`, `expand_exec()`, and `open_with()` unchanged
5. Update tests as needed (though behavioral expectations should remain identical)

### Impact:
This change would be largely isolated to `src/apps.rs` and maintain the same external interface:
- `list_apps()` would still return `Vec<AppEntry>`
- `open_with()` would keep the same signature and behavior
- Server handlers (`/apps`, `/open`), config system, and other components would require **no changes**

The trade-off is adding a dependency for significant code simplification and improved correctness in .desktop file parsing - a worthwhile exchange given the complexity of properly handling the freedesktop desktop entry specification.