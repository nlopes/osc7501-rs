//! Simulated release workflow with paced OSC 7501 updates.
use osc7501::{AppId, Kind, Progress, RecordId, State, Status};
use std::{
    io::{self, IsTerminal, Write},
    thread,
    time::Duration,
};

fn main() -> Result<(), osc7501::Error> {
    let stdout = io::stdout();
    let terminal = stdout.is_terminal();
    let mut output = stdout.lock();
    let app = AppId::new("osc7501-demo")?;
    let build = RecordId::new("build")?;
    let test_id = RecordId::new("build/tests")?;
    let docs_id = RecordId::new("build/docs")?;

    // Each update replaces its record, so repeat the app and title.
    let root = |state, message| {
        Status::new(state)
            .app(app)
            .title("Protocol demo")
            .message(message)
    };
    // Child records inherit the root's app, even without a build parent record.
    let tests = |state, message| {
        Status::new(state)
            .id(test_id)
            .title("Tests 🦀")
            .message(message)
    };
    let docs = |state, message| {
        Status::new(state)
            .id(docs_id)
            .title("Documentation")
            .message(message)
    };
    writeln!(output, "OSC 7501 demo: all work and prompts are simulated.")?;
    for (label, status) in [
        (
            "idle: waiting for work",
            root(State::Idle, "Waiting for work"),
        ),
        (
            "working: preparing a release (indeterminate progress)",
            root(State::Working, "Preparing a release"),
        ),
        (
            "blocked/auth: simulated sign-in",
            root(State::Blocked, "Sign in to continue").kind(Kind::Auth),
        ),
        (
            "working: signed in, starting the build (0%)",
            root(State::Working, "Starting the build").progress(Progress::new(0)?),
        ),
        (
            "working build/tests: running tests (25%)",
            tests(State::Working, "Running tests").progress(Progress::new(25)?),
        ),
        (
            "working build/docs: generating documentation",
            docs(State::Working, "Generating documentation"),
        ),
        (
            "done build/tests: tests passed",
            tests(State::Done, "Tests passed"),
        ),
        (
            "error build/docs: a link check failed",
            docs(State::Error, "A link check failed"),
        ),
        (
            "blocked/question: simulated answer",
            root(State::Blocked, "Which replacement link should I use?").kind(Kind::Question),
        ),
        (
            "working build/docs: retrying with the new link (75%)",
            docs(State::Working, "Retrying with the new link").progress(Progress::new(75)?),
        ),
        (
            "done build/docs: documentation ready",
            docs(State::Done, "Documentation ready"),
        ),
        (
            "blocked/permission: simulated approval (100%)",
            root(State::Blocked, "Publish this preview?")
                .kind(Kind::Permission)
                .progress(Progress::new(100)?),
        ),
        (
            "working: publishing the preview (100%)",
            root(State::Working, "Publishing the preview").progress(Progress::new(100)?),
        ),
        (
            "clear build: remove both child records",
            Status::clear().id(build),
        ),
        ("done: preview ready", root(State::Done, "Preview ready")),
    ] {
        if terminal {
            status.write_to(&mut output)?;
        }
        writeln!(output, "{label}")?;
        // Flush before pausing so the host can show each transition.
        output.flush()?;
        if terminal {
            thread::sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}
