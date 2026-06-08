# chronox-tui — design

A standalone ratatui application that renders the [chronox](https://github.com/bakedbean/chronox)
change-timeline frontend: a live, master-detail terminal UI for browsing the
file changes a Claude Code agent made in a worktree, with a syntax-highlighted
diff of the selected change.

## Purpose

chronox is a library crate. It provides every building block for the UI — a
parsed `Timeline` of change events, a pure `nav` cursor state machine, and
ratatui render helpers (`entry_lines` for bar rows, `change_detail_lines_styled`
for the diff) — but it ships **no runnable app**; its only binary is an example
that prints to stdout. This project is the missing piece: the interactive
terminal program that wires those building blocks into a real tool you run to
watch and inspect an active (or finished) Claude Code session.

This is built as a **standalone daily-use tool**, not a demo. It is also the
crate's reference consumer.

## Scope

### In scope (v1)
- Master-detail TUI: timeline list (left) + live diff of the selected change (right).
- Target worktree = current directory, overridable by a single path argument.
- Live polling: the timeline refreshes on a tick so new changes appear while a
  Claude Code session is running.
- Tab-toggled focus between the list and diff panes; diff scrolling.
- Mouse-draggable split between the two panes, with keyboard parity.
- Empty/error states rendered as screens, never crashes.

### Out of scope (v1 — YAGNI)
- chronox's `config` module (`Side` / `WidthSpec` bar placement) — that exists
  for the bar embedded inside wsx; our layout is fixed.
- In-app worktree picker (CWD/arg only).
- Persisting the split width across runs (always start from the default).
- Search / filter, theming, and any mouse interaction beyond resize + wheel-scroll.

## Dependencies & approach

- **ratatui 0.29 + crossterm** (ratatui's default backend). This matches the
  `ratatui = "0.29"` that chronox renders against, so the styled `Line` values
  chronox produces drop straight into our buffer with no version bridging.
- **chronox** via git dependency, default features on (we need the `render`
  module). Pin to a commit `rev` for reproducible builds.
- **No async runtime.** A synchronous loop built on
  `crossterm::event::poll(timeout)` handles both input and the live-poll tick in
  one place. A poll-on-tick TUI does not need tokio.

## Architecture

Small, single-purpose modules with clear boundaries:

```
src/
  main.rs   — CLI arg parsing (worktree path; default CWD), terminal setup/teardown
              (raw mode, alternate screen, mouse capture), panic-safe restore,
              run the event loop, propagate errors out of main.
  app.rs    — App state + pure transitions. No ratatui draw calls. Unit-testable.
  ui.rs     — draw(frame, &mut app): layout + widgets only. Reads App; the only
              mutation it performs is recording last_area for mouse hit-testing.
  input.rs  — map a crossterm Event (key or mouse) → an AppAction. All key
              bindings and the divider hit-test live here in one table.
```

The split between `app.rs` (state/logic) and `ui.rs` (rendering) is the key
boundary: app transitions can be tested with no terminal, and the draw layer can
change without touching logic.

## App state (`app.rs`)

```rust
enum Focus { List, Diff }

enum AppAction {
    Quit,
    Nav(chronox::NavKey),   // Up / Down / Top / Bottom / Enter / Esc
    ToggleFocus,            // Tab
    ScrollDiff(i32),        // +/- lines (PgUp/PgDn, wheel)
    NudgeSplit(i32),        // [ and ]  → -1 / +1 column
    StartResize,            // mouse down on divider
    Resize(u16),            // mouse drag → absolute target column
    EndResize,              // mouse up
    Tick,                   // poll timeout: refresh the timeline
    None,
}

struct App {
    worktree: PathBuf,
    session_files: Vec<PathBuf>,        // from claude_session_files(), refreshed each tick
    timeline: chronox::Timeline,        // crate cache; refresh() reparses only changed files
    selected: usize,
    focus: Focus,
    diff_scroll: usize,
    list_width: u16,                    // resizable left-pane width; default 32
    resizing: bool,                     // true during a divider drag
    last_area: Rect,                    // previous frame's full draw area (mouse hit-testing)
    last_visible_rows: usize,           // list rows visible last frame, for adjust_scroll
    diff_cache: Option<(usize, Vec<Line<'static>>)>,  // (selected idx, styled diff lines)
    should_quit: bool,
}
```

### Constants
- `DEFAULT_LIST_WIDTH: u16 = 32`
- `MIN_LIST: u16 = 16`
- `MIN_DIFF: u16 = 24`
  (`list_width` is always clamped to `[MIN_LIST, area.width - MIN_DIFF - 1]`,
  the `-1` reserving the separator column.)
- Poll timeout ≈ 250 ms; timeline refresh throttled to ≈ once per second.

### Key behaviors

**Reusing the crate, not reimplementing it.** List movement goes through
chronox's `nav()`, `adjust_scroll()`, and `clamp_scroll()`. Bar rows come from
`entry_lines()` + `clip_line_to_width()`. The diff comes from
`change_detail_lines_styled()`. We add only the app shell around these.

**Selection stability across live refresh.** New changes land at the **top** of
the newest-first timeline, which would shift a positional `selected` index onto a
different event. Before each `timeline.refresh()`, capture the selected event's
`source` (`session_file` + `line_index` + `index_in_line`) as a stable identity.
After refresh, re-find that event and set `selected` to its new index. If it is
gone (e.g. log rewritten), clamp `selected` to the new bounds. This keeps the
cursor pinned to the change the user is looking at while the list grows above it.

**Diff for the selected change.** Resolve lazily and cache by selected index:
1. `load_full_change(ev)` to re-read the full, un-clipped diff from the session
   log; fall back to `ev.detail` on `None`.
2. `resolve_line_in_file(&ev.file_path, &detail)` for the base line number.
3. `change_detail_lines_styled(&detail, base, lang_for_path(&ev.file_path))`.
Cache invalidates when `selected` changes or when the timeline content under the
selection changes. This avoids re-reading the log on every frame.

**Focus routing.** `Tab` toggles `Focus`. When `List`: `Nav(Up/Down)` moves the
selection (and resets `diff_scroll` to 0). When `Diff`: `Up`/`Down` and
`PgUp`/`PgDn` scroll the diff via `ScrollDiff`, clamped with `clamp_scroll`.
`Enter` (`NavAction::Open`) focuses the diff pane — in master-detail the diff is
always live, so Open just shifts focus rather than opening a modal. `Esc` and `q`
quit.

## Mouse split-resize

- **Divider.** The layout is `Length(list_width)` | a 1-column separator
  (`│`) | `Min(0)`. The separator is the grab target; it brightens while
  `resizing`.
- **Hit-testing.** `ui::draw` records `last_area` each frame. The divider's
  screen column is `last_area.x + list_width`. `input.rs` maps:
  - `Mouse(Down(Left))` within ±1 column of the divider → `StartResize`.
  - `Mouse(Drag(Left))` while `resizing` → `Resize(col)`, where
    `list_width = (col - last_area.x).clamp(MIN_LIST, last_area.width - MIN_DIFF - 1)`.
  - `Mouse(Up)` → `EndResize`.
  - `Mouse(ScrollUp/ScrollDown)` over the diff pane → `ScrollDiff(∓3)`.
- **Keyboard parity.** `[` and `]` nudge the split one column left/right
  (`NudgeSplit(-1)` / `NudgeSplit(+1)`), same clamp.
- **Note.** Mouse capture means terminal text-selection requires the usual
  Shift modifier; documented in the README.

## Data flow (one loop iteration)

```
loop:
  terminal.draw(|f| ui::draw(f, &mut app))     // records last_area
  if event::poll(≈250ms):
      ev = event::read()
      action = input::map(ev, &app)            // key/mouse → AppAction
      app.apply(action)                        // pure-ish state transition
  else (timeout):
      app.apply(Tick)                          // throttled: rescan files + timeline.refresh()
                                               //            + re-pin selection + invalidate cache
  if app.should_quit: break
```

`app.apply` is the single entry point for all state changes, which keeps the loop
trivial and makes transitions directly testable.

## UI (`ui.rs`)

- **Outer block** titled `chronox` with the worktree path; a footer line of live
  key hints.
- **Left pane (list).** One `entry_lines()` row per event, the selected row
  reversed (the crate already styles this), each row `clip_line_to_width()` to
  the pane width. Scrolled so the selection stays visible via `adjust_scroll()` /
  `clamp_scroll()`. `last_visible_rows` is written back for next frame.
- **Right pane (diff).** Header: `<relative path> · <tool.label()>` using
  `relative_display()`. Body: the cached styled diff lines, offset by
  `diff_scroll`, each `clip_line_to_width()` to the pane width.
- **Focus indicator.** The focused pane's border/title is bold/highlighted; the
  separator brightens during a resize drag.
- **Empty state.** No session files or no events → a centered message:
  `No changes recorded for <worktree> — run a Claude Code session here.`

## Keybindings

| Key                  | Action                                                |
|----------------------|-------------------------------------------------------|
| `↑`/`↓`, `k`/`j`     | List: move selection · Diff: scroll one line          |
| `PgUp`/`PgDn`        | Diff: scroll a page                                   |
| `g` / `G`            | List: jump to top / bottom (`NavKey::Top`/`Bottom`)   |
| `Tab`                | Toggle focus between list and diff                    |
| `Enter`              | Focus the diff pane                                   |
| `[` / `]`            | Nudge the split divider left / right                  |
| mouse drag divider   | Resize the split                                      |
| mouse wheel on diff  | Scroll the diff                                       |
| `q`, `Esc`, `Ctrl-C` | Quit                                                  |

## Error handling

- **Terminal restore is guaranteed**, including on panic: install a panic hook
  (and/or an RAII guard) that disables raw mode, leaves the alternate screen, and
  disables mouse capture before the default panic output, so a crash never leaves
  a wedged terminal.
- **Missing worktree dir or no logs** → the empty-state screen, not an error
  exit. (A worktree path that does not exist is reported once on stderr only if
  it was explicitly passed and is invalid; otherwise CWD is assumed.)
- **Malformed session logs / `load_full_change` → None** → fall back to the
  clipped `ev.detail`. The chronox parser is already defensive (skips
  unrecognized lines); the app never panics on log content.

## Testing strategy

- **`app.rs` (pure unit tests, no terminal):**
  - Selection stability: simulate a refresh that prepends events; assert
    `selected` re-pins to the same event identity, and clamps when it disappears.
  - Focus routing: `Up`/`Down` move the selection under `Focus::List` and scroll
    the diff under `Focus::Diff`.
  - `list_width` clamping at both bounds via `NudgeSplit` and `Resize`.
  - Resize transition: `StartResize → Resize(col) → EndResize` sets `resizing`
    and the clamped width given a fixed `last_area`.
  - `diff_scroll` clamping at the end of the diff.
- **`ui.rs` (ratatui `TestBackend` buffer assertions):**
  - Two-pane layout with the separator at the expected column for a given
    `list_width`.
  - Focus indicator reflects `app.focus`.
  - Empty-state screen renders when the timeline has no events.
- **Not re-tested:** chronox's `nav` and `render` logic is covered upstream; we
  test only the app shell and its use of those APIs.

## Open items resolved

- Left list width: default **32 columns**, resizable (not a percentage).
- `Enter`: **focuses the diff pane** (no modal in master-detail).
- Keyboard split nudge: `[` / `]`.
- Split width is **not persisted** across runs.
