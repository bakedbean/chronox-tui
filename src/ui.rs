//! Drawing for chronox. Reads `App`; the only state it writes back is
//! layout-derived (`last_area`, scroll offsets, visible-row counts).

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::render::{clip_line_to_width, edit_line, header_line, relative_display, side_cell_to_line};
use sessionx::nav::{adjust_scroll, clamp_scroll};

use crate::app::{App, DiffView, Focus, VisibleRow};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.last_area = area;
    app.reclamp_split();

    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Min(1),    // body
        Constraint::Length(1), // footer
    ])
    .split(area);
    let (title_area, body, footer) = (chunks[0], chunks[1], chunks[2]);

    render_status_strip(f, title_area, app);
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

const SPINNER: [&str; 9] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇"];

fn render_status_strip(f: &mut Frame, area: Rect, app: &App) {
    let green = Style::default().fg(Color::Green);
    let dim = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);
    let (add, del) = app.session_totals();
    let n = app.events().len();
    let m = app.groups().len();
    let spin = SPINNER[app.spinner_frame % SPINNER.len()];

    let line = Line::from(vec![
        Span::styled("● ", green),
        Span::styled("chronox  ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(app.worktree.display().to_string(), dim),
        Span::raw("   "),
        Span::styled(format!("{spin} live"), green),
        Span::styled(" · polling 1s", dim),
        Span::raw("   "),
        Span::styled(format!("+{add}"), green),
        Span::styled(format!(" -{del}"), Style::default().fg(Color::Red)),
        Span::raw("   "),
        Span::styled(format!("{n} changes · {m} files"), dim),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    // A transient status (e.g. an editor-launch error) takes over the footer
    // until the next keypress; otherwise show the key hints.
    let text: &str = match app.status() {
        Some(status) => status,
        None => match app.focus {
            Focus::List => " ↑↓ move · e edit · d view · Tab focus diff · [ ] resize · q quit ",
            Focus::Diff => {
                " ↑↓/PgUp/PgDn scroll · e edit · d view · Tab focus list · [ ] resize · q quit "
            }
        },
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text,
            Style::default().add_modifier(Modifier::DIM),
        ))),
        area,
    );
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let border_style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(format!(" {title} "), border_style))
}

fn render_list(f: &mut Frame, area: Rect, app: &mut App) {
    let block = pane_block("chronox · by file", app.focus == Focus::List);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = inner.height as usize;
    let len = app.visible().len();
    let scroll = clamp_scroll(
        adjust_scroll(app.list_scroll, app.selected, rows, len),
        len,
        rows,
    );
    app.list_scroll = scroll;
    app.last_visible_rows = rows;

    let sel = app.selected;
    let active = app.active_group;
    let width = inner.width;
    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in app.visible().iter().enumerate().skip(scroll).take(rows) {
        let line = match *row {
            VisibleRow::Header { group } => {
                let g = &app.groups()[group];
                let rel = relative_display(&g.file, &app.worktree);
                header_line(
                    &rel,
                    g.add,
                    g.del,
                    g.event_idxs.len(),
                    g.is_new,
                    group == active,
                    group == active,
                    width,
                    i == sel,
                )
            }
            VisibleRow::Edit { event } => {
                let g = &app.groups()[active];
                let last = g.event_idxs.last() == Some(&event);
                let (add, del) = app.event_counts(event);
                let ev = &app.events()[event];
                edit_line(
                    ev.timestamp_ms,
                    add,
                    del,
                    &ev.summary,
                    last,
                    i == sel,
                    width,
                )
            }
        };
        lines.push(clip_line_to_width(&line, width as usize));
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

    match app.diff_view {
        DiffView::Block => render_diff_block(f, inner, app),
        DiffView::SideBySide => render_diff_side_by_side(f, inner, app),
    }
}

/// Today's view: removed (red) block then added (green) block, one column.
fn render_diff_block(f: &mut Frame, inner: Rect, app: &mut App) {
    let body = inner.height as usize;
    let width = inner.width as usize;
    // Clamp against the row count first (a cheap cached call), then re-borrow
    // the cached slice to clip only the visible window — avoids cloning the
    // whole diff buffer every frame.
    let scroll = clamp_scroll(app.diff_scroll, app.diff_lines().len(), body);
    app.diff_scroll = scroll;

    let visible: Vec<Line> = app
        .diff_lines()
        .iter()
        .skip(scroll)
        .take(body)
        .map(|l| clip_line_to_width(l, width))
        .collect();
    f.render_widget(Paragraph::new(visible), inner);
}

/// Side-by-side: old on the left, new on the right, sharing one scroll offset
/// (the columns have equal row counts). The pane is split evenly with a 1-col
/// divider; each column clips independently.
fn render_diff_side_by_side(f: &mut Frame, inner: Rect, app: &mut App) {
    let body = inner.height as usize;
    // Clamp against the row count first (a cheap cached call), then re-borrow
    // the cached slice below — avoids cloning the whole row vector every frame.
    let scroll = clamp_scroll(app.diff_scroll, app.diff_side_rows().len(), body);
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
    for row in app.diff_side_rows().iter().skip(scroll).take(body) {
        left_lines.push(clip_line_to_width(
            &side_cell_to_line(row.left.as_ref()),
            left_w,
        ));
        right_lines.push(clip_line_to_width(
            &side_cell_to_line(row.right.as_ref()),
            right_w,
        ));
    }
    let sep: Vec<Line> = (0..sep_area.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(Color::DarkGray))))
        .collect();
    f.render_widget(Paragraph::new(left_lines), left_area);
    f.render_widget(Paragraph::new(sep), sep_area);
    f.render_widget(Paragraph::new(right_lines), right_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use sessionx::{ChangeDetail, ChangeEvent, ChangeSource, ChangeTool};
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

    fn ev_named(file: &str, ts: i64, line_index: usize) -> ChangeEvent {
        ChangeEvent {
            timestamp_ms: ts,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from(file),
            summary: "tweak the thing".into(),
            detail: ChangeDetail::Edit {
                old: "old".into(),
                new: "new".into(),
            },
            source: ChangeSource {
                session_file: PathBuf::from("s.jsonl"),
                line_index,
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
    fn footer_advertises_the_edit_key() {
        let mut app = App::bare(PathBuf::from("/wt"));
        let buf = draw_app(&mut app, 80, 10);
        assert!(buffer_text(&buf).contains("e edit"));
    }

    #[test]
    fn footer_shows_status_when_set() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_status("could not launch 'nope'".into());
        let buf = draw_app(&mut app, 80, 10);
        let text = buffer_text(&buf);
        assert!(text.contains("could not launch 'nope'"));
        // The status replaces the hint while it is visible.
        assert!(!text.contains("e edit"));
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
        // Positional check: the removed marker must sit on the same row as, and
        // to the LEFT of, the added marker — otherwise a regression to the
        // single-column block view would still satisfy the contains() asserts.
        let w = 80usize;
        // The list pane renders gauge counts containing '-'/'+', so restrict the
        // positional check to the diff pane (columns right of list + separator).
        let diff_start = app.list_width as usize + 1;
        let in_diff = |idx: usize| idx % w >= diff_start && idx / w > 0;
        let minus = buf
            .content
            .iter()
            .enumerate()
            .find(|(i, c)| c.symbol() == "-" && in_diff(*i))
            .map(|(i, _)| i)
            .expect("a '-' in the diff pane");
        let plus = buf
            .content
            .iter()
            .enumerate()
            .find(|(i, c)| c.symbol() == "+" && in_diff(*i))
            .map(|(i, _)| i)
            .expect("a '+' in the diff pane");
        assert_eq!(minus / w, plus / w, "old and new render on the same row");
        assert!(minus % w < plus % w, "old column is left of new column");
    }

    #[test]
    fn list_shows_file_header_and_nested_edit() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![
            ev_named("/wt/src/app.rs", 0, 1),
            ev_named("/wt/src/ui.rs", 0, 2),
        ]);
        app.list_width = 50; // wide enough for the nested edit summary to render unclipped
        let buf = draw_app(&mut app, 100, 12);
        let text = buffer_text(&buf);
        assert!(text.contains("src/app.rs"), "file header rendered");
        assert!(text.contains("▾"), "active file expanded");
        assert!(text.contains("▸"), "other file folded");
        assert!(text.contains("tweak the thing"), "active file's edit summary shown");
    }

    #[test]
    fn status_strip_shows_live_totals_and_file_count() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![
            ev_named("/wt/src/app.rs", 0, 1),
            ev_named("/wt/src/ui.rs", 0, 2),
        ]);
        let buf = draw_app(&mut app, 100, 12);
        let top: String = (0..100u16).map(|x| buf[(x, 0u16)].symbol()).collect();
        assert!(top.contains("chronox"));
        assert!(top.contains("live"));
        assert!(top.contains("changes"));
        assert!(top.contains("files"));
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
}
