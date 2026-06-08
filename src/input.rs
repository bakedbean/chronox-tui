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
    let divider_col = app.last_area.x.saturating_add(app.list_width);
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
        assert_eq!(
            map(key(KeyCode::Char('j')), &app),
            AppAction::Nav(NavKey::Down)
        );
        assert_eq!(map(key(KeyCode::Up), &app), AppAction::Nav(NavKey::Up));
        assert_eq!(map(key(KeyCode::Tab), &app), AppAction::ToggleFocus);
        assert_eq!(
            map(key(KeyCode::Char('[')), &app),
            AppAction::NudgeSplit(-1)
        );
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
        assert_eq!(
            map(mouse(MouseEventKind::ScrollDown, 60), &app),
            AppAction::ScrollDiff(3)
        );
        assert_eq!(
            map(mouse(MouseEventKind::ScrollUp, 60), &app),
            AppAction::ScrollDiff(-3)
        );
        assert_eq!(
            map(mouse(MouseEventKind::ScrollDown, 10), &app),
            AppAction::None
        );
    }
}
