//! Forward stdin unchanged and report OSC 7501 observations on stderr.

#[path = "terminal_stream/adapter.rs"]
mod adapter;
#[cfg(test)]
#[path = "terminal_stream/tests.rs"]
mod tests;

use std::io::{self, BufWriter, Read, Write};

use adapter::{Adapter, Kind, Segment};
use osc7501_parser::parse_sequence;

fn forward(
    segment: Segment<'_>,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> io::Result<()> {
    output.write_all(segment.bytes)?;
    match segment.kind {
        Kind::Osc if segment.bytes.starts_with(b"\x1b]7501;") => {
            match parse_sequence(segment.bytes) {
                Ok(Some(event)) => writeln!(diagnostics, "{event:?}")?,
                Ok(None) => writeln!(diagnostics, "ignored OSC 7501 report")?,
                Err(error) => writeln!(diagnostics, "rejected OSC 7501 report: {error}")?,
            }
        }
        Kind::Cancelled | Kind::Incomplete | Kind::Oversized
            if segment.bytes.starts_with(b"\x1b]7501;") =>
        {
            writeln!(diagnostics, "{:?} OSC 7501 prefix", segment.kind)?;
        }
        Kind::Reset => writeln!(diagnostics, "full terminal reset")?,
        _ => {}
    }
    Ok(())
}

fn run(
    input: &mut impl Read,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> io::Result<()> {
    let mut adapter = Adapter::default();
    let mut buffer = [0; 8192];
    loop {
        let length = match input.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if length == 0 {
            break;
        }
        adapter
            .try_feed(&buffer[..length], |segment| {
                forward(segment, output, diagnostics)
            })
            .map_err(adapter::Error::into_inner)?;
        output.flush()?;
    }
    adapter.finish(|segment| forward(segment, output, diagnostics))?;
    output.flush()
}

fn main() -> io::Result<()> {
    run(
        &mut io::stdin().lock(),
        &mut BufWriter::new(io::stdout().lock()),
        &mut io::stderr().lock(),
    )
}
