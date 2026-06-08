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
    app.reclamp_split();

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
    let block = pane_block("timeline", app.focus == Focus::List);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = inner.height as usize;
    let len = app.events().len();
    let scroll = clamp_scroll(
        adjust_scroll(app.list_scroll, app.selected, rows, len),
        len,
        rows,
    );
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
