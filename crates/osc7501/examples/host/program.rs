//! The producer TUI's simulated release, writing reports to a supplied stream.

use super::Error;
use osc7501::{AppId, Kind, Progress, RecordId, State, Status};
use ratatui::crossterm::event::KeyCode;
use std::io::Write;

pub(super) struct Task {
    pub id: RecordId<'static>,
    pub title: &'static str,
    state: State,
    progress: u8,
    step: u8,
}

impl Task {
    fn status(&self) -> Result<Status<'_>, Error> {
        let message = match self.state {
            State::Working => "Running simulated work",
            State::Done => "Task completed",
            State::Error => "Simulated failure; select this task and press r",
            _ => "Waiting to start",
        };
        let status = Status::new(self.state)
            .id(self.id)
            .title(self.title)
            .message(message);
        if self.state == State::Working {
            Ok(status.progress(Progress::new(self.progress)?))
        } else {
            Ok(status)
        }
    }
}

pub(super) struct Program {
    pub state: State,
    pub message: &'static str,
    pub tasks: [Task; 3],
    kind: Option<Kind>,
}

impl Program {
    pub fn new() -> Result<Self, Error> {
        let task = |id, title, step| -> Result<Task, Error> {
            Ok(Task {
                id: RecordId::new(id)?,
                title,
                state: State::Idle,
                progress: 0,
                step,
            })
        };
        Ok(Self {
            state: State::Idle,
            message: "Press s to start a simulated release",
            tasks: [
                task("release/tests", "Test suite", 4)?,
                task("release/docs", "Documentation", 3)?,
                task("release/package", "Package", 5)?,
            ],
            kind: None,
        })
    }

    pub fn emit_root(&self, output: &mut impl Write) -> Result<(), Error> {
        // Complete replacement requires the root's app and title on every report.
        let mut status = Status::new(self.state)
            .app(AppId::new("osc7501-tui")?)
            .title("Release playground")
            .message(self.message);
        if let Some(kind) = self.kind {
            status = status.kind(kind);
        }
        if matches!(self.state, State::Working | State::Blocked) {
            let total: u16 = self.tasks.iter().map(|task| u16::from(task.progress)).sum();
            status = status.progress(Progress::new(u8::try_from(total / 3).unwrap_or(100))?);
        }
        status.write_to(output)?;
        Ok(())
    }

    fn clear_tasks(&mut self, output: &mut impl Write) -> Result<(), Error> {
        Status::clear()
            .id(RecordId::new("release")?)
            .write_to(output)?;
        for task in &mut self.tasks {
            task.state = State::Idle;
            task.progress = 0;
        }
        Ok(())
    }

    pub fn tick(&mut self, output: &mut impl Write) -> Result<(), Error> {
        if self.state != State::Working {
            return Ok(());
        }
        for task in &mut self.tasks {
            if task.state == State::Working {
                task.progress = task.progress.saturating_add(task.step).min(100);
                if task.progress == 100 {
                    task.state = State::Done;
                }
                task.status()?.write_to(output)?;
            }
        }
        if self.tasks.iter().all(|task| task.state == State::Done) {
            self.state = State::Done;
            self.message = "Release ready; press s to run it again";
        }
        self.emit_root(output)
    }

    pub fn key(
        &mut self,
        key: KeyCode,
        selected: usize,
        output: &mut impl Write,
    ) -> Result<bool, Error> {
        match key {
            KeyCode::Char('s') => {
                self.clear_tasks(output)?;
                self.state = State::Working;
                self.kind = None;
                self.message = "Preparing a release";
                self.emit_root(output)?;
                for task in &mut self.tasks {
                    task.state = State::Working;
                    task.status()?.write_to(output)?;
                }
            }
            KeyCode::Char('c') | KeyCode::Esc => {
                self.clear_tasks(output)?;
                self.state = State::Idle;
                self.kind = None;
                self.message = "Release cancelled; press s to start again";
                self.emit_root(output)?;
            }
            KeyCode::Char('p' | 'a' | '?') if self.state == State::Working => {
                let (kind, message) = match key {
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
                self.emit_root(output)?;
            }
            KeyCode::Enter if self.state == State::Blocked => {
                self.state = State::Working;
                self.kind = None;
                self.message = "Resuming the release";
                self.emit_root(output)?;
            }
            KeyCode::Char('f')
                if self.state == State::Working && self.tasks[selected].state == State::Working =>
            {
                self.tasks[selected].state = State::Error;
                self.tasks[selected].status()?.write_to(output)?;
                self.state = State::Error;
                self.message = "A task failed; select the red task and press r";
                self.emit_root(output)?;
            }
            KeyCode::Char('r') if self.tasks[selected].state == State::Error => {
                self.tasks[selected].state = State::Working;
                self.tasks[selected].progress = 0;
                self.tasks[selected].status()?.write_to(output)?;
                self.state = State::Working;
                self.kind = None;
                self.message = "Retrying the failed task";
                self.emit_root(output)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
