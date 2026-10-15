//! Connect the release producer to the terminal host through encoded reports.

use super::{Error, Host, program::Program};
use osc7501::store::{Record, Slot};
use osc7501::{RecordId, State, write_query};
use ratatui::crossterm::event::KeyCode;
use std::collections::VecDeque;

pub(super) struct App<'a> {
    pub host: Host<'a>,
    pub program: Option<Program>,
    pub replies: Vec<u8>,
    pub log: VecDeque<(State, String)>,
    pub selected: usize,
    pub inspect_root: bool,
    pub inspect_api: bool,
    pub insight: &'static str,
    pub tasks: [(RecordId<'static>, &'static str); 3],
    diagnostics: Vec<u8>,
}

impl<'a> App<'a> {
    pub fn new(slots: &'a mut [Slot]) -> Result<Self, Error> {
        let program = Program::new()?;
        let tasks =
            std::array::from_fn(|index| (program.tasks[index].id, program.tasks[index].title));
        let mut bytes = Vec::new();
        write_query(&mut bytes)?;
        program.emit_root(&mut bytes)?;
        let mut app = Self {
            host: Host::new(slots)?,
            program: Some(program),
            replies: Vec::new(),
            log: VecDeque::new(),
            selected: 0,
            inspect_root: true,
            inspect_api: false,
            insight: "Start the same release with s. The host cards read the store after each input buffer is dropped.",
            tasks,
            diagnostics: Vec::new(),
        };
        app.receive(&bytes)?;
        app.record(State::Idle, "Support query answered; initial root retained");
        Ok(app)
    }

    fn receive(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.host
            .receive(bytes, &mut self.replies, &mut self.diagnostics)
    }

    fn record(&mut self, state: State, message: impl Into<String>) {
        if self.log.len() == 32 {
            self.log.pop_front();
        }
        self.log.push_back((state, message.into()));
    }

    pub fn selected_id(&self) -> Option<RecordId<'static>> {
        (!self.inspect_root).then_some(self.tasks[self.selected].0)
    }

    pub fn selected_record(&self) -> Option<Record<'_>> {
        self.host.records.get(self.selected_id())
    }

    pub fn command(&mut self, code: KeyCode) -> Result<(), Error> {
        if code == KeyCode::Char('s') && self.program.is_none() {
            self.program = Some(Program::new()?);
        }
        let Some(program) = &mut self.program else {
            return Ok(());
        };
        let mut bytes = b"ordinary application output\n".to_vec();
        if !program.key(code, self.selected, &mut bytes)? {
            return Ok(());
        }
        let state = program.state;
        let message = program.message;
        self.receive(&bytes)?;
        self.record(state, message);
        self.insight = match code {
            KeyCode::Char('s') => {
                "Three concurrent child records inherit osc7501-tui from the root. The release parent need not exist."
            }
            KeyCode::Char('p' | 'a' | '?') => {
                self.inspect_root = true;
                "The host reads the blocked state, reason, message, and progress from the stored root."
            }
            KeyCode::Enter => {
                "The new root report replaces every field. The old blocked kind disappears when work resumes."
            }
            KeyCode::Char('f') => {
                self.inspect_root = false;
                "The failed task is a stored result. Press r to replace it with a retry, or X to exit and retain the error."
            }
            KeyCode::Char('r') => {
                "Retry replaces the failed task's error message and state. Other task records stay independent."
            }
            _ => {
                "A clear report for release removes all three child records. The separate root is replaced with idle."
            }
        };
        Ok(())
    }

    pub fn tick(&mut self) -> Result<(), Error> {
        let Some(program) = &mut self.program else {
            return Ok(());
        };
        let previous = program.state;
        let mut bytes = Vec::new();
        program.tick(&mut bytes)?;
        let state = program.state;
        self.receive(&bytes)?;
        if previous != state && state == State::Done {
            self.record(
                State::Done,
                "Release completed; press X to drop the producer",
            );
            self.insight = "The store now holds four completed results. Press X: they survive after the producer is dropped.";
        }
        Ok(())
    }

    pub fn finish(&mut self, prompt: bool) {
        // The host must not depend on the producer's task model after exit.
        self.program = None;
        let mut removed = 0;
        if prompt {
            self.host.records.on_prompt(|_| removed += 1);
        } else {
            self.host.records.on_process_exit(|_| removed += 1);
        }
        self.record(
            State::Idle,
            format!(
                "{}: removed {removed} active records; {} retained",
                if prompt {
                    "Shell prompt"
                } else {
                    "Process exit"
                },
                self.host.records.len(),
            ),
        );
        self.insight = "Producer dropped. Active records removed; done and error results stay in the host. Press s for a new release.";
    }

    pub fn soft_reset(&mut self) {
        self.record(State::Idle, "Soft reset preserved every current record");
        self.insight = "A soft reset changes terminal settings. It needs no store action and preserves all current records.";
    }

    pub fn full_reset(&mut self) {
        self.host.records.on_full_reset(|_| {});
        self.host.decoder.reset();
        self.record(State::Idle, "Full reset cleared every current record");
        self.insight = "Full reset clears the store. It does not stop the program; later reports can create records again.";
    }
}
