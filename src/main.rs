mod app;
mod input;
mod ui;

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
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
