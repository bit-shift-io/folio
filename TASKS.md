# TASKS.md for freedesktop-desktop-entry integration

## Phase 1: Setup and Basic Integration

### Task 1: Add freedesktop-desktop-entry dependency
- [x] Add `freedesktop-desktop-entry = "0.8"` to Cargo.toml dependencies
- [x] Run `cargo check` to verify dependency resolves correctly

### Task 2: Update imports in src/apps.rs
- [x] Add `use freedesktop_desktop_entry::{DesktopEntry, Iter};` to imports
- [ ] Remove unused imports if any
- [x] Run `cargo check` to verify imports work

### Task 3: Replace Fields struct and read_fields function stub
- [x] Comment out Fields struct and read_fields function (preserve as reference)
- [x] Add TODO markers for replacement
- [x] Run `cargo check` to verify code still compiles (will fail on usage, expected)

## Phase 2: Implement list_apps with freedesktop-desktop-entry

### Task 4: Implement basic desktop entry loading in list_apps
- [ ] Replace read_fields call with DesktopEntry::from_path
- [ ] Handle Err case by continuing (same as before)
- [ ] Run `cargo check` to verify basic loading works

### Task 5: Implement filtering logic using DesktopEntry methods
- [ ] Replace Fields.type_ check with desktop_entry.type_()
- [ ] Replace Fields.hidden check with desktop_entry.hidden()
- [ ] Replace Fields.no_display check with desktop_entry.no_display()
- [ ] Replace Fields.terminal check with desktop_entry.terminal()
- [ ] Run `cargo check` to verify filtering compiles

### Task 6: Implement flatpak detection
- [ ] Add flatpak check using desktop_entry.exec()
- [ ] Maintain same logic as before (skip if exec starts with "flatpak run")
- [ ] Run `cargo check` to verify flatpak detection works

### Task 7: Implement executable resolution
- [ ] Replace Fields.exec and Fields.try_exec usage with desktop_entry methods
- [ ] Keep existing split_exec and resolvable logic unchanged
- [ ] Run `cargo check` to verify executable resolution works

### Task 8: Implement AppEntry construction
- [ ] Replace Fields.name usage with desktop_entry.name() with fallback
- [ ] Replace Fields.mime_types usage with desktop_entry.mimetypes()
- [ ] Keep AppEntry struct construction identical
- [ ] Run `cargo check` to verify AppEntry construction works

### Task 9: Verify list_apps completes and compiles
- [ ] Ensure all error handling is proper
- [ ] Run `cargo check` to verify entire function compiles
- [ ] Run `cargo test` to verify existing tests still pass

## Phase 3: Implement open_with with freedesktop-desktop-entry

### Task 10: Update open_with to use DesktopEntry
- [ ] Replace read_fields call with DesktopEntry::from_path
- [ ] Handle Err case by returning LaunchError::NoExec
- [ ] Run `cargo check` to verify basic loading works

### Task 11: Update exec handling in open_with
- [ ] Replace Fields.exec usage with desktop_entry.exec()
- [ ] Handle None case by returning LaunchError::NoExec
- [ ] Run `cargo check` to verify exec handling works

### Task 12: Verify open_with completes and compiles
- [ ] Ensure all error handling is proper
- [ ] Run `cargo check` to verify entire function compiles
- [ ] Run `cargo test` to verify existing tests still pass

## Phase 4: Cleanup and Finalization

### Task 13: Remove obsolete code
- [ ] Completely remove Fields struct and read_fields function
- [ ] Remove is_flatpak function (inline the check)
- [ ] Verify no unused imports remain
- [ ] Run `cargo check` to verify code compiles without old code

### Task 14: Final test run
- [ ] Run full test suite: `cargo test`
- [ ] Ensure all tests pass
- [ ] Run integration tests specifically: `cargo test --tests`

### Task 15: Verify functionality manually (if possible)
- [ ] Run folio with a test directory
- [ ] Verify /apps endpoint returns expected applications
- [ ] Verify /open endpoint works for known applications