//! Status output shares the application's existing writer.
use osc7501::{AppId, Kind, Progress, State, Status};
use std::io::{self, IsTerminal, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stderr = io::stderr();
    if !stderr.is_terminal() {
        return Ok(());
    }
    let app = AppId::new("example").ok_or("invalid application id")?;
    let mut writer = stderr.lock();
    let output: &mut dyn Write = &mut writer;
    Status::new(State::Working)
        .app(app)
        .message("Rendering preview")
        .progress(Progress::new(40).ok_or("invalid progress")?)
        .write_to(output)?;
    Status::new(State::Blocked)
        .app(app)
        .kind(Kind::Permission)
        .message("Publish this preview?")
        .write_to(output)?;
    Status::new(State::Done)
        .app(app)
        .message("Preview ready")
        .write_to(output)?;
    output.flush()?;
    Ok(())
}
