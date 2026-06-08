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
