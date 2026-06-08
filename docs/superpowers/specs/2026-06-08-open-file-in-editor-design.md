# Open the selected change's file in `$EDITOR`

**Date:** 2026-06-08
**Status:** Approved

## Goal

From the chronox-tui change viewer, let the user press a key to open the
currently-selected change's file in their editor, positioned at the changed
line. Editor-agnostic: nvim, vim, nano, emacs, etc.

## Approach (lightest weight)

Resolve the editor from the universal Unix convention — `$VISUAL`, then
`$EDITOR`, falling back to `vi`. **No new config schema**; chronox core is
untouched. Position with the `+N file` convention (vim-family / nano / emacs).

## Design

### Keybinding (`input.rs`)
- `e` -> new `AppAction::OpenInEditor`, active in both List and Diff focus.
  `e` is currently unbound.

### Action plumbing (`app.rs`)
- Add `AppAction::OpenInEditor`. It is a side-effecting action handled by the
  run loop (which owns the terminal), so `App::apply` carries only a no-op arm
  for exhaustiveness.
- Add accessor `selected_path_and_line(&self) -> Option<(PathBuf, u32)>`. It
  reuses the same `load_full_change(ev).unwrap_or(ev.detail)` +
  `resolve_line_in_file` path that backs the diff view, so the editor lands on
  the line the diff shows. `resolve_line_in_file` returns 1 when the file is
  unreadable, so the line is always >= 1.
- Add a transient `status: Option<String>` plus `set_status`/`clear_status`,
  surfaced in the footer and dismissed on the next keypress.

### Editor launch (`main.rs`)
- Refactor the terminal lifecycle into `enter_screen()` / `leave_screen()`
  (raw mode + alternate screen + mouse capture) so both startup and the
  suspend/resume path share them.
- Pure builder `editor_command(env_val: Option<&str>, line: u32, path: &Path)
  -> (String, Vec<String>)`:
  - empty/`None` -> `vi`; otherwise split on whitespace (first token = program,
    rest = leading args, so `EDITOR="code -w"` works).
  - append `+{line}` then the path.
- `run()` intercepts `OpenInEditor`: `leave_screen` -> spawn editor with
  inherited stdio and wait -> `enter_screen` + `terminal.clear()`. On spawn
  failure, set the transient status; otherwise clear it. All other actions go
  through `app.apply` as before. Any keypress clears a prior status first.

### Footer (`ui.rs`)
- Add `· e edit` to both focus hints.
- When `status` is set, render it (DIM) in place of the hint.

## Testable seams
- `input.rs`: `e` maps to `OpenInEditor`.
- `app.rs`: `selected_path_and_line` returns the expected `(path, 1)` for a
  seeded event; `None` when there are no events.
- `main.rs`: `editor_command` — `vi` fallback, single binary, embedded args,
  whitespace trimming.

The spawn / terminal dance stays thin and is left untested (pure I/O).

## Known limitation (accepted)
`+N` targets vim-family / nano / emacs / `vi`. A non-`+N` editor (e.g. VS Code)
would mishandle the line arg. A per-editor mapping can be added later if needed.
