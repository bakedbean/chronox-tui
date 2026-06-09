//! App state and transitions for chronox-tui. All state changes go through
//! `App::apply`. Timeline parsing, navigation, and styled rendering are
//! delegated to the `chronox` crate; this module is the shell around them.

use ratatui::layout::Rect;
use ratatui::text::Line;
use std::path::PathBuf;

use chronox::extract::{claude_session_files, load_full_change, resolve_line_in_file};
use chronox::nav::nav;
use chronox::{
    ChangeEvent, ChangeSource, NavAction, NavKey, SideRow, Timeline,
    change_detail_lines_styled, change_detail_side_by_side, lang_for_path,
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

/// Which rendering the diff pane uses. Side-by-side is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffView {
    SideBySide,
    Block,
}

/// Every state change is expressed as one of these and applied via `App::apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    Quit,
    Nav(NavKey),
    ToggleFocus,
    ScrollDiff(i32),
    /// Flip the diff pane between side-by-side and block rendering.
    #[allow(dead_code)]
    ToggleDiffView,
    NudgeSplit(i32),
    StartResize,
    Resize(u16),
    EndResize,
    /// Open the selected change's file in `$EDITOR`. Side-effecting: handled by
    /// the run loop (which owns the terminal), not by `App::apply`.
    OpenInEditor,
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
    pub diff_view: DiffView,
    #[allow(dead_code)]
    side_cache: Option<(ChangeSource, Vec<SideRow>)>,
    /// Transient one-line message for the footer (e.g. an editor-launch error),
    /// dismissed on the next keypress.
    status: Option<String>,
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

/// Build the full, un-clipped styled diff for one change. Re-reads the session
/// log for fidelity, falling back to the bounded `detail` when the log is
/// unavailable.
fn build_diff_lines(ev: &ChangeEvent) -> Vec<Line<'static>> {
    let detail = load_full_change(ev).unwrap_or_else(|| ev.detail.clone());
    let base = resolve_line_in_file(&ev.file_path, &detail);
    change_detail_lines_styled(&detail, base, lang_for_path(&ev.file_path))
}

/// Build the full, un-clipped side-by-side rows for one change. Same re-read +
/// base-line resolution as `build_diff_lines`.
#[allow(dead_code)]
fn build_side_rows(ev: &ChangeEvent) -> Vec<SideRow> {
    let detail = load_full_change(ev).unwrap_or_else(|| ev.detail.clone());
    let base = resolve_line_in_file(&ev.file_path, &detail);
    change_detail_side_by_side(&detail, base, lang_for_path(&ev.file_path))
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
            diff_view: DiffView::SideBySide,
            side_cache: None,
            status: None,
            should_quit: false,
        }
    }

    pub fn events(&self) -> &[ChangeEvent] {
        &self.events
    }

    pub fn selected_event(&self) -> Option<&ChangeEvent> {
        self.events.get(self.selected)
    }

    /// Absolute path and 1-based line of the current selection, for handing to
    /// an external editor. Reuses the same full-change + line-resolution path
    /// the diff view uses, so the editor lands on the line the diff shows.
    /// `resolve_line_in_file` returns 1 when the file is unreadable, so the
    /// line is always >= 1.
    pub fn selected_path_and_line(&self) -> Option<(PathBuf, u32)> {
        let ev = self.events.get(self.selected)?;
        let detail = load_full_change(ev).unwrap_or_else(|| ev.detail.clone());
        let line = resolve_line_in_file(&ev.file_path, &detail);
        Some((ev.file_path.clone(), line))
    }

    /// The transient footer message, if any.
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Set the transient footer message (shown until the next keypress).
    pub fn set_status(&mut self, msg: String) {
        self.status = Some(msg);
    }

    /// Clear the transient footer message.
    pub fn clear_status(&mut self) {
        self.status = None;
    }
}

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
            AppAction::ToggleDiffView => {
                self.diff_view = match self.diff_view {
                    DiffView::SideBySide => DiffView::Block,
                    DiffView::Block => DiffView::SideBySide,
                };
                self.diff_scroll = 0;
            }
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
            // Handled by the run loop, which owns the terminal; no state change.
            AppAction::OpenInEditor => {}
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

    /// Re-clamp the split width to the current `last_area` — called each frame so
    /// a terminal resize (or an oversized default on first draw) can't push the
    /// divider off-screen or squeeze the diff pane below its minimum.
    pub fn reclamp_split(&mut self) {
        self.resize_to(self.list_width);
    }
}

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
        // Safe to key on ChangeSource: Claude Code session logs are append-only,
        // so a given (file, line, index) is written once and never mutated.
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

    /// Styled side-by-side rows for the current selection, cached by the
    /// selected change's `ChangeSource` (mirrors `diff_lines`).
    #[allow(dead_code)]
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
        let old_events = [ev(2, "/wt/a.rs", 1), ev(1, "/wt/b.rs", 2)];
        let pinned = old_events[1].source.clone(); // selected b.rs at index 1
        let new_events = vec![
            ev(3, "/wt/new.rs", 9),
            ev(2, "/wt/a.rs", 1),
            ev(1, "/wt/b.rs", 2),
        ];
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
        assert!(
            !lines.is_empty(),
            "fallback detail must still render a diff"
        );
    }

    #[test]
    fn diff_lines_empty_when_no_events() {
        let mut app = App::bare(PathBuf::from("/wt"));
        assert!(app.diff_lines().is_empty());
    }

    #[test]
    fn selected_path_and_line_returns_path_and_line() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev(1, "/wt/a.rs", 1)]);
        // The source log is absent, so resolve_line_in_file falls back to 1.
        let (path, line) = app.selected_path_and_line().expect("a selection exists");
        assert_eq!(path, PathBuf::from("/wt/a.rs"));
        assert_eq!(line, 1);
    }

    #[test]
    fn selected_path_and_line_none_when_no_events() {
        let app = App::bare(PathBuf::from("/wt"));
        assert!(app.selected_path_and_line().is_none());
    }

    #[test]
    fn status_sets_and_clears() {
        let mut app = App::bare(PathBuf::from("/wt"));
        assert_eq!(app.status(), None);
        app.set_status("nope".into());
        assert_eq!(app.status(), Some("nope"));
        app.clear_status();
        assert_eq!(app.status(), None);
    }

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

    #[test]
    fn reclamp_split_shrinks_to_fit_small_area() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.list_width = 32; // the default
        app.last_area = Rect::new(0, 0, 40, 10); // max = 40 - MIN_DIFF(24) - 1 = 15 → floored to MIN_LIST
        app.reclamp_split();
        assert_eq!(app.list_width, MIN_LIST);
        // A comfortably wide area leaves the width untouched.
        app.list_width = 32;
        app.last_area = Rect::new(0, 0, 120, 10);
        app.reclamp_split();
        assert_eq!(app.list_width, 32);
    }
}
