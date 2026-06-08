# Toggle to a side-by-side diff view

**Date:** 2026-06-08
**Status:** Approved

## Goal

The diff pane currently renders a change as a "before and after" block: every
`old` line in red, then every `new` line in green (`change_detail_diff` in
`chronox/src/syntax.rs`). For anything but a one-line change this makes it hard
to see *what actually changed* — you read the whole block twice.

Add a key (`d`) that toggles the diff pane between:

- **Side-by-side** (the new default): old on the left, new on the right,
  aligned line-by-line via a real line-level diff. Removed lines show on the
  left in red, added lines on the right in green, unchanged lines appear on both
  sides plain — only genuinely-changed lines get colored.
- **Block** (today's view): kept as the toggle-to alternative.

## Cross-repo shape

`chronox-tui` depends on the `chronox` library by a pinned **git rev**, not a
path. The diff model and its `DiffMarker` live in that library, so the work
spans two repos and two PRs:

- **PR #1 (`chronox`)** — the neutral side-by-side diff model + builder + render
  helper + exports.
- **PR #2 (`chronox-tui`)** — bump the `chronox` git rev to PR #1's merge SHA,
  add the `DiffView` toggle, input, UI, docs.

During development the TUI temporarily points its `chronox` dependency at the
sibling `chronox` worktree (path override); before finalizing PR #2 it switches
back to the pinned git rev of the merged PR #1 commit. The two PRs cross-link.

## Library design (`chronox`) — PR #1

### Neutral model + builder (`syntax.rs`, dependency-free)

- A small **line-level LCS** over `old.lines()` vs `new.lines()` — O(n·m) DP.
  The blocks are small (a single edit's `old_string`/`new_string`), so DP is
  fine and keeps `syntax.rs` dependency-free, matching the file's ethos.
- New neutral types:
  - `enum CellKind { Context, Added, Removed }`
  - `struct DiffCell { gutter: String, kind: CellKind, code: Vec<Token> }`
  - `struct SideRow { left: Option<DiffCell>, right: Option<DiffCell> }`
- `pub fn change_detail_side_by_side(detail: &ChangeDetail, base_line: u32,
  lang: Option<&LangSpec>) -> Vec<SideRow>`:
  - **`Edit { old, new }`**: walk the LCS alignment.
    - *Equal* line → `SideRow { left: Context(old), right: Context(new) }`.
    - A *removed-run* adjacent to an *added-run* (a replace block) is zipped
      row-by-row: row `i` → `left = R.get(i)` as `Removed` (or blank `None`),
      `right = A.get(i)` as `Added` (or blank `None`). The shorter side gets a
      blank cell.
  - **`Write { head }`**: every line → `SideRow { left: None, right: Added }`.
  - **`None`**: empty `Vec`.
  - **Gutter / numbering**: maintain a new-file line counter starting at
    `base_line`; assign it to every cell that exists in the new file (Context +
    Added on the right) and increment. The **left gutter stays blank** — we have
    no reliable old-file line numbers, consistent with today's removed-line
    convention (`change_detail_diff` gives removed lines a blank gutter).

### Render helper (`render.rs`, `ratatui` feature)

- `pub fn side_cell_to_line(cell: Option<&DiffCell>) -> Line<'static>`:
  - `None` → empty `Line` (a blank column).
  - Otherwise: dim gutter span, a marker span (`+ ` green for `Added`, `- ` red
    for `Removed`, `  ` for `Context`), then `token_spans(&cell.code)`.
  - Reuses the existing `style_for` / `token_spans` helpers.

### Exports (`lib.rs`)

- `CellKind`, `DiffCell`, `SideRow`, `change_detail_side_by_side` from `syntax`.
- `side_cell_to_line` from `render` (under the `ratatui` feature).

## TUI design (`chronox-tui`) — PR #2

### State + actions (`app.rs`)

- `enum DiffView { SideBySide, Block }`; `App.diff_view` field defaulting to
  `DiffView::SideBySide`.
- `AppAction::ToggleDiffView`. `App::apply` flips `diff_view` and zeroes
  `diff_scroll` (the two renderings have different row counts).
- `diff_side_rows(&mut self) -> &[SideRow]` builder, cached by
  `(ChangeSource, DiffView)` alongside the existing `diff_lines()`. Built via
  the same `load_full_change(ev).unwrap_or(ev.detail)` + `resolve_line_in_file`
  base-line path the block view uses.

### Input (`input.rs`)

- `KeyCode::Char('d')` → `AppAction::ToggleDiffView`. `d` is currently unbound.

### Drawing (`ui.rs`)

- `render_diff` branches on `app.diff_view`:
  - **Block** → today's single-`Paragraph` path, unchanged.
  - **SideBySide** → split the block's inner area into `[left | separator |
    right]` with an even split (`left_w = (inner.width - 1) / 2`, separator 1
    col, right takes the rest). Render two `Paragraph`s built from
    `side_cell_to_line`, each clipped to its column width, sharing one scroll
    offset (row counts match across columns). Scroll is clamped against the
    `SideRow` count.
- Footer hints gain `d diff/block`. The pane title may note the active mode.

### Docs

- README + footer updated to advertise `d`.

## Narrow-pane tradeoff (accepted)

The diff pane's minimum is `MIN_DIFF = 24` cols, so a narrow split gives ~11
cols per column — tight for code. The accepted default is to split evenly and
let each column clip; **no** auto-fallback to block mode (that would be
surprising). A per-width heuristic can be added later if it proves annoying.

## Testable seams

- **Library**
  - `change_detail_side_by_side`: pure-replace, pure-add, pure-remove, mixed
    edit, `Write`, `None`; right-gutter numbering from `base_line`; blank left
    gutter on removed lines; blank cell on the short side of an uneven replace.
  - `side_cell_to_line`: `None` → empty line; marker color/text per `CellKind`;
    gutter dim; token spans preserved.
- **TUI**
  - `App` default `diff_view` is `SideBySide`.
  - `input.rs`: `d` maps to `ToggleDiffView`.
  - `app.rs`: `ToggleDiffView` flips the view and resets `diff_scroll`.
  - `ui.rs`: side-by-side renders two columns with the separator at the split;
    footer advertises `d`.

The two-column `Paragraph` layout dance is exercised via the existing
`TestBackend` buffer assertions.
