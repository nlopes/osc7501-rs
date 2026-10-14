//! Interactive release simulation with OSC 7501 reports and a shared TUI writer.

#[path = "tui/ui.rs"]
mod ui;

use osc7501::{AppId, Kind, Progress, RecordId, State, Status};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        cursor::{Hide, Show},
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};
use std::{
    collections::VecDeque,
    error::Error,
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const TICK: Duration = Duration::from_millis(200);

struct Task {
    id: RecordId<'static>,
    title: &'static str,
    state: State,
    progress: u8,
    step: u8,
}

impl Task {
    fn status(&self) -> Result<Status<'_>> {
        let status = Status::new(self.state)
            .id(self.id)
            .title(self.title)
            .message(self.message());
        if self.state == State::Working {
            Ok(status.progress(Progress::new(self.progress)?))
        } else {
            Ok(status)
        }
    }

    fn message(&self) -> &'static str {
        match self.state {
            State::Working => "Running simulated work",
            State::Done => "Task completed",
            State::Error => "Simulated failure; select this task and press r",
            _ => "Waiting to start",
        }
    }
}

struct App {
    id: AppId<'static>,
    subtree: RecordId<'static>,
    state: State,
    kind: Option<Kind>,
    message: &'static str,
    tasks: [Task; 3],
    selected: usize,
    inspect_root: bool,
    log: VecDeque<(State, String)>,
    reports: usize,
}

impl App {
    fn new() -> Result<Self> {
        let task = |id, title, step| -> Result<Task> {
            Ok(Task {
                id: RecordId::new(id)?,
                title,
                state: State::Idle,
                progress: 0,
                step,
            })
        };
        Ok(Self {
            id: AppId::new("osc7501-tui")?,
            subtree: RecordId::new("release")?,
            state: State::Idle,
            kind: None,
            message: "Press s to start a simulated release",
            tasks: [
                task("release/tests", "Test suite", 4)?,
                task("release/docs", "Documentation", 3)?,
                task("release/package", "Package", 5)?,
            ],
            selected: 0,
            inspect_root: true,
            log: VecDeque::new(),
            reports: 0,
        })
    }

    fn progress(&self) -> u8 {
        let total: u16 = self.tasks.iter().map(|task| u16::from(task.progress)).sum();
        u8::try_from(total / 3).unwrap_or(100)
    }

    fn emit_root(&mut self, output: &mut impl Write) -> Result {
        // Reports replace every field, so repeat the root's app and title.
        let mut status = Status::new(self.state)
            .app(self.id)
            .title("Release playground")
            .message(self.message);
        if let Some(kind) = self.kind {
            status = status.kind(kind);
        }
        if matches!(self.state, State::Working | State::Blocked) {
            status = status.progress(Progress::new(self.progress())?);
        }
        status.write_to(output)?;
        self.record(self.state, format!("root  {}", self.message));
        Ok(())
    }

    fn emit_task(&mut self, index: usize, output: &mut impl Write) -> Result {
        let task = &self.tasks[index];
        // Child records inherit the app from the root.
        task.status()?.write_to(output)?;
        let label = format!(
            "{}  {}  {}%",
            task.id.as_str(),
            task.state.as_str(),
            task.progress
        );
        self.record(task.state, label);
        Ok(())
    }

    fn record(&mut self, state: State, label: String) {
        self.reports += 1;
        if self.log.len() == 32 {
            self.log.pop_front();
        }
        self.log.push_back((state, label));
    }

    fn clear_tasks(&mut self, output: &mut impl Write) -> Result {
        Status::clear().id(self.subtree).write_to(output)?;
        self.record(
            State::Idle,
            "clear release  removed all child records".into(),
        );
        for task in &mut self.tasks {
            task.state = State::Idle;
            task.progress = 0;
        }
        Ok(())
    }

    fn start(&mut self, output: &mut impl Write) -> Result {
        self.clear_tasks(output)?;
        self.state = State::Working;
        self.kind = None;
        self.message = "Preparing a release";
        self.emit_root(output)?;
        for index in 0..self.tasks.len() {
            self.tasks[index].state = State::Working;
            self.emit_task(index, output)?;
        }
        Ok(())
    }

    fn cancel(&mut self, output: &mut impl Write) -> Result {
        self.clear_tasks(output)?;
        self.state = State::Idle;
        self.kind = None;
        self.message = "Release cancelled; press s to start again";
        self.emit_root(output)
    }

    fn tick(&mut self, output: &mut impl Write) -> Result {
        if self.state != State::Working {
            return Ok(());
        }
        for index in 0..self.tasks.len() {
            let task = &mut self.tasks[index];
            if task.state == State::Working {
                task.progress = task.progress.saturating_add(task.step).min(100);
                if task.progress == 100 {
                    task.state = State::Done;
                }
                self.emit_task(index, output)?;
            }
        }
        if self.tasks.iter().all(|task| task.state == State::Done) {
            self.state = State::Done;
            self.message = "Release ready; press s to run it again";
        }
        self.emit_root(output)
    }

    fn key(&mut self, key: KeyEvent, output: &mut impl Write) -> Result<bool> {
        if key.code == KeyCode::Char('q')
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            // Leave a completed result visible to the host after exit.
            if matches!(self.state, State::Working | State::Blocked) {
                self.cancel(output)?;
            }
            return Ok(true);
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = (self.selected + self.tasks.len() - 1) % self.tasks.len();
                self.inspect_root = false;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected = (self.selected + 1) % self.tasks.len();
                self.inspect_root = false;
            }
            KeyCode::Char('b') => self.inspect_root = !self.inspect_root,
            KeyCode::Char('s') => self.start(output)?,
            KeyCode::Char('c') | KeyCode::Esc => self.cancel(output)?,
            KeyCode::Char('p' | 'a' | '?') if self.state == State::Working => {
                let (kind, message) = match key.code {
                    KeyCode::Char('a') => {
                        (Kind::Auth, "Simulated sign-in: press Enter to continue")
                    }
                    KeyCode::Char('?') => (
                        Kind::Question,
                        "Use the default release target? Press Enter",
                    ),
                    _ => (
                        Kind::Permission,
                        "Approve this release? Press Enter to continue",
                    ),
                };
                self.state = State::Blocked;
                self.kind = Some(kind);
                self.message = message;
                self.inspect_root = true;
                self.emit_root(output)?;
            }
            KeyCode::Enter if self.state == State::Blocked => {
                self.state = State::Working;
                self.kind = None;
                self.message = "Resuming the release";
                self.emit_root(output)?;
            }
            KeyCode::Char('f')
                if self.state == State::Working
                    && self.tasks[self.selected].state == State::Working =>
            {
                self.tasks[self.selected].state = State::Error;
                self.emit_task(self.selected, output)?;
                self.state = State::Error;
                self.message = "A task failed; select the red task and press r";
                self.emit_root(output)?;
            }
            KeyCode::Char('r') if self.tasks[self.selected].state == State::Error => {
                self.tasks[self.selected].state = State::Working;
                self.tasks[self.selected].progress = 0;
                self.emit_task(self.selected, output)?;
                self.state = State::Working;
                self.kind = None;
                self.message = "Retrying the failed task";
                self.emit_root(output)?;
            }
            _ => {}
        }
        Ok(false)
    }
}

struct TerminalModes;

impl Drop for TerminalModes {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

fn main() -> Result {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "Run this example in an interactive terminal. Use --example demo for a text trace."
                .into(),
        );
    }
    let mut app = App::new()?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    enable_raw_mode()?;
    let _modes = TerminalModes;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    execute!(output, EnterAlternateScreen, Hide)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(output))?;
    let result = run(&mut terminal, &mut app);
    if result.is_err() {
        app.state = State::Error;
        app.kind = None;
        app.message = "The TUI stopped after an I/O or report error";
        let _ = app.emit_root(terminal.backend_mut());
        let _ = terminal.backend_mut().flush();
    }
    result
}

fn run<W: Write>(terminal: &mut Terminal<CrosstermBackend<W>>, app: &mut App) -> Result {
    app.emit_root(terminal.backend_mut())?;
    let mut last_tick = Instant::now();
    loop {
        // Drawing and reports share one writer and one event loop.
        terminal.backend_mut().flush()?;
        terminal.draw(|frame| ui::draw(frame, app))?;
        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && app.key(key, terminal.backend_mut())?
        {
            terminal.backend_mut().flush()?;
            return Ok(());
        }
        if last_tick.elapsed() >= TICK {
            app.tick(terminal.backend_mut())?;
            last_tick = Instant::now();
        }
    }
}
