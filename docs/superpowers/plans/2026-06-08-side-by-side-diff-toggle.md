# Side-by-side diff view toggle — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `d` key that toggles the chronox-tui diff pane between today's before/after block view and a new, default side-by-side diff (old | new, aligned line-by-line, only changed lines colored).

**Architecture:** The line-level diff model lives in the framework-agnostic `chronox` core (a dependency-free LCS + neutral `SideRow`/`DiffCell` types), with a thin ratatui render helper. The TUI splits the diff pane into two columns and renders them with a shared scroll offset. Because chronox-tui pins `chronox` by git rev, this ships as two sequential PRs: the library PR merges first, then the TUI PR bumps the rev and adds the toggle.

**Tech Stack:** Rust (edition 2024), ratatui 0.29, `chronox` library crate, wsx for the cross-repo worktrees.

---

## File Structure

**PR #1 — `chronox` library**
- Modify `src/syntax.rs` — add `CellKind`, `DiffCell`, `SideRow`, an internal `DiffOp` + `lcs_ops`, and the public `change_detail_side_by_side` builder.
- Modify `src/render.rs` — add `side_cell_to_line` (ratatui feature).
- Modify `src/lib.rs` — export the new items.

**PR #2 — `chronox-tui`**
- Modify `Cargo.toml` — bump the `chronox` git `rev` to PR #1's merge SHA.
- Modify `src/app.rs` — `DiffView` enum, `App.diff_view` field, `AppAction::ToggleDiffView`, `diff_side_rows()` + its cache.
- Modify `src/input.rs` — map `d`.
- Modify `src/ui.rs` — branch `render_diff`; add `render_diff_block` + `render_diff_side_by_side`; footer hint.
- Modify `README.md` — document `d`.

---

# Phase 1 — `chronox` library (PR #1)

### Task 1: Create the chronox worktree

**Files:** none (workspace setup)

- [ ] **Step 1: Create a sibling workspace in the `chronox` repo**

Run from this chronox-tui session:
```bash
wsx workspace create chronox --name side-by-side-diff
```
Expected: wsx creates a branch (its configured prefix + `side-by-side-diff`) and a worktree.

- [ ] **Step 2: Resolve and enter the worktree**

```bash
CHRONOX_WT="$(wsx workspace path chronox side-by-side-diff)"
echo "$CHRONOX_WT"
cd "$CHRONOX_WT"
```
Expected: prints a path like `/home/eben/.local/state/wsx/worktrees/chronox/side-by-side-diff` and changes into it. Run all Phase 1 commands from `$CHRONOX_WT`.

- [ ] **Step 3: Baseline the test suite**

Run: `cargo test`
Expected: PASS (existing suite green before any change).

---

### Task 2: Side-by-side diff model + builder (`src/syntax.rs`)

**Files:**
- Modify: `src/syntax.rs`
- Test: `src/syntax.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Add these tests to the existing `mod tests` block in `src/syntax.rs`:

```rust
    // Helper: join a cell's code tokens back into a plain string.
    fn cell_code(cell: &Option<DiffCell>) -> Option<String> {
        cell.as_ref()
            .map(|c| c.code.iter().map(|(t, _)| t.clone()).collect())
    }

    #[test]
    fn side_by_side_pure_replace_zips_old_and_new() {
        let detail = ChangeDetail::Edit {
            old: "a\nb".into(),
            new: "x\ny".into(),
        };
        let rows = change_detail_side_by_side(&detail, 5, None);
        assert_eq!(rows.len(), 2);
        // row 0: removed "a" (blank gutter) | added "x" (numbered from base 5)
        let l0 = rows[0].left.as_ref().unwrap();
        let r0 = rows[0].right.as_ref().unwrap();
        assert_eq!(l0.kind, CellKind::Removed);
        assert_eq!(l0.gutter, "     ");
        assert_eq!(cell_code(&rows[0].left).unwrap(), "a");
        assert_eq!(r0.kind, CellKind::Added);
        assert_eq!(r0.gutter, "   5 ");
        assert_eq!(cell_code(&rows[0].right).unwrap(), "x");
        // row 1: new line numbered 6
        assert_eq!(rows[1].right.as_ref().unwrap().gutter, "   6 ");
    }

    #[test]
    fn side_by_side_keeps_context_and_blanks_short_side() {
        let detail = ChangeDetail::Edit {
            old: "ctx\nlet x = 1;".into(),
            new: "ctx\nlet x = 2;\nlet y = 3;".into(),
        };
        let rows = change_detail_side_by_side(&detail, 10, None);
        // row 0 is the shared context line, present on both sides
        assert_eq!(rows[0].left.as_ref().unwrap().kind, CellKind::Context);
        assert_eq!(rows[0].right.as_ref().unwrap().kind, CellKind::Context);
        assert_eq!(cell_code(&rows[0].left).unwrap(), "ctx");
        // a replace row (let x = 1; -> let x = 2;) then an add-only row (let y = 3;)
        assert_eq!(cell_code(&rows[1].left).unwrap(), "let x = 1;");
        assert_eq!(cell_code(&rows[1].right).unwrap(), "let x = 2;");
        assert!(rows[2].left.is_none(), "the extra added line has no left side");
        assert_eq!(cell_code(&rows[2].right).unwrap(), "let y = 3;");
        assert_eq!(rows[2].right.as_ref().unwrap().kind, CellKind::Added);
    }

    #[test]
    fn side_by_side_write_is_added_only_on_the_right() {
        let detail = ChangeDetail::Write {
            head: "one\ntwo".into(),
        };
        let rows = change_detail_side_by_side(&detail, 1, None);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].left.is_none());
        assert_eq!(rows[0].right.as_ref().unwrap().gutter, "   1 ");
        assert_eq!(rows[0].right.as_ref().unwrap().kind, CellKind::Added);
        assert_eq!(rows[1].right.as_ref().unwrap().gutter, "   2 ");
    }

    #[test]
    fn side_by_side_none_is_empty() {
        assert!(change_detail_side_by_side(&ChangeDetail::None, 1, None).is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib side_by_side`
Expected: FAIL — `cannot find function change_detail_side_by_side` / `cannot find type DiffCell`.

- [ ] **Step 3: Add the model types and builder**

In `src/syntax.rs`, after the `DiffLine` struct definition, add:

```rust
/// Which side/kind a side-by-side cell represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Context,
    Added,
    Removed,
}

/// One side (left=old or right=new) of a side-by-side diff row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffCell {
    pub gutter: String,
    pub kind: CellKind,
    pub code: Vec<Token>,
}

/// One row of a side-by-side diff. Either side may be absent (a blank column).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideRow {
    pub left: Option<DiffCell>,
    pub right: Option<DiffCell>,
}

/// Internal line-alignment op produced by `lcs_ops`.
enum DiffOp {
    Equal(usize, usize),
    Removed(usize),
    Added(usize),
}

/// Longest-common-subsequence alignment of two line sequences. O(n*m) DP —
/// the blocks here are a single edit's old/new text, so this stays cheap and
/// keeps `syntax.rs` dependency-free.
fn lcs_ops(old: &[&str], new: &[&str]) -> Vec<DiffOp> {
    let (n, m) = (old.len(), new.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            ops.push(DiffOp::Equal(i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(DiffOp::Removed(i));
            i += 1;
        } else {
            ops.push(DiffOp::Added(j));
            j += 1;
        }
    }
    while i < n {
        ops.push(DiffOp::Removed(i));
        i += 1;
    }
    while j < m {
        ops.push(DiffOp::Added(j));
        j += 1;
    }
    ops
}

/// Flush a pending change block (consecutive removed and/or added lines) as
/// zipped side-by-side rows: row k pairs the k-th removed line (left, red) with
/// the k-th added line (right, green); the shorter side gets a blank cell.
fn push_change_block(
    rows: &mut Vec<SideRow>,
    rem: &mut Vec<usize>,
    add: &mut Vec<usize>,
    old_lines: &[&str],
    new_lines: &[&str],
    base_line: u32,
    lang: Option<&LangSpec>,
) {
    let k = rem.len().max(add.len());
    for idx in 0..k {
        let left = rem.get(idx).map(|&i| DiffCell {
            gutter: "     ".to_string(),
            kind: CellKind::Removed,
            code: code_tokens(old_lines[i], lang),
        });
        let right = add.get(idx).map(|&j| DiffCell {
            gutter: format!("{:>4} ", base_line.saturating_add(j as u32)),
            kind: CellKind::Added,
            code: code_tokens(new_lines[j], lang),
        });
        rows.push(SideRow { left, right });
    }
    rem.clear();
    add.clear();
}

/// Build a side-by-side diff: old on the left, new on the right, aligned by an
/// LCS so only genuinely-changed lines are marked. Right-side (new-file) lines
/// are numbered from `base_line`; the left gutter is blank (no reliable
/// old-file numbers — matches the removed-line convention of `change_detail_diff`).
pub fn change_detail_side_by_side(
    detail: &ChangeDetail,
    base_line: u32,
    lang: Option<&LangSpec>,
) -> Vec<SideRow> {
    match detail {
        ChangeDetail::Edit { old, new } => {
            let old_lines: Vec<&str> = old.lines().collect();
            let new_lines: Vec<&str> = new.lines().collect();
            let mut rows: Vec<SideRow> = Vec::new();
            let mut rem: Vec<usize> = Vec::new();
            let mut add: Vec<usize> = Vec::new();
            for op in lcs_ops(&old_lines, &new_lines) {
                match op {
                    DiffOp::Equal(i, j) => {
                        push_change_block(
                            &mut rows, &mut rem, &mut add, &old_lines, &new_lines, base_line, lang,
                        );
                        rows.push(SideRow {
                            left: Some(DiffCell {
                                gutter: "     ".to_string(),
                                kind: CellKind::Context,
                                code: code_tokens(old_lines[i], lang),
                            }),
                            right: Some(DiffCell {
                                gutter: format!("{:>4} ", base_line.saturating_add(j as u32)),
                                kind: CellKind::Context,
                                code: code_tokens(new_lines[j], lang),
                            }),
                        });
                    }
                    DiffOp::Removed(i) => rem.push(i),
                    DiffOp::Added(j) => add.push(j),
                }
            }
            push_change_block(
                &mut rows, &mut rem, &mut add, &old_lines, &new_lines, base_line, lang,
            );
            rows
        }
        ChangeDetail::Write { head } => head
            .lines()
            .enumerate()
            .map(|(j, l)| SideRow {
                left: None,
                right: Some(DiffCell {
                    gutter: format!("{:>4} ", base_line.saturating_add(j as u32)),
                    kind: CellKind::Added,
                    code: code_tokens(l, lang),
                }),
            })
            .collect(),
        ChangeDetail::None => Vec::new(),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib side_by_side`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/syntax.rs
git commit -m "feat(syntax): side-by-side diff model and LCS builder"
```

---

### Task 3: Render helper (`src/render.rs`)

**Files:**
- Modify: `src/render.rs`
- Test: `src/render.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/render.rs`:

```rust
    #[test]
    fn side_cell_styles_marker_gutter_and_none() {
        use crate::syntax::{change_detail_side_by_side, CellKind};
        let detail = ChangeDetail::Edit {
            old: "a".into(),
            new: "let y = 1".into(),
        };
        let rows = change_detail_side_by_side(&detail, 4, lang_for_path(Path::new("a.rs")));
        // removed cell on the left: dim gutter, red "- " marker
        let left = side_cell_to_line(rows[0].left.as_ref());
        assert_eq!(left.spans[0].content.as_ref(), "     ");
        assert!(left.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(left.spans[1].content.as_ref(), "- ");
        assert_eq!(left.spans[1].style.fg, Some(Color::Red));
        // added cell on the right: gutter "   4 ", green "+ ", "let" highlighted
        let right = side_cell_to_line(rows[0].right.as_ref());
        assert_eq!(right.spans[0].content.as_ref(), "   4 ");
        assert_eq!(right.spans[1].content.as_ref(), "+ ");
        assert_eq!(right.spans[1].style.fg, Some(Color::Green));
        assert!(right.spans.iter().any(|s| s.content.as_ref() == "let"
            && s.style.fg == Some(Color::Magenta)));
        // a context cell uses a blank "  " marker with no colour
        let _ = CellKind::Context;
        // None -> an empty line (blank column)
        assert!(side_cell_to_line(None).spans.is_empty());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib side_cell`
Expected: FAIL — `cannot find function side_cell_to_line`.

- [ ] **Step 3: Implement `side_cell_to_line`**

In `src/render.rs`, update the imports line and add the function after `diff_line_to_ratatui`.

Change the syntax import at the top of the file from:
```rust
use crate::syntax::{DiffLine, DiffMarker, LangSpec, Token, TokenKind, change_detail_diff};
```
to:
```rust
use crate::syntax::{
    CellKind, DiffCell, DiffLine, DiffMarker, LangSpec, Token, TokenKind, change_detail_diff,
};
```

Then add:
```rust
/// Map one side-by-side cell to a styled line. `None` yields an empty line (a
/// blank column). Same gutter/marker/colour vocabulary as `diff_line_to_ratatui`.
pub fn side_cell_to_line(cell: Option<&DiffCell>) -> Line<'static> {
    let Some(c) = cell else {
        return Line::default();
    };
    let dim = Style::default().add_modifier(Modifier::DIM);
    let (marker, marker_style) = match c.kind {
        CellKind::Added => ("+ ", Style::default().fg(Color::Green)),
        CellKind::Removed => ("- ", Style::default().fg(Color::Red)),
        CellKind::Context => ("  ", Style::default()),
    };
    let mut spans = vec![
        Span::styled(c.gutter.clone(), dim),
        Span::styled(marker.to_string(), marker_style),
    ];
    spans.extend(token_spans(&c.code));
    Line::from(spans)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib side_cell`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/render.rs
git commit -m "feat(render): side_cell_to_line for side-by-side diff"
```

---

### Task 4: Export the new API (`src/lib.rs`)

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Add the exports**

In `src/lib.rs`, change the syntax re-export from:
```rust
pub use syntax::{DiffLine, DiffMarker, LangSpec, Token, TokenKind, lang_for_path};
```
to:
```rust
pub use syntax::{
    CellKind, DiffCell, DiffLine, DiffMarker, LangSpec, SideRow, Token, TokenKind,
    change_detail_side_by_side, lang_for_path,
};
```

And change the ratatui render re-export from:
```rust
pub use render::{
    change_detail_lines_styled, clip_line_to_width, entry_lines, hhmm, relative_display,
    should_auto_hide,
};
```
to:
```rust
pub use render::{
    change_detail_lines_styled, clip_line_to_width, entry_lines, hhmm, relative_display,
    should_auto_hide, side_cell_to_line,
};
```

- [ ] **Step 2: Run the full suite and clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "feat(lib): export side-by-side diff API"
```

---

### Task 5: Open PR #1

**Files:** none

- [ ] **Step 1: Push the branch**

```bash
git push -u origin HEAD
```

- [ ] **Step 2: Open the PR**

```bash
gh pr create --title "feat: side-by-side diff model + builder" \
  --body "Adds a framework-agnostic side-by-side diff to the chronox core: a dependency-free LCS line aligner, neutral \`SideRow\`/\`DiffCell\`/\`CellKind\` types, the \`change_detail_side_by_side\` builder, and a \`side_cell_to_line\` ratatui helper. Consumed by chronox-tui PR (cross-linked below)."
```
Expected: prints the PR URL. **Record it — Task 11 cross-links to it.**

- [ ] **Step 3: STOP for review**

Wait for the user to review and merge PR #1. **Phase 2 cannot start until PR #1 is merged**, because the TUI bumps its git rev to the merge commit.

---

# Phase 2 — `chronox-tui` (PR #2)

> Run all Phase 2 commands from the chronox-tui worktree:
> `/home/eben/.local/state/wsx/worktrees/chronox-tui/silent-rose`

### Task 6: Bump the chronox dependency rev

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Record PR #1's merge SHA**

```bash
git -C "$(wsx workspace path chronox side-by-side-diff)" fetch origin
git -C "$(wsx workspace path chronox side-by-side-diff)" rev-parse origin/main
```
Expected: prints the 40-char merge commit SHA of PR #1. Call it `<SHA>`.

- [ ] **Step 2: Update the rev in `Cargo.toml`**

Change the dependency line from:
```toml
chronox = { git = "https://github.com/bakedbean/chronox", rev = "deacc9ad8698acd95dbb1fb0aea35b1becf5e8bd" }
```
to (substitute the real `<SHA>`):
```toml
chronox = { git = "https://github.com/bakedbean/chronox", rev = "<SHA>" }
```

- [ ] **Step 3: Fetch and build against the new rev**

Run: `cargo build`
Expected: Cargo updates `Cargo.lock` to the new rev and builds clean.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: bump chronox to side-by-side diff rev"
```

---

### Task 7: View state, toggle action, and row cache (`src/app.rs`)

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/app.rs`:

```rust
    #[test]
    fn default_diff_view_is_side_by_side() {
        let app = App::bare(PathBuf::from("/wt"));
        assert_eq!(app.diff_view, DiffView::SideBySide);
    }

    #[test]
    fn toggle_diff_view_flips_and_resets_scroll() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.diff_scroll = 9;
        app.apply(AppAction::ToggleDiffView);
        assert_eq!(app.diff_view, DiffView::Block);
        assert_eq!(app.diff_scroll, 0, "toggling resets the diff scroll");
        app.apply(AppAction::ToggleDiffView);
        assert_eq!(app.diff_view, DiffView::SideBySide);
    }

    #[test]
    fn diff_side_rows_falls_back_to_detail_when_log_missing() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev(1, "/wt/a.rs", 1)]);
        // ev() is an Edit { old: "a", new: "b" } -> one zipped replace row.
        let rows = app.diff_side_rows().to_vec();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].left.is_some() && rows[0].right.is_some());
    }

    #[test]
    fn diff_side_rows_empty_when_no_events() {
        let mut app = App::bare(PathBuf::from("/wt"));
        assert!(app.diff_side_rows().is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test diff_view diff_side_rows`
Expected: FAIL — `cannot find type DiffView` / `no variant ToggleDiffView` / `no method diff_side_rows`.

- [ ] **Step 3: Add the `DiffView` enum**

In `src/app.rs`, after the `Focus` enum, add:
```rust
/// Which rendering the diff pane uses. Side-by-side is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffView {
    SideBySide,
    Block,
}
```

- [ ] **Step 4: Add the action variant**

In the `AppAction` enum, add a variant after `ScrollDiff(i32)`:
```rust
    /// Flip the diff pane between side-by-side and block rendering.
    ToggleDiffView,
```

- [ ] **Step 5: Extend imports and `App` fields**

Change the chronox import block from:
```rust
use chronox::{
    ChangeEvent, ChangeSource, NavAction, NavKey, Timeline, change_detail_lines_styled,
    lang_for_path,
};
```
to:
```rust
use chronox::{
    ChangeEvent, ChangeSource, NavAction, NavKey, SideRow, Timeline,
    change_detail_lines_styled, change_detail_side_by_side, lang_for_path,
};
```

In the `App` struct, after the `diff_cache` field, add:
```rust
    diff_view: DiffView,
    side_cache: Option<(ChangeSource, Vec<SideRow>)>,
```
Make `diff_view` public by writing it as `pub diff_view: DiffView,` (the UI and tests read it).

In `App::bare`, after `diff_cache: None,`, add:
```rust
            diff_view: DiffView::SideBySide,
            side_cache: None,
```

- [ ] **Step 6: Add the side-row builder**

In `src/app.rs`, after the `build_diff_lines` free function, add:
```rust
/// Build the full, un-clipped side-by-side rows for one change. Same re-read +
/// base-line resolution as `build_diff_lines`.
fn build_side_rows(ev: &ChangeEvent) -> Vec<SideRow> {
    let detail = load_full_change(ev).unwrap_or_else(|| ev.detail.clone());
    let base = resolve_line_in_file(&ev.file_path, &detail);
    change_detail_side_by_side(&detail, base, lang_for_path(&ev.file_path))
}
```

- [ ] **Step 7: Handle the action**

In `App::apply`, add an arm after the `ScrollDiff` arm:
```rust
            AppAction::ToggleDiffView => {
                self.diff_view = match self.diff_view {
                    DiffView::SideBySide => DiffView::Block,
                    DiffView::Block => DiffView::SideBySide,
                };
                self.diff_scroll = 0;
            }
```

- [ ] **Step 8: Add the cached accessor**

In the `impl App` block that contains `diff_lines`, add after `diff_lines`:
```rust
    /// Styled side-by-side rows for the current selection, cached by the
    /// selected change's `ChangeSource` (mirrors `diff_lines`).
    pub fn diff_side_rows(&mut self) -> &[SideRow] {
        let src = self.events.get(self.selected).map(|e| e.source.clone());
        let needs = match (&self.side_cache, &src) {
            (Some((cached, _)), Some(s)) => cached != s,
            _ => true,
        };
        if needs {
            match src {
                Some(s) => {
                    let rows = self
                        .events
                        .get(self.selected)
                        .map(build_side_rows)
                        .unwrap_or_default();
                    self.side_cache = Some((s, rows));
                }
                None => self.side_cache = None,
            }
        }
        self.side_cache
            .as_ref()
            .map(|(_, r)| r.as_slice())
            .unwrap_or(&[])
    }
```

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test diff_view diff_side_rows`
Expected: PASS (4 tests).

- [ ] **Step 10: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): DiffView toggle and side-by-side row cache"
```

---

### Task 8: Bind the `d` key (`src/input.rs`)

**Files:**
- Modify: `src/input.rs`
- Test: `src/input.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/input.rs`:
```rust
    #[test]
    fn d_toggles_diff_view() {
        let app = App::bare(PathBuf::from("/wt"));
        assert_eq!(
            map(key(KeyCode::Char('d')), &app),
            AppAction::ToggleDiffView
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test d_toggles_diff_view`
Expected: FAIL — asserts `AppAction::None != AppAction::ToggleDiffView`.

- [ ] **Step 3: Add the key mapping**

In `src/input.rs`, in `map_key`, add an arm after the `KeyCode::Char('e')` arm:
```rust
        KeyCode::Char('d') => AppAction::ToggleDiffView,
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test d_toggles_diff_view`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input.rs
git commit -m "feat(input): d toggles the diff view"
```

---

### Task 9: Render side-by-side and update the footer (`src/ui.rs`)

**Files:**
- Modify: `src/ui.rs`
- Test: `src/ui.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/ui.rs`:
```rust
    #[test]
    fn footer_advertises_the_diff_toggle_key() {
        let mut app = App::bare(PathBuf::from("/wt"));
        let buf = draw_app(&mut app, 80, 10);
        assert!(buffer_text(&buf).contains("d view"));
    }

    #[test]
    fn side_by_side_shows_old_left_and_new_right() {
        let mut app = App::bare(PathBuf::from("/wt"));
        // ev()'s detail is Edit { old: "old", new: "new" } -> one replace row.
        app.set_events_for_test_pub(vec![ev("/wt/src/main.rs")]);
        app.focus = Focus::Diff; // default view is SideBySide
        let buf = draw_app(&mut app, 80, 12);
        let text = buffer_text(&buf);
        assert!(text.contains("- old"), "removed line shown on the left");
        assert!(text.contains("+ new"), "added line shown on the right");
    }

    #[test]
    fn block_view_still_renders_after_toggle() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev("/wt/src/main.rs")]);
        app.apply(crate::app::AppAction::ToggleDiffView); // -> Block
        let buf = draw_app(&mut app, 80, 12);
        let text = buffer_text(&buf);
        assert!(text.contains("- old") && text.contains("+ new"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test footer_advertises_the_diff_toggle_key side_by_side_shows`
Expected: FAIL — footer lacks "d view"; side-by-side path not implemented yet.

- [ ] **Step 3: Extend imports**

In `src/ui.rs`, change:
```rust
use chronox::{clip_line_to_width, entry_lines, relative_display};
```
to:
```rust
use chronox::{clip_line_to_width, entry_lines, relative_display, side_cell_to_line};
```
and:
```rust
use crate::app::{App, Focus};
```
to:
```rust
use crate::app::{App, DiffView, Focus};
```

- [ ] **Step 4: Update the footer hints**

In `render_footer`, change the two focus hint strings to add `· d view`:
```rust
            Focus::List => " ↑↓ move · e edit · d view · Tab focus diff · [ ] resize · q quit ",
            Focus::Diff => " ↑↓/PgUp/PgDn scroll · e edit · d view · Tab focus list · [ ] resize · q quit ",
```

- [ ] **Step 5: Branch `render_diff` and add the two renderers**

Replace the body of `render_diff` *after* `f.render_widget(block, area);` (the part that computes `body`/`width`/`lines` and renders the paragraph) with a dispatch, and add two helper fns. The full replacement for `render_diff` and its helpers:

```rust
fn render_diff(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.focus == Focus::Diff;
    let header = match app.selected_event() {
        Some(ev) => format!(
            "{} · {}",
            relative_display(&ev.file_path, &app.worktree),
            ev.tool.label()
        ),
        None => "—".to_string(),
    };
    let block = pane_block(&header, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    match app.diff_view {
        DiffView::Block => render_diff_block(f, inner, app),
        DiffView::SideBySide => render_diff_side_by_side(f, inner, app),
    }
}

/// Today's view: removed (red) block then added (green) block, one column.
fn render_diff_block(f: &mut Frame, inner: Rect, app: &mut App) {
    let body = inner.height as usize;
    let width = inner.width as usize;
    let lines = app.diff_lines().to_vec();
    let scroll = clamp_scroll(app.diff_scroll, lines.len(), body);
    app.diff_scroll = scroll;

    let visible: Vec<Line> = lines
        .into_iter()
        .skip(scroll)
        .take(body)
        .map(|l| clip_line_to_width(&l, width))
        .collect();
    f.render_widget(Paragraph::new(visible), inner);
}

/// Side-by-side: old on the left, new on the right, sharing one scroll offset
/// (the columns have equal row counts). The pane is split evenly with a 1-col
/// divider; each column clips independently.
fn render_diff_side_by_side(f: &mut Frame, inner: Rect, app: &mut App) {
    let body = inner.height as usize;
    let rows = app.diff_side_rows().to_vec();
    let scroll = clamp_scroll(app.diff_scroll, rows.len(), body);
    app.diff_scroll = scroll;

    let cols = Layout::horizontal([
        Constraint::Length(inner.width.saturating_sub(1) / 2),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);
    let (left_area, sep_area, right_area) = (cols[0], cols[1], cols[2]);
    let left_w = left_area.width as usize;
    let right_w = right_area.width as usize;

    let mut left_lines: Vec<Line> = Vec::new();
    let mut right_lines: Vec<Line> = Vec::new();
    for row in rows.iter().skip(scroll).take(body) {
        left_lines.push(clip_line_to_width(&side_cell_to_line(row.left.as_ref()), left_w));
        right_lines.push(clip_line_to_width(&side_cell_to_line(row.right.as_ref()), right_w));
    }
    let sep: Vec<Line> = (0..sep_area.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(Color::DarkGray))))
        .collect();
    f.render_widget(Paragraph::new(left_lines), left_area);
    f.render_widget(Paragraph::new(sep), sep_area);
    f.render_widget(Paragraph::new(right_lines), right_area);
}
```

Note: `clamp_scroll`, `Layout`, `Constraint`, `Span`, `Style`, `Color`, `Line`, `Paragraph` are already imported at the top of `src/ui.rs`.

- [ ] **Step 6: Run the new tests to verify they pass**

Run: `cargo test footer_advertises_the_diff_toggle_key side_by_side_shows block_view_still_renders`
Expected: PASS (3 tests).

- [ ] **Step 7: Commit**

```bash
git add src/ui.rs
git commit -m "feat(ui): render side-by-side diff and advertise d key"
```

---

### Task 10: Docs + full verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the key in the README**

Open `README.md`, find the key/usage list (where `e` and the navigation keys are documented), and add a line describing `d`:
```markdown
- `d` — toggle the diff pane between side-by-side (default) and block (before/after) views
```
Match the surrounding markdown style (bullet vs table) exactly; if the existing keys are in a table, add a matching table row instead.

- [ ] **Step 2: Run the full suite and clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document the d diff-view toggle"
```

---

### Task 11: Open PR #2 and cross-link

**Files:** none

- [ ] **Step 1: Push the branch**

```bash
git push -u origin HEAD
```

- [ ] **Step 2: Open the PR, cross-linking PR #1**

```bash
gh pr create --title "feat: toggle to a side-by-side diff view" \
  --body "Adds a 'd' key that toggles the diff pane between today's before/after block view and a new, default side-by-side diff (old | new, aligned line-by-line, only changed lines colored). Builds on chronox library PR <PR1_URL> (rev bumped in Cargo.toml). Spec: docs/superpowers/specs/2026-06-08-side-by-side-diff-toggle-design.md"
```
Substitute `<PR1_URL>` recorded in Task 5. Expected: prints PR #2's URL.

- [ ] **Step 3: Cross-link from PR #1**

Add a comment on PR #1 pointing at PR #2 so they merge in order (library first):
```bash
gh pr comment <PR1_URL> --body "TUI consumer: <PR2_URL>"
```

- [ ] **Step 4: STOP for review**

Tell the user both PRs are open and cross-linked, and that they should merge PR #1 (library) before PR #2 (TUI).

---

## Self-Review Notes

- **Spec coverage:** `d` toggle (Tasks 7–9), side-by-side default (Task 7 Step 3/5), library LCS + neutral model (Task 2), render helper (Task 3), exports (Task 4), even-split + clip narrow behavior (Task 9 Step 5), blank left gutter + right numbering (Task 2 Step 3), Write/None handling (Task 2 tests), two-PR rev-bump sequencing (Tasks 5–6, 11), docs (Task 10) — all covered.
- **Type consistency:** `CellKind`/`DiffCell`/`SideRow`/`change_detail_side_by_side`/`side_cell_to_line`/`DiffView`/`AppAction::ToggleDiffView`/`diff_side_rows`/`build_side_rows` are used consistently across tasks and exported in Task 4.
- **Narrow panes (accepted):** even split + independent clipping, no auto-fallback (spec §"Narrow-pane tradeoff").
