# Chronox grouped-by-file timeline — design

Date: 2026-06-10
Status: approved (pending implementation plan)

## Overview

Today chronox's left pane is a flat, newest-first list of individual file
changes (one `entry_lines` row per `ChangeEvent`). This redesign **groups
changes by file**, so the timeline answers "what did the agent do to *this*
file, and in what order?". The right pane (the syntax-highlighted side-by-side /
block diff) is **unchanged**.

Source design package: `~/Documents/chronox/design_handoff_grouped_timeline`
(HTML/CSS/JS prototype — reference only, not code to copy). The chosen treatment
is **B1 · Summaries**, folded per **B3 · Collapsed roll-up**.

Target: the existing Rust app — ratatui 0.29 + crossterm, with `sessionx`
providing the parsed `Timeline`, `nav`, and styled render helpers. Modules:
`src/app.rs` (state), `src/ui.rs` (draw), `src/render.rs` (event→`Line`),
`src/input.rs` (key/mouse → `AppAction`).

## Decisions (locked during brainstorming)

- **Count source: A — in-memory bounded `detail`.** Counts feed a glanceable
  gauge + small `+N`/`-N` labels; exactness past `DETAIL_MAX_CHARS` does not
  matter, and this keeps the 1s refresh loop I/O-free.
- **Selection style: blue bar** — `Color::Rgb(0x24,0x30,0x49)` background across
  the full inner width with brightened fg (the one deliberate departure from the
  16-color ANSI palette; the mock uses it).
- **Fold model: accordion only** — exactly the active file is expanded; all
  others fold automatically. No persisted fold set, no `space` key, no
  `AppAction::ToggleFold`.
- **Chrome: all three** — status strip (with braille spinner), single-frame
  split (`┬`/`│`/`┴`), updated footer. The footer does **not** advertise
  `space fold` (no such key) and does **not** advertise `/ filter` (future).

## 1. State model (`app.rs`)

```rust
enum VisibleRow {
    Header { group: usize },   // index into `groups`
    Edit   { event: usize },   // index into `App.events`
}

struct FileGroup {
    file: PathBuf,
    event_idxs: Vec<usize>,    // newest-first, into App.events
    add: u32,
    del: u32,                  // roll-up over the group
    is_new: bool,              // single Write → " new" tag
}
```

New `App` fields:

- `groups: Vec<FileGroup>` — ordered most-recently-touched first.
- `visible: Vec<VisibleRow>` — headers (always) interleaved with the active
  file's edit rows, in draw order.
- `counts: HashMap<ChangeSource, (u32, u32)>` — memoized per-change line counts.
- `spinner_frame: usize` — advanced one frame per poll tick.

`selected` stays a `usize` but now indexes `visible`, not `events`.

**Selection is semantic-first.** After any selection move, capture *what* is
selected — a `ChangeSource` for an edit row, or a `PathBuf` for a header — then
rebuild `groups`/`visible`, then re-resolve the index of that semantic target.
This makes the accordion and live-refresh repin fall out of one mechanism
instead of fighting index shifts when rows expand/collapse.

## 2. Grouping, ordering, accordion

Group `events` by `file_path`, **preserving first-seen order**. Because
`sessionx`'s `Timeline` is already newest-first, a stable group-by yields:

- groups ordered most-recently-touched file first,
- edits newest-first within each group.

(Mirrors the reference `byFile()` in `design/data.jsx`.)

The **active file** is the group that owns the selected row. `visible` is built
as: every header in group order, with the active group's edit rows inserted
directly under its header. Exactly the active file is expanded (`▾`); every
other file is folded (`▸`, header only). No persisted fold state.

Because selecting a header makes that file active, the cursor is never parked on
a folded header — landing on a header expands its file. Crossing files by moving
the cursor collapses the previous file and expands the new one (an accordion).

## 3. Counts (source A)

```rust
fn change_counts(detail: &ChangeDetail) -> (u32, u32)
```

Runs `change_detail_diff` over the in-memory bounded `ChangeDetail` and counts
`DiffMarker::Added` vs `DiffMarker::Removed` lines. Results are memoized in
`App.counts` keyed by `ChangeSource` — append-only session logs make a given
`(file, line, index)` stable, so the cache never needs invalidation. Per-group
`add`/`del` are the sums over the group's events. The cache persists across
refreshes; only newly-seen sources compute.

## 4. Navigation (`Focus::List`)

- `↑`/`↓`, `k`/`j`, `g`/`G`, `Home`/`End` move the cursor over `visible` via
  `sessionx::nav` against `visible.len()`.
- Landing on an **edit row** drives the diff pane to that event (as today).
- Landing on a **file header** makes that file active → it expands, the
  previously active file collapses, and the diff shows that file's **newest**
  change.
- `Enter` → focus the diff pane (unchanged). `Tab` → toggle Focus. Diff-focus
  arrow scrolling, `[`/`]`, mouse drag/wheel, `e`, `d`, `q`/`Esc`/`Ctrl-C`:
  unchanged.
- `adjust_scroll` / `clamp_scroll` run against `visible.len()`.

Moving the selection resets `diff_scroll` to 0 (preserved from current
behavior).

### Resolving the selected event for the diff pane

`selected` now indexes `visible`, but the diff pane, `selected_path_and_line`,
and the `diff_lines`/`diff_side_rows` caches all need an **event** index. A
single helper resolves the selected `VisibleRow` to an event:

- `Edit { event }` → that event index.
- `Header { group }` → the group's **newest** event (`event_idxs[0]`).

Every current use of `self.selected` as an event index (`selected_event`,
`selected_path_and_line`, the diff caches keyed by `ChangeSource`) routes
through this helper, so a header selection shows that file's newest change.

## 5. Live refresh / repin

On `Tick`:

1. Capture the selected semantic target (the edit's `ChangeSource`, or the
   header's `PathBuf`).
2. Re-scan logs, rebuild `events`, `groups`, `visible`.
3. Re-find that target's new index in `visible`; clamp if it is gone.

New changes that arrive for the active file appear at the top of its group
without moving the cursor. The existing `repin` behavior (pin to the selected
change's `ChangeSource`) is generalized to also handle header selection (pin to
file path).

## 6. Rendering (`render.rs` + `ui.rs`)

`entry_lines` is replaced by two `Line` builders.

### Header row

Format: `<caret><path><pad><gauge> +A[ -D]<pad><count>`

| Field | Glyph / format | Style |
|---|---|---|
| caret | `▾ ` expanded, `▸ ` folded | `Cyan` expanded; `DarkGray`+`DIM` folded |
| path | worktree-relative, via `relative_display` + `abbreviate_path` | `Cyan`+`BOLD` if active, else `White` |
| ` new` | only when group is a single `Write` | `Blue` |
| gauge | 4-cell magnitude bar | green / red / `DarkGray` |
| `+A` | group added sum | `Green` |
| `-D` | group removed sum (omit if 0) | `Red` |
| count | edit count, right-aligned | `DarkGray`+`DIM` |

Caret+path left-justified; `gauge … +A -D … count` block right-justified; pad
the middle with spaces. Front-truncate the path (via `abbreviate_path`) before
the right block if the row would overflow.

### Edit row (active file only)

Format: `  <connector> <HH:MM>  +a[ -d]  <summary>`

| Field | Glyph / format | Style |
|---|---|---|
| indent | 2 spaces | — |
| connector | `├ ` for every edit except the last, `└ ` for the last | `DarkGray` |
| time | `HH:MM` via `render::hhmm` | `DarkGray`+`DIM` |
| `+a` | added lines for this change | `Green` |
| `-d` | removed lines (omit if 0) | `Red` |
| summary | `ev.summary`, clipped | `DarkGray`+`DIM`; `White` when selected |

### Magnitude gauge

Fixed-width bar of `▰` (filled) / `▱` (empty), `width` cells (header width = 4),
per the reference `statBar`:

```
total = add + del            // floor at 1 to avoid /0
g = round(add / total * width)
r = round(del / total * width)
if add > 0 && g == 0 { g = 1 }
if del > 0 && r == 0 { r = 1 }
while g + r > width { if r > g { r -= 1 } else { g -= 1 } }
empty = width - g - r
// '▰'*g Green, '▰'*r Red, '▱'*empty DarkGray(faint)
```

### Selection bar

Apply `Style::default().bg(Color::Rgb(0x24,0x30,0x49))` with a brightened fg
across the full inner-width of the selected row (pad to inner width so the bar
fills the row).

### `render_list`

Iterates `visible[scroll..scroll+rows]`, emitting a header or edit `Line` per
row, clipping each to the inner width, applying the selection bar to the
selected index.

## 7. Chrome

### Status strip (line 0, replaces the plain title)

`● chronox  <worktree>   ⠋ live · polling 1s   +<TOTAL_ADD> -<TOTAL_DEL>   <N> changes · <M> files`

- `●` and `live` in `Green`; totals `+`/`-` green/red.
- `⠋` is a braille spinner advanced one frame per poll tick from
  `⠋⠙⠹⠸⠼⠴⠦⠧⠇` (`App.spinner_frame`). Drop the spinner under reduced-motion /
  non-interactive output.
- `N = events.len()`, `M = groups.len()`, totals = session sums.

### Single-frame split

One `Block::bordered` body with an internal `┬` (top) / `│` (body) / `┴`
(bottom) divider column instead of two adjacent bordered panes. Left title
`chronox · by file`; right title `<relpath> · <tool.label()>`. The existing
draggable-divider mouse hit-test stays anchored on the divider column.

### Footer

` ↑↓ move · enter diff · d view · e edit · tab focus · q quit `

(No `space fold`; no `/ filter`.) A transient status message still takes over the
footer until the next keypress.

## 8. Module placement & testing

- `app.rs`: `FileGroup`, `VisibleRow`, grouping/ordering, accordion derivation,
  visible-row nav, semantic repin, `counts` cache, `spinner_frame`.
- `render.rs`: header/edit `Line` builders, gauge, `change_counts`.
- `ui.rs`: `render_list` over `visible`, status strip, single-frame split, titles.
- `input.rs`: unchanged (no new keys).

Built test-first. Test impact:

- Existing `app.rs` nav tests assert raw-event-index semantics → rewritten for
  visible-row semantics.
- `render.rs` `entry_lines` tests → replaced by header/edit builder tests.
- `ui.rs` buffer tests for the two-pane separator / titles → updated for the
  single-frame layout.
- New unit tests: gauge algorithm, grouping/ordering, count derivation +
  caching, accordion expansion on header selection, status-strip totals.

## Out of scope

- Right-pane diff rendering (unchanged).
- B2 magnitude edit rows, B4 file-tree view.
- Search / filtering.
- Manual fold pins / `space` key.
- Worktree picking.
