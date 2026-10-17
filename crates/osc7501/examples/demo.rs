//! Simulated release workflow with paced OSC 7501 updates.
use osc7501::{AppId, Context, Kind, Progress, RecordId, State, Status};
use std::{
    io::{self, IsTerminal, Write},
    thread,
    time::Duration,
};

fn reports() -> Result<[(&'static str, Status<'static>); 15], osc7501::Error> {
    let build = RecordId::new("build")?;

    let root = Context::new()
        .app(AppId::new("osc7501-demo")?)
        .title("Protocol demo");
    // Child records inherit the root's app, even without a build parent record.
    let tests = Context::new()
        .id(RecordId::new("build/tests")?)
        .title("Tests 🦀");
    let docs = Context::new()
        .id(RecordId::new("build/docs")?)
        .title("Documentation");
    Ok([
        (
            "idle: waiting for work",
            root.status(State::Idle).message("Waiting for work"),
        ),
        (
            "working: preparing a release (indeterminate progress)",
            root.status(State::Working).message("Preparing a release"),
        ),
        (
            "blocked/auth: simulated sign-in",
            root.status(State::Blocked)
                .message("Sign in to continue")
                .kind(Kind::Auth),
        ),
        (
            "working: signed in, starting the build (0%)",
            root.status(State::Working)
                .message("Starting the build")
                .progress(Progress::new(0)?),
        ),
        (
            "working build/tests: running tests (25%)",
            tests
                .status(State::Working)
                .message("Running tests")
                .progress(Progress::new(25)?),
        ),
        (
            "working build/docs: generating documentation",
            docs.status(State::Working)
                .message("Generating documentation"),
        ),
        (
            "done build/tests: tests passed",
            tests.status(State::Done).message("Tests passed"),
        ),
        (
            "error build/docs: a link check failed",
            docs.status(State::Error).message("A link check failed"),
        ),
        (
            "blocked/question: simulated answer",
            root.status(State::Blocked)
                .message("Which replacement link should I use?")
                .kind(Kind::Question),
        ),
        (
            "working build/docs: retrying with the new link (75%)",
            docs.status(State::Working)
                .message("Retrying with the new link")
                .progress(Progress::new(75)?),
        ),
        (
            "done build/docs: documentation ready",
            docs.status(State::Done).message("Documentation ready"),
        ),
        (
            "blocked/permission: simulated approval (100%)",
            root.status(State::Blocked)
                .message("Publish this preview?")
                .kind(Kind::Permission)
                .progress(Progress::new(100)?),
        ),
        (
            "working: publishing the preview (100%)",
            root.status(State::Working)
                .message("Publishing the preview")
                .progress(Progress::new(100)?),
        ),
        (
            "clear build: remove both child records",
            Status::clear().id(build),
        ),
        (
            "done: preview ready",
            root.status(State::Done).message("Preview ready"),
        ),
    ])
}

fn main() -> Result<(), osc7501::Error> {
    let stdout = io::stdout();
    let terminal = stdout.is_terminal();
    let mut output = stdout.lock();
    let reports = reports()?;
    writeln!(output, "OSC 7501 demo: all work and prompts are simulated.")?;
    for (label, status) in reports {
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
