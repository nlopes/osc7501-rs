//! The release playground with a terminal host that retains the producer reports.
#[path = "host/program.rs"]
mod program;
#[path = "host/scenario.rs"]
mod scenario;
#[path = "host/ui.rs"]
mod ui;

#[cfg(test)]
use osc7501::Status;
use osc7501::store::{MIN_HOST_CAPACITY, Slot, Store};
use osc7501::{
    AppId, CapabilityReply, Kind, RecordId, State,
    parser::{Decoder, Event},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        cursor::{Hide, Show},
        event::{self, Event as TerminalEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};
use scenario::App;
use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Producer(#[from] osc7501::Error),
    #[error(transparent)]
    Parser(#[from] osc7501::parser::Error),
    #[error(transparent)]
    Store(#[from] osc7501::store::Error),
}

struct Host<'a> {
    decoder: Decoder,
    records: Store<'a>,
    reports_received: usize,
}

impl<'a> Host<'a> {
    fn new(slots: &'a mut [Slot]) -> Result<Self, Error> {
        Ok(Self {
            decoder: Decoder::new(),
            records: Store::new(slots)?,
            reports_received: 0,
        })
    }

    fn receive(
        &mut self,
        bytes: &[u8],
        program_input: &mut dyn Write,
        trace: &mut dyn Write,
    ) -> Result<(), Error> {
        // Seven-byte chunks simulate fragmented reads from one terminal's PTY.
        for chunk in bytes.chunks(7) {
            let mut result = Ok(());
            self.decoder.feed(chunk, |event| {
                // The callback cannot return an error; propagate it after feed.
                if result.is_err() {
                    return;
                }
                result = match event {
                    Ok(Event::Report(report)) => self
                        .records
                        .apply(report, |_| {})
                        .map(|()| {
                            self.reports_received += 1;
                        })
                        .map_err(Error::from),
                    Ok(Event::Query) => CapabilityReply::new()
                        .states(&[
                            State::Idle,
                            State::Working,
                            State::Done,
                            State::Blocked,
                            State::Error,
                        ])
                        .kinds(&[Kind::Permission, Kind::Question, Kind::Auth])
                        .write_to(program_input)
                        .map_err(Error::from),
                    Ok(_) => Ok(()),
                    Err(error) => {
                        writeln!(trace, "  Rejected report: {error}").map_err(Error::from)
                    }
                };
            });
            result?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn reports(
        &mut self,
        reports: &[Status<'_>],
        program_input: &mut dyn Write,
        trace: &mut dyn Write,
    ) -> Result<(), Error> {
        let mut bytes = Vec::new();
        for report in reports {
            bytes.extend_from_slice(b"ordinary application output\n");
            report.write_to(&mut bytes)?;
        }
        self.receive(&bytes, program_input, trace)
    }

    fn show(&self, label: &str, trace: &mut dyn Write) -> io::Result<()> {
        writeln!(trace, "\n{label} ({} records)", self.records.len())?;
        if self.records.is_empty() {
            writeln!(trace, "  No current records.")?;
        }
        for record in self.records.iter() {
            let app = self.records.effective_app(record.id());
            write!(
                trace,
                "  {}: {} app={}",
                record.id().map_or("<root>", RecordId::as_str),
                record.state().as_str(),
                app.map_or("-", AppId::as_str),
            )?;
            if app.is_some() && record.app().is_none() {
                write!(trace, " (inherited)")?;
            }
            if let Some(progress) = record.progress() {
                write!(trace, " progress={}%", progress.get())?;
            }
            if let Some(kind) = record.kind() {
                write!(trace, " kind={}", kind.as_str())?;
            }
            if let Some(title) = record.title() {
                write!(trace, " title={title:?}")?;
            }
            if let Some(message) = record.message() {
                write!(trace, " message={message:?}")?;
            }
            writeln!(trace)?;
        }
        Ok(())
    }
}

fn run(trace: &mut dyn Write) -> Result<(), Error> {
    let mut slots: Vec<_> = (0..MIN_HOST_CAPACITY).map(|_| Slot::EMPTY).collect();
    let mut app = App::new(&mut slots)?;
    writeln!(trace, "OSC 7501 / release playground / terminal host")?;
    writeln!(
        trace,
        "Capability reply: {:?}",
        String::from_utf8_lossy(&app.replies)
    )?;
    app.host
        .show("Ready: the same three release tasks", trace)?;
    app.command(KeyCode::Char('s'))?;
    for _ in 0..20 {
        app.tick()?;
    }
    app.host
        .show("Package done; tests and documentation still running", trace)?;
    app.command(KeyCode::Char('p'))?;
    app.host
        .show("Permission prompt: stored root is blocked", trace)?;
    app.command(KeyCode::Enter)?;
    app.selected = 1;
    app.command(KeyCode::Char('f'))?;
    app.host
        .show("Documentation failed; other task records remain", trace)?;
    app.finish(false);
    app.host.show(
        "Producer dropped: completed and failed results survive",
        trace,
    )?;
    app.command(KeyCode::Char('s'))?;
    app.command(KeyCode::Char('f'))?;
    app.command(KeyCode::Char('r'))?;
    app.host
        .show("Restart and retry: error fields replaced", trace)?;
    app.command(KeyCode::Char('c'))?;
    app.host
        .show("Cancel: clear release removed all three children", trace)?;
    app.command(KeyCode::Char('s'))?;
    for _ in 0..34 {
        app.tick()?;
    }
    app.finish(true);
    app.host
        .show("Shell prompt: four completed results remain", trace)?;
    app.soft_reset();
    app.host.show("Soft reset: all results preserved", trace)?;
    app.full_reset();
    app.host.show("Full reset: all records cleared", trace)?;
    Ok(())
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

fn interactive() -> Result<(), Error> {
    let mut slots: Vec<_> = (0..MIN_HOST_CAPACITY).map(|_| Slot::EMPTY).collect();
    let mut app = App::new(&mut slots)?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    enable_raw_mode()?;
    let _modes = TerminalModes;
    let mut output = io::stdout().lock();
    execute!(output, EnterAlternateScreen, Hide)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(output))?;
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        if event::poll(Duration::from_millis(200).saturating_sub(last_tick.elapsed()))?
            && let TerminalEvent::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && keypress(&mut app, key)?
        {
            return Ok(());
        }
        if last_tick.elapsed() >= Duration::from_millis(200) {
            app.tick()?;
            last_tick = Instant::now();
        }
    }
}

fn keypress(app: &mut App<'_>, key: KeyEvent) -> Result<bool, Error> {
    if key.code == KeyCode::Char('q')
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Ok(true);
    }
    match key.code {
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
            app.selected = (app.selected + 1) % app.tasks.len();
            app.inspect_root = false;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.selected = (app.selected + app.tasks.len() - 1) % app.tasks.len();
            app.inspect_root = false;
        }
        KeyCode::Char('b') => app.inspect_root = !app.inspect_root,
        KeyCode::Char('i') => app.inspect_api = !app.inspect_api,
        KeyCode::Char('P') => app.finish(true),
        KeyCode::Char('X') => app.finish(false),
        KeyCode::Char('S') => app.soft_reset(),
        KeyCode::Char('R') => app.full_reset(),
        code => app.command(code)?,
    }
    Ok(false)
}

fn main() -> Result<(), Error> {
    if !io::stdin().is_terminal()
        || !io::stdout().is_terminal()
        || std::env::args().any(|arg| arg == "--plain")
    {
        let mut output = io::stdout().lock();
        run(&mut output)?;
        output.flush()?;
        Ok(())
    } else {
        interactive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_replies_use_only_the_program_input_writer() -> Result<(), Error> {
        let mut slots = [Slot::EMPTY];
        let mut host = Host::new(&mut slots)?;
        let mut replies = Vec::new();
        let mut trace = Vec::new();
        host.receive(b"ordinary text\x1b]7501;?\x1b\\", &mut replies, &mut trace)?;
        assert_eq!(replies, b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\");
        assert_eq!(trace, b"");
        assert!(host.records.is_empty());
        let expected_reply = replies.clone();
        host.reports(&[Status::new(State::Done)], &mut replies, &mut trace)?;
        assert_eq!(replies, expected_reply);
        Ok(())
    }

    #[test]
    fn records_outlive_reads_and_invalid_updates_preserve_them() -> Result<(), Error> {
        let mut slots = [const { Slot::EMPTY }; 3];
        let mut host = Host::new(&mut slots)?;
        let mut replies = Vec::new();
        let mut trace = Vec::new();
        let id = RecordId::new("build/docs")?;
        {
            let mut bytes = Vec::new();
            Status::new(State::Idle)
                .app(AppId::new("release")?)
                .write_to(&mut bytes)?;
            Status::new(State::Done)
                .id(id)
                .message("Docs built 🦀")
                .write_to(&mut bytes)?;
            for chunk in bytes.chunks(1) {
                host.receive(chunk, &mut replies, &mut trace)?;
            }
        }
        host.receive(
            b"\x1b]7501;state=paused:id=build/docs\x1b\\\
              \x1b]7501;state=error:id=build/docs:msg=AA==\x1b\\",
            &mut replies,
            &mut trace,
        )?;
        host.reports(
            &[Status::new(State::Working).id(RecordId::new("watch")?)],
            &mut replies,
            &mut trace,
        )?;
        host.records.on_prompt(|_| {});
        let record = host
            .records
            .get(Some(id))
            .ok_or(io::Error::other("missing result"))?;
        assert_eq!(record.state(), State::Done);
        assert_eq!(record.message(), Some("Docs built 🦀"));
        assert_eq!(record.app(), None);
        assert_eq!(
            host.records.effective_app(Some(id)),
            Some(AppId::new("release")?)
        );
        assert_eq!(host.records.len(), 2);
        assert!(trace.starts_with(b"  Rejected report: "));
        assert_eq!(String::from_utf8_lossy(&trace).lines().count(), 1);
        assert_eq!(replies, b"");
        Ok(())
    }

    #[test]
    fn callback_writer_failures_reach_the_caller() -> Result<(), Error> {
        let mut slots = [Slot::EMPTY];
        let mut host = Host::new(&mut slots)?;
        let mut no_space = &mut [][..];
        let mut trace = Vec::new();
        assert!(matches!(
            host.receive(b"\x1b]7501;?\x1b\\", &mut no_space, &mut trace),
            Err(Error::Producer(osc7501::Error::Io(_)))
        ));
        assert!(matches!(
            host.receive(
                b"\x1b]7501;state=done:msg=AA==\x1b\\",
                &mut Vec::new(),
                &mut no_space
            ),
            Err(Error::Io(_))
        ));
        Ok(())
    }

    #[test]
    fn complete_demo_writes_plain_text() -> Result<(), Error> {
        let mut trace = Vec::new();
        run(&mut trace)?;
        assert!(!trace.contains(&0x1b));
        Ok(())
    }

    #[test]
    fn release_controls_apply_replacement_inheritance_and_subtree_clear() -> Result<(), Error> {
        let mut slots: Vec<_> = (0..MIN_HOST_CAPACITY).map(|_| Slot::EMPTY).collect();
        let mut app = App::new(&mut slots)?;
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        keypress(&mut app, key(KeyCode::Char('s')))?;
        app.tick()?;
        assert_eq!(app.host.records.len(), 4);
        let docs = Some(RecordId::new("release/docs")?);
        assert_eq!(
            app.host.records.effective_app(docs),
            Some(AppId::new("osc7501-tui")?)
        );
        keypress(&mut app, key(KeyCode::Char('p')))?;
        assert_eq!(
            app.host
                .records
                .get(None)
                .and_then(osc7501::store::Record::kind),
            Some(Kind::Permission)
        );
        keypress(&mut app, key(KeyCode::Enter))?;
        assert_eq!(
            app.host
                .records
                .get(None)
                .and_then(osc7501::store::Record::kind),
            None
        );
        keypress(&mut app, key(KeyCode::Down))?;
        assert_eq!(app.selected_id(), docs);
        keypress(&mut app, key(KeyCode::Char('f')))?;
        assert_eq!(
            app.selected_record().map(osc7501::store::Record::state),
            Some(State::Error)
        );
        keypress(&mut app, key(KeyCode::Char('r')))?;
        assert_eq!(
            app.selected_record()
                .and_then(osc7501::store::Record::message),
            Some("Running simulated work")
        );
        assert_eq!(
            app.selected_record()
                .and_then(osc7501::store::Record::progress)
                .map(osc7501::Progress::get),
            Some(0)
        );
        keypress(&mut app, key(KeyCode::Esc))?;
        assert_eq!(app.host.records.len(), 1);
        assert_eq!(
            app.host
                .records
                .get(None)
                .map(osc7501::store::Record::state),
            Some(State::Idle)
        );
        assert!(keypress(&mut app, key(KeyCode::Char('q')))?);
        Ok(())
    }

    #[test]
    fn results_survive_after_the_producer_is_dropped() -> Result<(), Error> {
        for prompt in [false, true] {
            let mut slots: Vec<_> = (0..MIN_HOST_CAPACITY).map(|_| Slot::EMPTY).collect();
            let mut app = App::new(&mut slots)?;
            app.command(KeyCode::Char('s'))?;
            for _ in 0..20 {
                app.tick()?;
            }
            app.selected = 1;
            app.command(KeyCode::Char('f'))?;
            app.finish(prompt);
            assert!(app.program.is_none());
            assert_eq!(app.host.records.len(), 3);
            assert_eq!(
                app.host
                    .records
                    .get(Some(RecordId::new("release/package")?))
                    .map(osc7501::store::Record::state),
                Some(State::Done)
            );
            assert_eq!(
                app.host
                    .records
                    .get(Some(RecordId::new("release/docs")?))
                    .map(osc7501::store::Record::state),
                Some(State::Error)
            );
            assert!(
                app.host
                    .records
                    .get(Some(RecordId::new("release/tests")?))
                    .is_none()
            );
            let reports = app.host.reports_received;
            app.tick()?;
            assert_eq!(app.host.reports_received, reports);
            app.soft_reset();
            assert_eq!(app.host.records.len(), 3);
            app.full_reset();
            assert!(app.host.records.is_empty());
            app.command(KeyCode::Char('s'))?;
            assert_eq!(app.host.records.len(), 4);
        }
        Ok(())
    }

    #[test]
    fn release_and_host_actions_render_at_compact_and_wide_sizes() -> Result<(), Error> {
        use ratatui::backend::TestBackend;
        let mut slots: Vec<_> = (0..MIN_HOST_CAPACITY).map(|_| Slot::EMPTY).collect();
        let mut app = App::new(&mut slots)?;
        for action in [
            None,
            Some('s'),
            Some('p'),
            Some('f'),
            Some('r'),
            Some('c'),
            Some('X'),
            Some('R'),
        ] {
            if action == Some('f') {
                app.command(KeyCode::Enter)?;
            }
            if let Some(code) = action {
                keypress(
                    &mut app,
                    KeyEvent::new(KeyCode::Char(code), KeyModifiers::NONE),
                )?;
            }
            for (width, height) in [(80, 24), (120, 38)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height))
                    .map_err(|never| -> Error { match never {} })?;
                for inspect_api in [false, true] {
                    app.inspect_api = inspect_api;
                    terminal
                        .draw(|frame| ui::draw(frame, &app))
                        .map_err(|never| -> Error { match never {} })?;
                    let text: String = terminal
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .map(ratatui::buffer::Cell::symbol)
                        .collect();
                    assert!(text.contains("release playground"));
                    assert!(text.contains("osc7501-store"));
                    assert!(text.contains("q quit"));
                    assert!(text.contains("X exit"));
                }
            }
        }
        let mut terminal =
            Terminal::new(TestBackend::new(50, 15)).map_err(|never| -> Error { match never {} })?;
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .map_err(|never| -> Error { match never {} })?;
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(text.contains("Resize to at least"));
        Ok(())
    }
}
