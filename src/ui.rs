//! Drawing for chronox. Reads `App`; the only state it writes back is
//! layout-derived (`last_area`, scroll offsets, visible-row counts).

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::render::{
    INACTIVE_FG, clip_line_to_width, edit_line, header_line, relative_display, side_cell_to_line,
};
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

    // The single frame needs room for two border rows + a content row, and a
    // left border + divider + right border. Below that, just bail gracefully
    // rather than underflow the frame geometry.
    if body.height < 3 || body.width < 5 {
        return;
    }

    // Keep `list_width` consistent with the divider `render_frame` actually
    // draws (clamped inside the frame). `reclamp_split` only floors at
    // `MIN_LIST`, so on a terminal narrower than `MIN_LIST + 2` the drawn
    // divider would otherwise diverge from `list_width` — which `input.rs` uses
    // for mouse divider hit-testing and diff-wheel routing.
    app.list_width = app.list_width.min(body.width.saturating_sub(2));

    let (left, right) = render_frame(f, body, app);
    render_list_inner(f, left, app);
    render_diff_inner(f, right, app);
}

fn render_frame(f: &mut Frame, body: Rect, app: &App) -> (Rect, Rect) {
    let faint = Style::default().fg(Color::DarkGray);
    let list_focus = app.focus == Focus::List;
    let left_title_style = if list_focus {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let right_title_style = if list_focus {
        Style::default().fg(Color::White)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };

    let x0 = body.x;
    let y0 = body.y;
    let w = body.width;
    let h = body.height;
    let right_col = (x0 + w).saturating_sub(1);
    // Divider column: list_width cells in from the left edge (matches the mouse
    // hit-test in input.rs, which uses last_area.x + list_width).
    let lo = x0 + 1;
    let hi = right_col.saturating_sub(1).max(lo);
    let dx = (x0 + app.list_width).clamp(lo, hi);

    let left_title = "chronox · by file";
    let right_title = match app.selected_event() {
        Some(ev) => format!(
            "{} · {}",
            relative_display(&ev.file_path, &app.worktree),
            ev.tool.label()
        ),
        None => "—".to_string(),
    };

    // ── top border ────────────────────────────────────────────────────────
    // Built as two fixed-width segments so the `┬` always lands exactly on the
    // divider column `dx` (matching the bottom border and the body divider),
    // regardless of title length. Titles are clipped to their segment width.
    //
    //   left segment  (exactly `left_seg_w` cols):  "┌─ " <title> " " ──…
    //   right segment (exactly `right_seg_w` cols): "┬─ " <title> " " ──… "┐"
    let left_seg_w = (dx.saturating_sub(x0)) as usize;
    let title_budget = left_seg_w.saturating_sub(4); // "┌─ " (3) + trailing " " (1)
    let lt: String = left_title.chars().take(title_budget).collect();
    let fill_left = left_seg_w.saturating_sub(3 + lt.chars().count() + 1);
    let mut top: Vec<Span> = vec![
        Span::styled("┌─ ", faint),
        Span::styled(lt, left_title_style),
        Span::styled(" ", faint),
        Span::styled("─".repeat(fill_left), faint),
    ];

    let right_seg_w = (right_col + 1).saturating_sub(dx) as usize;
    let rtitle_budget = right_seg_w.saturating_sub(5); // "┬─ " (3) + " " (1) + "┐" (1)
    let rt: String = right_title.chars().take(rtitle_budget).collect();
    let fill_right = right_seg_w.saturating_sub(3 + rt.chars().count() + 1 + 1);
    top.push(Span::styled("┬─ ", faint));
    top.push(Span::styled(rt, right_title_style));
    top.push(Span::styled(" ", faint));
    top.push(Span::styled("─".repeat(fill_right), faint));
    top.push(Span::styled("┐", faint));
    f.render_widget(Paragraph::new(Line::from(top)), Rect::new(x0, y0, w, 1));

    // ── bottom border: └─...─┴─...─┘ ──────────────────────────────────────
    let mut bottom: Vec<Span> = vec![Span::styled("└", faint)];
    bottom.push(Span::styled(
        "─".repeat(dx.saturating_sub(x0 + 1) as usize),
        faint,
    ));
    bottom.push(Span::styled("┴", faint));
    bottom.push(Span::styled(
        "─".repeat(right_col.saturating_sub(dx + 1) as usize),
        faint,
    ));
    bottom.push(Span::styled("┘", faint));
    f.render_widget(
        Paragraph::new(Line::from(bottom)),
        Rect::new(x0, (y0 + h).saturating_sub(1), w, 1),
    );

    // ── side + divider columns for the body rows ──────────────────────────
    let divider_style = if app.resizing {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        faint
    };
    for y in (y0 + 1)..(y0 + h).saturating_sub(1) {
        let left_edge: Vec<Line> = vec![Line::from(Span::styled("│", faint))];
        f.render_widget(Paragraph::new(left_edge.clone()), Rect::new(x0, y, 1, 1));
        f.render_widget(
            Paragraph::new(vec![Line::from(Span::styled("│", divider_style))]),
            Rect::new(dx, y, 1, 1),
        );
        f.render_widget(Paragraph::new(left_edge), Rect::new(right_col, y, 1, 1));
    }

    let left = Rect::new(
        x0 + 1,
        y0 + 1,
        dx.saturating_sub(x0 + 1),
        h.saturating_sub(2),
    );
    let right = Rect::new(
        dx + 1,
        y0 + 1,
        right_col.saturating_sub(dx + 1),
        h.saturating_sub(2),
    );
    (left, right)
}

const SPINNER: [&str; 9] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇"];

fn render_status_strip(f: &mut Frame, area: Rect, app: &App) {
    let green = Style::default().fg(Color::Green);
    let dim = Style::default().fg(INACTIVE_FG);
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
            Focus::List => " ↑↓ move · enter diff · d view · e edit · tab focus · q quit ",
            Focus::Diff => " ↑↓/PgUp/PgDn scroll · d view · e edit · tab focus list · q quit ",
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

fn render_list_inner(f: &mut Frame, inner: Rect, app: &mut App) {
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

fn render_diff_inner(f: &mut Frame, inner: Rect, app: &mut App) {
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
    fn single_frame_has_top_and_bottom_divider_junctions() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev_named("/wt/src/main.rs", 0, 1)]);
        app.list_width = 30;
        let buf = draw_app(&mut app, 80, 12);
        // Status strip is y=0; the body frame spans y=1..=10, footer y=11.
        // Divider column = body.x + list_width = 30.
        assert_eq!(buf[(30u16, 1u16)].symbol(), "┬", "top divider junction");
        assert_eq!(buf[(30u16, 10u16)].symbol(), "┴", "bottom divider junction");
        assert_eq!(buf[(30u16, 5u16)].symbol(), "│", "divider body");
    }

    #[test]
    fn frame_titles_label_both_panes() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev_named("/wt/src/main.rs", 0, 1)]);
        let buf = draw_app(&mut app, 80, 12);
        let top: String = (0..80u16).map(|x| buf[(x, 1u16)].symbol()).collect();
        assert!(top.contains("chronox · by file"), "left title");
        assert!(top.contains("main.rs"), "right title shows the file");
    }

    #[test]
    fn footer_lists_grouped_timeline_hints() {
        let mut app = App::bare(PathBuf::from("/wt"));
        let buf = draw_app(&mut app, 100, 10);
        let text = buffer_text(&buf);
        assert!(text.contains("enter diff"));
        assert!(text.contains("e edit"));
        assert!(text.contains("tab focus"));
        assert!(
            !text.contains("space fold"),
            "no space key in accordion-only mode"
        );
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
    fn focused_pane_title_is_cyan() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev_named("/wt/src/main.rs", 0, 1)]);
        app.focus = Focus::List;
        let buf = draw_app(&mut app, 80, 12);
        // The left title 'chronox · by file' starts at column 3 of the top row.
        assert_eq!(buf[(3u16, 1u16)].fg, Color::Cyan);
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
        assert!(
            text.contains("tweak the thing"),
            "active file's edit summary shown"
        );
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

    #[test]
    fn draw_does_not_panic_at_tiny_sizes() {
        // Regression: render_frame underflowed u16 subtractions at small bodies.
        for w in 1u16..=12 {
            for h in 1u16..=6 {
                let mut app = App::bare(PathBuf::from("/wt"));
                app.set_events_for_test_pub(vec![
                    ev_named("/wt/src/app.rs", 0, 1),
                    ev_named("/wt/src/ui.rs", 0, 2),
                ]);
                // Must not panic at any size.
                let _ = draw_app(&mut app, w, h);
            }
        }
    }

    #[test]
    fn list_width_clamped_to_frame_on_narrow_terminal() {
        // On a terminal narrower than MIN_LIST + 2, reclamp_split floors
        // list_width at MIN_LIST while the drawn divider is clamped inside the
        // frame. draw must reconcile them so list_width matches the rendered
        // divider (which input.rs uses for mouse hit-testing).
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev_named("/wt/src/app.rs", 0, 1)]);
        app.list_width = 30; // wider than the frame can hold
        let w = 12u16; // body.width == 12 for a full-width terminal at x=0
        let _ = draw_app(&mut app, w, 8);
        assert!(
            app.list_width <= w.saturating_sub(2),
            "divider stays inside the frame so it matches the mouse hit-test"
        );
    }
}
