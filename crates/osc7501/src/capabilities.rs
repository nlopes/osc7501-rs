use std::io::{self, Write};

use crate::{Error, Kind, State, parser};

/// A terminal host's capability reply, with borrowed state and kind lists.
///
/// Advertise only capabilities the host implements. Omitted lists use the
/// fixed revision 0.4 defaults; explicit empty lists advertise no support.
/// A default reply has the same bytes as a query. On terminal-to-program input,
/// interpret [`parser::Event::Query`] using the caller's pending-query state.
#[derive(Clone, Copy, Debug, Default)]
pub struct CapabilityReply<'a> {
    states: Option<&'a [State]>,
    kinds: Option<&'a [Kind]>,
}

impl<'a> CapabilityReply<'a> {
    /// Uses the fixed revision 0.4 defaults for both omitted lists.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            states: None,
            kinds: None,
        }
    }

    /// Advertises these states in order; an empty slice advertises none.
    #[must_use]
    pub const fn states(mut self, states: &'a [State]) -> Self {
        self.states = Some(states);
        self
    }

    /// Advertises these blocking reasons in order; an empty slice advertises none.
    #[must_use]
    pub const fn kinds(mut self, kinds: &'a [Kind]) -> Self {
        self.kinds = Some(kinds);
        self
    }

    /// Encodes the complete reply before writing to the caller's writer.
    /// Uses no heap allocation and does not flush.
    ///
    /// # Errors
    /// Returns [`parser::Error::Limit`] wrapped in [`Error::Parser`] before any
    /// output if the reply exceeds the complete-frame cap. Writer failures
    /// return [`Error::Io`] and can leave a partial frame.
    pub fn write_to<W: Write + ?Sized>(self, output: &mut W) -> Result<(), Error> {
        let mut frame = [0; parser::MAX_SEQUENCE];
        let length = self
            .encode(&mut frame)
            .map_err(|_| parser::Error::Limit(parser::Limit::Sequence))?;
        output.write_all(&frame[..length])?;
        Ok(())
    }

    fn encode(self, frame: &mut [u8]) -> io::Result<usize> {
        let capacity = frame.len();
        let mut remaining = frame;
        remaining.write_all(b"\x1b]7501;?")?;
        if let Some(states) = self.states {
            write_list(
                &mut remaining,
                b":states=",
                states.iter().copied().map(State::as_str),
            )?;
        }
        if let Some(kinds) = self.kinds {
            write_list(
                &mut remaining,
                b":kinds=",
                kinds.iter().copied().map(Kind::as_str),
            )?;
        }
        remaining.write_all(b"\x1b\\")?;
        Ok(capacity - remaining.len())
    }
}

fn write_list(
    output: &mut impl Write,
    key: &[u8],
    values: impl Iterator<Item = &'static str>,
) -> io::Result<()> {
    output.write_all(key)?;
    for (index, value) in values.enumerate() {
        if index != 0 {
            output.write_all(b",")?;
        }
        output.write_all(value.as_bytes())?;
    }
    Ok(())
}
