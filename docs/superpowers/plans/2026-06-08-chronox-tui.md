# chronox-tui Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone ratatui terminal app that renders the chronox change-timeline frontend — a live master-detail view of a worktree's Claude Code file changes with a syntax-highlighted diff.

**Architecture:** A synchronous crossterm event loop. State and transitions live in `app.rs` (pure, unit-tested); drawing in `ui.rs`; key/mouse→action mapping in `input.rs`; terminal lifecycle and the loop in `main.rs`. All timeline parsing, navigation, and styled-line rendering is delegated to the `chronox` crate — this app is only the shell.

**Tech Stack:** Rust (edition 2024), ratatui 0.29 (+ its bundled crossterm 0.28, used via `ratatui::crossterm`), chronox (git dep pinned to commit `deacc9a`).

---

## File Structure

```
chronox-tui/
  Cargo.toml          — package + deps (ratatui, chronox git dep)
  .gitignore          — /target
  README.md           — what it is, how to run, the mouse-capture note
  src/
    main.rs           — CLI arg, terminal setup/teardown, panic hook, run loop
    app.rs            — App state, AppAction, apply()/transitions, refresh(), diff cache
    ui.rs             — draw(frame, &mut app): layout + widgets
    input.rs          — map(Event, &App) -> AppAction
  docs/superpowers/
    specs/2026-06-08-chronox-tui-design.md
    plans/2026-06-08-chronox-tui.md
```

**Module responsibilities**
- `app.rs` — owns all state. Every state change goes through `App::apply(AppAction)`. Holds an owned `Vec<ChangeEvent>` snapshot (refreshed from the chronox `Timeline`) so logic is testable without touching the filesystem. Pure helpers (`repin`, `build_diff_lines`) are free functions.
- `ui.rs` — reads `App`, draws. Its only mutation is recording layout-derived state back onto `App` (`last_area`, `list_scroll`, `last_visible_rows`, clamped scrolls).
- `input.rs` — translates a crossterm `Event` into an `AppAction`, including the divider hit-test for mouse resize. No state mutation.
- `main.rs` — terminal raw mode / alternate screen / mouse capture, panic-safe restore, the poll+tick loop.

---

## Task 1: Project scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `src/main.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "chronox-tui"
version = "0.1.0"
edition = "2024"
description = "A ratatui TUI for the chronox change timeline."

[dependencies]
ratatui = "0.29"
chronox = { git = "https://github.com/bakedbean/chronox", rev = "deacc9ad8698acd95dbb1fb0aea35b1becf5e8bd" }

[dev-dependencies]
tempfile = "3"
```

Note: crossterm is used via `ratatui::crossterm` (ratatui 0.29 bundles crossterm 0.28), so it is **not** a separate dependency — this guarantees the versions match.

- [ ] **Step 2: Write `.gitignore`**

```
/target
```

- [ ] **Step 3: Write a stub `src/main.rs` so the crate compiles**

```rust
mod app;
mod input;
mod ui;

fn main() {
    println!("chronox-tui scaffold");
}
```

- [ ] **Step 4: Create empty module files so `mod` lines resolve**

Create `src/app.rs`, `src/input.rs`, `src/ui.rs` each containing a single line:

```rust
// implemented in later tasks
```

- [ ] **Step 5: Build to fetch the git dep and verify it compiles**

Run: `cargo build`
Expected: PASS (downloads chronox at the pinned rev; warnings about unused modules are fine).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore src/
git commit -m "chore: scaffold chronox-tui crate"
```

---

## Task 2: App state, types, and the `repin` helper

**Files:**
- Modify: `src/app.rs` (replace stub)

This task defines the core types, the no-IO constructor `App::bare` used throughout the tests, and the pure selection-pinning helper `repin`, with its tests.

- [ ] **Step 1: Write the failing tests for `repin`**

Put this complete content in `src/app.rs`:

```rust
//! App state and transitions for chronox-tui. All state changes go through
//! `App::apply`. Timeline parsing, navigation, and styled rendering are
//! delegated to the `chronox` crate; this module is the shell around them.

use ratatui::layout::Rect;
use ratatui::text::Line;
use std::path::PathBuf;

use chronox::extract::{claude_session_files, load_full_change, resolve_line_in_file};
use chronox::nav::nav;
use chronox::{
    ChangeEvent, ChangeSource, NavAction, NavKey, Timeline, change_detail_lines_styled,
    lang_for_path,
};

/// Default columns for the left (list) pane.
pub const DEFAULT_LIST_WIDTH: u16 = 32;
/// Minimum columns the list pane may shrink to.
pub const MIN_LIST: u16 = 16;
/// Minimum columns the diff pane must keep.
pub const MIN_DIFF: u16 = 24;

/// Which pane currently receives ↑/↓.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Diff,
}

/// Every state change is expressed as one of these and applied via `App::apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    Quit,
    Nav(NavKey),
    ToggleFocus,
    ScrollDiff(i32),
    NudgeSplit(i32),
    StartResize,
    Resize(u16),
    EndResize,
    Tick,
    None,
}

pub struct App {
    pub worktree: PathBuf,
    timeline: Timeline,
    events: Vec<ChangeEvent>,
    pub selected: usize,
    pub focus: Focus,
    pub diff_scroll: usize,
    pub list_scroll: usize,
    pub list_width: u16,
    pub resizing: bool,
    pub last_area: Rect,
    pub last_visible_rows: usize,
    diff_cache: Option<(ChangeSource, Vec<Line<'static>>)>,
    pub should_quit: bool,
}

/// Pure selection re-pin: given the freshly merged `events`, the source of the
/// previously-selected change, and the old index, return the index to select.
/// Keeps the cursor on the same change when new changes are prepended; clamps
/// when that change is gone.
fn repin(events: &[ChangeEvent], pinned: Option<&ChangeSource>, old: usize) -> usize {
    if let Some(src) = pinned
        && let Some(idx) = events.iter().position(|e| &e.source == src)
    {
        return idx;
    }
    old.min(events.len().saturating_sub(1))
}

impl App {
    /// Construct without touching the filesystem. `new` wraps this and refreshes.
    pub(crate) fn bare(worktree: PathBuf) -> Self {
        App {
            worktree,
            timeline: Timeline::default(),
            events: Vec::new(),
            selected: 0,
            focus: Focus::List,
            diff_scroll: 0,
            list_scroll: 0,
            list_width: DEFAULT_LIST_WIDTH,
            resizing: false,
            last_area: Rect::default(),
            last_visible_rows: 0,
            diff_cache: None,
            should_quit: false,
        }
    }

    pub fn events(&self) -> &[ChangeEvent] {
        &self.events
    }

    pub fn selected_event(&self) -> Option<&ChangeEvent> {
        self.events.get(self.selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronox::{ChangeDetail, ChangeTool};

    fn ev(ts: i64, file: &str, line_index: usize) -> ChangeEvent {
        ChangeEvent {
            timestamp_ms: ts,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from(file),
            summary: String::new(),
            detail: ChangeDetail::Edit {
                old: "a".into(),
                new: "b".into(),
            },
            source: ChangeSource {
                session_file: PathBuf::from("s.jsonl"),
                line_index,
                index_in_line: 0,
            },
        }
    }

    #[test]
    fn repin_keeps_same_event_when_new_events_prepended() {
        let old_events = vec![ev(2, "/wt/a.rs", 1), ev(1, "/wt/b.rs", 2)];
        let pinned = old_events[1].source.clone(); // selected b.rs at index 1
        let new_events = vec![ev(3, "/wt/new.rs", 9), ev(2, "/wt/a.rs", 1), ev(1, "/wt/b.rs", 2)];
        assert_eq!(repin(&new_events, Some(&pinned), 1), 2);
    }

    #[test]
    fn repin_clamps_when_event_gone() {
        let new_events = vec![ev(3, "/wt/new.rs", 9)];
        let gone = ChangeSource {
            session_file: PathBuf::from("x.jsonl"),
            line_index: 99,
            index_in_line: 0,
        };
        assert_eq!(repin(&new_events, Some(&gone), 5), 0);
    }

    #[test]
    fn repin_empty_is_zero() {
        assert_eq!(repin(&[], None, 3), 0);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail to compile / fail**

Run: `cargo test --lib 2>&1 | head -40` (or `cargo test repin`)
Expected: At this point the code imports `nav`, `change_detail_lines_styled`, `lang_for_path`, `load_full_change`, `resolve_line_in_file`, `NavAction`, `claude_session_files` which are not yet used (they are consumed in Tasks 3–4) — this produces **unused-import warnings**, not errors, and the three `repin_*` tests should **PASS**. If instead you get an import *error* (not warning), the chronox API path is wrong — fix the `use` path before continuing.

Run: `cargo test repin`
Expected: 3 passed.

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): core types, bare constructor, repin helper"
```

(The imports flagged as unused are consumed in Tasks 3–4. Leave them.)

---

## Task 3: App transitions (`apply` and friends)

**Files:**
- Modify: `src/app.rs` (add transition methods + tests)

- [ ] **Step 1: Write the failing tests**

Add these test functions inside the existing `mod tests` block in `src/app.rs` (after the `repin_*` tests):

```rust
    #[test]
    fn list_focus_moves_selection() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![
            ev(3, "/wt/a.rs", 1),
            ev(2, "/wt/b.rs", 2),
            ev(1, "/wt/c.rs", 3),
        ]);
        app.focus = Focus::List;
        app.apply(AppAction::Nav(NavKey::Down));
        assert_eq!(app.selected, 1);
        app.apply(AppAction::Nav(NavKey::Bottom));
        assert_eq!(app.selected, 2);
        app.apply(AppAction::Nav(NavKey::Top));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn moving_selection_resets_diff_scroll() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev(2, "/wt/a.rs", 1), ev(1, "/wt/b.rs", 2)]);
        app.focus = Focus::List;
        app.diff_scroll = 7;
        app.apply(AppAction::Nav(NavKey::Down));
        assert_eq!(app.diff_scroll, 0);
    }

    #[test]
    fn diff_focus_routes_arrows_to_scroll() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev(1, "/wt/a.rs", 1)]);
        app.focus = Focus::Diff;
        app.diff_scroll = 3;
        app.apply(AppAction::Nav(NavKey::Up));
        assert_eq!(app.diff_scroll, 2);
        assert_eq!(app.selected, 0, "diff focus must not move the list");
        app.apply(AppAction::Nav(NavKey::Down));
        assert_eq!(app.diff_scroll, 3);
    }

    #[test]
    fn scroll_diff_floors_at_zero() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.diff_scroll = 0;
        app.apply(AppAction::ScrollDiff(-5));
        assert_eq!(app.diff_scroll, 0);
        app.apply(AppAction::ScrollDiff(4));
        assert_eq!(app.diff_scroll, 4);
    }

    #[test]
    fn toggle_focus_flips() {
        let mut app = App::bare(PathBuf::from("/wt"));
        assert_eq!(app.focus, Focus::List);
        app.apply(AppAction::ToggleFocus);
        assert_eq!(app.focus, Focus::Diff);
        app.apply(AppAction::ToggleFocus);
        assert_eq!(app.focus, Focus::List);
    }

    #[test]
    fn esc_and_quit_set_should_quit() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.apply(AppAction::Nav(NavKey::Esc));
        assert!(app.should_quit);
        let mut app2 = App::bare(PathBuf::from("/wt"));
        app2.apply(AppAction::Quit);
        assert!(app2.should_quit);
    }

    #[test]
    fn resize_clamps_to_bounds() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.last_area = Rect::new(0, 0, 100, 30); // max = 100 - MIN_DIFF(24) - 1 = 75
        app.apply(AppAction::Resize(5)); // below MIN_LIST
        assert_eq!(app.list_width, MIN_LIST);
        app.apply(AppAction::Resize(90)); // above max
        assert_eq!(app.list_width, 75);
        app.apply(AppAction::Resize(40));
        assert_eq!(app.list_width, 40);
    }

    #[test]
    fn nudge_split_respects_bounds() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.last_area = Rect::new(0, 0, 100, 30);
        app.list_width = MIN_LIST;
        app.apply(AppAction::NudgeSplit(-1));
        assert_eq!(app.list_width, MIN_LIST, "cannot go below MIN_LIST");
        app.apply(AppAction::NudgeSplit(1));
        assert_eq!(app.list_width, MIN_LIST + 1);
    }

    #[test]
    fn resize_flag_transitions() {
        let mut app = App::bare(PathBuf::from("/wt"));
        assert!(!app.resizing);
        app.apply(AppAction::StartResize);
        assert!(app.resizing);
        app.apply(AppAction::EndResize);
        assert!(!app.resizing);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib 2>&1 | head -30`
Expected: FAIL — `set_events_for_test`, `apply` not found.

- [ ] **Step 3: Implement the transitions**

Add this `impl App` block in `src/app.rs` (after the existing `impl App` from Task 2, before `#[cfg(test)]`):

```rust
impl App {
    /// Test seam — crate-visible so `ui.rs` and `input.rs` tests can seed events
    /// without touching the filesystem.
    #[cfg(test)]
    pub(crate) fn set_events_for_test_pub(&mut self, events: Vec<ChangeEvent>) {
        self.events = events;
    }

    /// Single entry point for all state changes.
    pub fn apply(&mut self, action: AppAction) {
        match action {
            AppAction::Quit => self.should_quit = true,
            AppAction::Tick => self.refresh(),
            AppAction::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::List => Focus::Diff,
                    Focus::Diff => Focus::List,
                };
            }
            AppAction::Nav(key) => self.on_nav(key),
            AppAction::ScrollDiff(delta) => self.scroll_diff(delta),
            AppAction::NudgeSplit(delta) => {
                let target = (self.list_width as i32 + delta).max(0) as u16;
                self.resize_to(target);
            }
            AppAction::StartResize => self.resizing = true,
            AppAction::Resize(col) => {
                let target = col.saturating_sub(self.last_area.x);
                self.resize_to(target);
            }
            AppAction::EndResize => self.resizing = false,
            AppAction::None => {}
        }
    }

    fn on_nav(&mut self, key: NavKey) {
        // In Diff focus, ↑/↓ scroll the diff instead of moving the list.
        if self.focus == Focus::Diff {
            match key {
                NavKey::Up => return self.scroll_diff(-1),
                NavKey::Down => return self.scroll_diff(1),
                NavKey::Esc => {
                    self.should_quit = true;
                    return;
                }
                _ => {}
            }
        }
        let (new_sel, act) = nav(self.selected, key, self.events.len());
        match act {
            NavAction::Exit => self.should_quit = true,
            NavAction::Open(_) => self.focus = Focus::Diff,
            NavAction::None => {}
        }
        if new_sel != self.selected {
            self.selected = new_sel;
            self.diff_scroll = 0;
        }
    }

    fn scroll_diff(&mut self, delta: i32) {
        let next = self.diff_scroll as i64 + delta as i64;
        self.diff_scroll = next.max(0) as usize;
        // The upper bound is clamped against the rendered diff length at draw time.
    }

    fn resize_to(&mut self, target: u16) {
        let max = self
            .last_area
            .width
            .saturating_sub(MIN_DIFF + 1)
            .max(MIN_LIST);
        self.list_width = target.clamp(MIN_LIST, max);
    }
}
```

Note: `apply` calls `self.refresh()` for `Tick`; `refresh` is added in Task 4. Add a temporary stub so this task compiles on its own — put this with the methods above:

```rust
impl App {
    // Temporary stub; real implementation lands in Task 4.
    fn refresh(&mut self) {}
}
```

- [ ] **Step 4: Run to verify passing**

Run: `cargo test --lib`
Expected: PASS (all Task 2 + Task 3 tests). Unused-import warnings for the diff/render helpers remain — resolved in Task 4.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): apply() transitions — nav, focus, scroll, resize"
```

---

## Task 4: Timeline refresh + lazy diff cache

**Files:**
- Modify: `src/app.rs` (replace the `refresh` stub, add `diff_lines`, `new`, `build_diff_lines`; add tests)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/app.rs`:

```rust
    #[test]
    fn new_on_empty_worktree_has_no_events() {
        let dir = tempfile::TempDir::new().unwrap();
        let app = App::new(dir.path().to_path_buf());
        assert!(app.events().is_empty());
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn diff_lines_falls_back_to_detail_when_log_missing() {
        let mut app = App::bare(PathBuf::from("/wt"));
        // source.session_file does not exist → load_full_change returns None →
        // we fall back to ev.detail (an Edit), which yields at least one diff line.
        app.set_events_for_test_pub(vec![ev(1, "/wt/a.rs", 1)]);
        let lines = app.diff_lines().to_vec();
        assert!(!lines.is_empty(), "fallback detail must still render a diff");
    }

    #[test]
    fn diff_lines_empty_when_no_events() {
        let mut app = App::bare(PathBuf::from("/wt"));
        assert!(app.diff_lines().is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib 2>&1 | head -20`
Expected: FAIL — `App::new` and `diff_lines` not found.

- [ ] **Step 3: Implement refresh, new, diff cache**

In `src/app.rs`, **replace** the temporary `refresh` stub from Task 3 with the real implementations, and add `new`, `diff_lines`, and the free function `build_diff_lines`:

```rust
impl App {
    /// Construct for `worktree` and load its current timeline.
    pub fn new(worktree: PathBuf) -> Self {
        let mut app = Self::bare(worktree);
        app.refresh();
        app
    }

    /// Re-scan the worktree's session logs, rebuild the merged event list, and
    /// re-pin the cursor to the same change. Cheap to call on a tick — the
    /// chronox `Timeline` reparses only files whose size/mtime changed.
    fn refresh(&mut self) {
        let pinned = self.events.get(self.selected).map(|e| e.source.clone());
        let files = claude_session_files(&self.worktree);
        self.timeline.refresh(&files);
        self.events = self.timeline.events().to_vec();
        self.selected = repin(&self.events, pinned.as_ref(), self.selected);
    }

    /// Styled diff lines for the current selection, built lazily and cached by
    /// the selected change's `ChangeSource` (robust across refresh + selection
    /// changes).
    pub fn diff_lines(&mut self) -> &[Line<'static>] {
        let src = self.events.get(self.selected).map(|e| e.source.clone());
        let needs = match (&self.diff_cache, &src) {
            (Some((cached, _)), Some(s)) => cached != s,
            _ => true,
        };
        if needs {
            match src {
                Some(s) => {
                    let lines = self
                        .events
                        .get(self.selected)
                        .map(build_diff_lines)
                        .unwrap_or_default();
                    self.diff_cache = Some((s, lines));
                }
                None => self.diff_cache = None,
            }
        }
        self.diff_cache
            .as_ref()
            .map(|(_, l)| l.as_slice())
            .unwrap_or(&[])
    }
}

/// Build the full, un-clipped styled diff for one change. Re-reads the session
/// log for fidelity, falling back to the bounded `detail` when the log is
/// unavailable.
fn build_diff_lines(ev: &ChangeEvent) -> Vec<Line<'static>> {
    let detail = load_full_change(ev).unwrap_or_else(|| ev.detail.clone());
    let base = resolve_line_in_file(&ev.file_path, &detail);
    change_detail_lines_styled(&detail, base, lang_for_path(&ev.file_path))
}
```

Also delete the temporary `fn refresh(&mut self) {}` stub added in Task 3 (this task provides the real one).

- [ ] **Step 4: Run to verify passing**

Run: `cargo test --lib`
Expected: PASS. No more unused-import warnings from `app.rs` (every chronox helper is now used).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): timeline refresh with selection pinning + diff cache"
```

---

## Task 5: Rendering (`ui.rs`)

**Files:**
- Modify: `src/ui.rs` (replace stub)

- [ ] **Step 1: Write the implementation with its tests**

Replace `src/ui.rs` with this complete content:

```rust
//! Drawing for chronox-tui. Reads `App`; the only state it writes back is
//! layout-derived (`last_area`, scroll offsets, visible-row counts).

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use chronox::nav::{adjust_scroll, clamp_scroll};
use chronox::{clip_line_to_width, entry_lines, relative_display};

use crate::app::{App, Focus};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.last_area = area;

    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Min(1),    // body
        Constraint::Length(1), // footer
    ])
    .split(area);
    let (title_area, body, footer) = (chunks[0], chunks[1], chunks[2]);

    render_title(f, title_area, app);
    render_footer(f, footer, app);

    if app.events().is_empty() {
        let msg = format!(
            "No changes recorded for {} — run a Claude Code session here.",
            app.worktree.display()
        );
        f.render_widget(Paragraph::new(msg).alignment(Alignment::Center), body);
        return;
    }

    let cols = Layout::horizontal([
        Constraint::Length(app.list_width),
        Constraint::Length(1), // separator / drag handle
        Constraint::Min(0),
    ])
    .split(body);
    render_list(f, cols[0], app);
    render_separator(f, cols[1], app);
    render_diff(f, cols[2], app);
}

fn render_title(f: &mut Frame, area: Rect, app: &App) {
    let title = format!("chronox — {}", app.worktree.display());
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))),
        area,
    );
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let hint = match app.focus {
        Focus::List => " ↑↓ move · Tab focus diff · [ ] resize · q quit ",
        Focus::Diff => " ↑↓/PgUp/PgDn scroll · Tab focus list · [ ] resize · q quit ",
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().add_modifier(Modifier::DIM),
        ))),
        area,
    );
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let border_style = if focused {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(format!(" {title} "), border_style))
}

fn render_list(f: &mut Frame, area: Rect, app: &mut App) {
    let block = pane_block("timeline", app.focus == Focus::List);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = inner.height as usize;
    let len = app.events().len();
    let scroll = clamp_scroll(adjust_scroll(app.list_scroll, app.selected, rows, len), len, rows);
    app.list_scroll = scroll;
    app.last_visible_rows = rows;

    let sel = app.selected;
    let width = inner.width;
    let mut lines: Vec<Line> = Vec::new();
    for (i, ev) in app.events().iter().enumerate().skip(scroll).take(rows) {
        for line in entry_lines(ev, &app.worktree, width, i == sel) {
            lines.push(clip_line_to_width(&line, width as usize));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_separator(f: &mut Frame, area: Rect, app: &App) {
    let style = if app.resizing {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let lines: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", style)))
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use chronox::{ChangeDetail, ChangeEvent, ChangeSource, ChangeTool};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use std::path::PathBuf;

    fn ev(file: &str) -> ChangeEvent {
        ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from(file),
            summary: String::new(),
            detail: ChangeDetail::Edit {
                old: "old".into(),
                new: "new".into(),
            },
            source: ChangeSource {
                session_file: PathBuf::from("s.jsonl"),
                line_index: 0,
                index_in_line: 0,
            },
        }
    }

    fn buffer_text(buf: &Buffer) -> String {
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    fn draw_app(app: &mut App, w: u16, h: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| super::draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn empty_state_renders_message() {
        let mut app = App::bare(PathBuf::from("/wt"));
        let buf = draw_app(&mut app, 80, 10);
        assert!(buffer_text(&buf).contains("No changes recorded"));
    }

    #[test]
    fn two_pane_layout_places_separator_at_list_width() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev("/wt/src/main.rs")]);
        app.list_width = 20;
        let buf = draw_app(&mut app, 80, 12);
        // Body starts at y=1 (after the title row); separator column == list_width.
        assert_eq!(buf[(20u16, 3u16)].symbol(), "│");
    }

    #[test]
    fn focus_indicator_colors_active_pane_border() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev("/wt/src/main.rs")]);
        app.focus = Focus::List;
        let buf = draw_app(&mut app, 80, 12);
        // List block top-left corner is at (0, 1) — under the title row.
        assert_eq!(buf[(0u16, 1u16)].fg, Color::Cyan);
    }
}
```

The tests call `app.set_events_for_test_pub(...)` — the crate-visible test seam already defined on `App` in Task 3, reused here from a different module.

- [ ] **Step 2: Run to verify passing**

Run: `cargo test --lib`
Expected: PASS — all `app.rs` and `ui.rs` tests.

If `buf[(20u16, 3u16)]` is not `"│"`, print the buffer to debug:
`cargo test two_pane -- --nocapture` and add a temporary `eprintln!("{}", buffer_text(&buf));` — confirm the separator row index, then fix the literal. (Body begins at y=1; the separator spans all body rows, so any body row works — y=3 is safely inside for height 12.)

- [ ] **Step 3: Commit**

```bash
git add src/app.rs src/ui.rs
git commit -m "feat(ui): master-detail layout, diff pane, separator, empty state"
```

---

## Task 6: Input mapping (`input.rs`)

**Files:**
- Modify: `src/input.rs` (replace stub)

- [ ] **Step 1: Write the implementation with its tests**

Replace `src/input.rs` with this complete content:

```rust
//! Translate a crossterm `Event` into an `AppAction`. No state mutation; the
//! mouse hit-test reads `App::last_area` and `App::list_width` to locate the
//! draggable divider.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use chronox::NavKey;

use crate::app::{App, AppAction};

pub fn map(event: Event, app: &App) -> AppAction {
    match event {
        Event::Key(k) => map_key(k),
        Event::Mouse(m) => map_mouse(m, app),
        _ => AppAction::None,
    }
}

fn map_key(k: KeyEvent) -> AppAction {
    // Ignore key-release / key-repeat events (Windows can deliver them).
    if k.kind != KeyEventKind::Press {
        return AppAction::None;
    }
    if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
        return AppAction::Quit;
    }
    match k.code {
        KeyCode::Char('q') => AppAction::Quit,
        KeyCode::Esc => AppAction::Nav(NavKey::Esc),
        KeyCode::Up | KeyCode::Char('k') => AppAction::Nav(NavKey::Up),
        KeyCode::Down | KeyCode::Char('j') => AppAction::Nav(NavKey::Down),
        KeyCode::Char('g') | KeyCode::Home => AppAction::Nav(NavKey::Top),
        KeyCode::Char('G') | KeyCode::End => AppAction::Nav(NavKey::Bottom),
        KeyCode::Enter => AppAction::Nav(NavKey::Enter),
        KeyCode::Tab => AppAction::ToggleFocus,
        KeyCode::PageUp => AppAction::ScrollDiff(-10),
        KeyCode::PageDown => AppAction::ScrollDiff(10),
        KeyCode::Char('[') => AppAction::NudgeSplit(-1),
        KeyCode::Char(']') => AppAction::NudgeSplit(1),
        _ => AppAction::None,
    }
}

fn map_mouse(m: MouseEvent, app: &App) -> AppAction {
    let divider_col = app.last_area.x + app.list_width;
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if m.column.abs_diff(divider_col) <= 1 {
                AppAction::StartResize
            } else {
                AppAction::None
            }
        }
        MouseEventKind::Drag(MouseButton::Left) if app.resizing => AppAction::Resize(m.column),
        MouseEventKind::Up(MouseButton::Left) => AppAction::EndResize,
        MouseEventKind::ScrollDown if m.column > divider_col => AppAction::ScrollDiff(3),
        MouseEventKind::ScrollUp if m.column > divider_col => AppAction::ScrollDiff(-3),
        _ => AppAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::layout::Rect;
    use std::path::PathBuf;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn mouse(kind: MouseEventKind, column: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn q_quits() {
        let app = App::bare(PathBuf::from("/wt"));
        assert_eq!(map(key(KeyCode::Char('q')), &app), AppAction::Quit);
    }

    #[test]
    fn ctrl_c_quits() {
        let app = App::bare(PathBuf::from("/wt"));
        let ev = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(map(ev, &app), AppAction::Quit);
    }

    #[test]
    fn arrows_and_vim_keys_map_to_nav() {
        let app = App::bare(PathBuf::from("/wt"));
        assert_eq!(map(key(KeyCode::Down), &app), AppAction::Nav(NavKey::Down));
        assert_eq!(map(key(KeyCode::Char('j')), &app), AppAction::Nav(NavKey::Down));
        assert_eq!(map(key(KeyCode::Up), &app), AppAction::Nav(NavKey::Up));
        assert_eq!(map(key(KeyCode::Tab), &app), AppAction::ToggleFocus);
        assert_eq!(map(key(KeyCode::Char('[')), &app), AppAction::NudgeSplit(-1));
        assert_eq!(map(key(KeyCode::Char(']')), &app), AppAction::NudgeSplit(1));
    }

    #[test]
    fn key_release_is_ignored() {
        let app = App::bare(PathBuf::from("/wt"));
        let mut ke = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        ke.kind = KeyEventKind::Release;
        assert_eq!(map(Event::Key(ke), &app), AppAction::None);
    }

    #[test]
    fn mouse_down_on_divider_starts_resize() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.last_area = Rect::new(0, 0, 100, 30);
        app.list_width = 30; // divider at column 30
        let ev = mouse(MouseEventKind::Down(MouseButton::Left), 30);
        assert_eq!(map(ev, &app), AppAction::StartResize);
    }

    #[test]
    fn mouse_down_away_from_divider_is_none() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.last_area = Rect::new(0, 0, 100, 30);
        app.list_width = 30;
        let ev = mouse(MouseEventKind::Down(MouseButton::Left), 50);
        assert_eq!(map(ev, &app), AppAction::None);
    }

    #[test]
    fn drag_resizes_only_while_resizing() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.last_area = Rect::new(0, 0, 100, 30);
        app.list_width = 30;
        let drag = mouse(MouseEventKind::Drag(MouseButton::Left), 45);
        assert_eq!(map(drag.clone(), &app), AppAction::None, "not dragging yet");
        app.resizing = true;
        assert_eq!(map(drag, &app), AppAction::Resize(45));
    }

    #[test]
    fn wheel_over_diff_scrolls() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.last_area = Rect::new(0, 0, 100, 30);
        app.list_width = 30; // diff starts right of column 30
        assert_eq!(map(mouse(MouseEventKind::ScrollDown, 60), &app), AppAction::ScrollDiff(3));
        assert_eq!(map(mouse(MouseEventKind::ScrollUp, 60), &app), AppAction::ScrollDiff(-3));
        assert_eq!(map(mouse(MouseEventKind::ScrollDown, 10), &app), AppAction::None);
    }
}
```

- [ ] **Step 2: Run to verify passing**

Run: `cargo test --lib`
Expected: PASS — all input tests plus the earlier suites.

- [ ] **Step 3: Commit**

```bash
git add src/input.rs
git commit -m "feat(input): key + mouse event mapping with divider hit-test"
```

---

## Task 7: Terminal lifecycle + run loop (`main.rs`)

**Files:**
- Modify: `src/main.rs` (replace stub)

This task has no unit tests (it is terminal/IO glue); it is verified by running the app.

- [ ] **Step 1: Replace `src/main.rs`**

```rust
mod app;
mod input;
mod ui;

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use app::{App, AppAction};

type Term = Terminal<CrosstermBackend<io::Stdout>>;

const POLL: Duration = Duration::from_millis(250);
const TICK: Duration = Duration::from_millis(1000);

fn main() -> io::Result<()> {
    let worktree = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let app = App::new(worktree);
    let result = run(&mut terminal, app);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> io::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Term) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Restore the terminal even on panic, so a crash never leaves it wedged.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original(info);
    }));
}

fn run(terminal: &mut Term, mut app: App) -> io::Result<()> {
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        if event::poll(POLL)? {
            let ev: Event = event::read()?;
            app.apply(input::map(ev, &app));
        }
        if last_tick.elapsed() >= TICK {
            app.apply(AppAction::Tick);
            last_tick = Instant::now();
        }
        if app.should_quit {
            return Ok(());
        }
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: PASS, no warnings.

- [ ] **Step 3: Run against a worktree with Claude session logs**

Run: `cargo run -- /home/eben/chronox`
(Use any directory that has had a Claude Code session; `/home/eben/chronox` itself is a good candidate. If it shows the empty state, pick a worktree you've run Claude in.)

Manually verify:
- The timeline list renders on the left, a diff on the right.
- `↑`/`↓` move the selection and the diff updates.
- `Tab` moves focus to the diff; `↑`/`↓` then scroll it; `Tab` returns.
- Dragging the center divider with the mouse resizes the panes; `[` / `]` also nudge it.
- `q` quits cleanly and the terminal is restored (prompt intact, no raw-mode artifacts).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: terminal lifecycle, panic-safe restore, poll+tick run loop"
```

---

## Task 8: README + final verification

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write `README.md`**

```markdown
# chronox-tui

A standalone [ratatui](https://ratatui.rs) terminal UI for
[chronox](https://github.com/bakedbean/chronox): browse the newest-first
timeline of file changes a Claude Code agent made in a worktree, with a
syntax-highlighted diff of the selected change. The timeline updates live while
a session is running.

## Run

```bash
cargo run                      # current directory
cargo run -- /path/to/worktree # an explicit worktree
```

The worktree must have Claude Code session logs
(`~/.claude/projects/<encoded-worktree>/*.jsonl`) — run a Claude Code session in
it (and make a few edits) first, or you'll see the empty state.

## Keys

| Key | Action |
|-----|--------|
| `↑`/`↓`, `k`/`j` | List: move selection · Diff: scroll one line |
| `PgUp`/`PgDn` | Diff: scroll a page |
| `g`/`G`, `Home`/`End` | List: jump to top / bottom |
| `Tab` | Toggle focus between the list and the diff |
| `Enter` | Focus the diff pane |
| `[` / `]` | Nudge the split divider left / right |
| mouse drag on the divider | Resize the split |
| mouse wheel over the diff | Scroll the diff |
| `q`, `Esc`, `Ctrl-C` | Quit |

## Note on mouse capture

The app captures the mouse (to support drag-resize), so your terminal's native
text selection needs the usual modifier (often `Shift`) while chronox-tui is
running.
```

- [ ] **Step 2: Run the full verification suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Expected: `fmt` clean, `clippy` no warnings, all tests pass, release build succeeds. Fix anything that fails before committing.

- [ ] **Step 3: Commit**

```bash
git add README.md src/
git commit -m "docs: add README; fmt + clippy pass"
```

---

## Self-Review notes (for the implementer)

- **Spec coverage check:** master-detail layout (Task 5), CWD/arg target (Task 7 `main`), live poll (Task 4 `refresh` + Task 7 tick), selection pinning (Task 2 `repin` + Task 4), Tab focus (Task 3), diff scroll (Task 3 + Task 5 clamp), mouse drag-resize (Task 3 `resize_to` + Task 6 hit-test), `[`/`]` parity (Task 3/6), panic-safe restore + empty/malformed states (Task 4 fallback, Task 5 empty state, Task 7 hook) — all present.
- **Out of scope (do not add):** chronox `config` module, worktree picker, split-width persistence, search/filter, theming.
- **Type consistency:** `AppAction`/`Focus`/`App` field names are identical across `app.rs`, `ui.rs`, `input.rs`. `repin` and `build_diff_lines` are free functions in `app.rs`. The test seam `set_events_for_test_pub` is defined `pub(crate)` in Task 3 and reused by `ui.rs` tests in Task 5.
- **chronox API used (pinned at `deacc9a`):** `Timeline::{default,refresh,events}`, `nav::{nav,clamp_scroll,adjust_scroll}`, `NavKey`, `NavAction`, `extract::{claude_session_files,load_full_change,resolve_line_in_file}`, `change_detail_lines_styled`, `lang_for_path`, `entry_lines`, `clip_line_to_width`, `relative_display`, `ChangeEvent`, `ChangeSource`, `ChangeDetail`, `ChangeTool`.
```
