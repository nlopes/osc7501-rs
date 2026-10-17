//! Original terminal bytes with VT-aware OSC boundaries.

use core::fmt;
use osc7501_parser::MAX_SEQUENCE;

/// Why a segment was delivered. Every segment retains its original bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// Output outside a retained OSC frame.
    Bytes,
    /// A complete OSC frame, including its introducer and BEL or ST.
    Osc,
    /// A cancelled OSC prefix. The cancelling bytes follow separately.
    Cancelled,
    /// A pending prefix at an explicit end-of-stream boundary.
    Incomplete,
    /// An over-limit OSC prefix. Its remaining bytes pass through normally.
    Oversized,
    /// Bytes through a full reset (RIS). Its ESC may have been delivered earlier.
    Reset,
}

/// A borrowed output segment, valid only during its callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Segment<'a> {
    /// The framing outcome or terminal action associated with these bytes.
    pub kind: Kind,
    /// Original bytes, including any framing controls.
    pub bytes: &'a [u8],
}

/// A consumer failure and the number of bytes consumed from this feed call.
#[derive(Debug)]
pub struct Error<E> {
    consumed: usize,
    source: E,
}

impl<E> Error<E> {
    /// Resume with the suffix at this offset on the same adapter.
    /// Zero is possible when a segment retained from an earlier call is delivered.
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// Borrows the original consumer error or stop value.
    pub const fn get_ref(&self) -> &E {
        &self.source
    }

    /// Returns the original consumer error or stop value.
    pub fn into_inner(self) -> E {
        self.source
    }
}

impl<E> fmt::Display for Error<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "terminal consumer stopped after {} input bytes",
            self.consumed()
        )
    }
}

impl<E: core::error::Error + 'static> core::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(self.get_ref())
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Bytes,
    Escape,
    Osc,
    Terminator { dispatched: bool },
}

#[derive(Default)]
struct Dispatch {
    osc: bool,
    reset: bool,
}

impl vte::Perform for Dispatch {
    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {
        self.osc = true;
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.reset = !ignore && intermediates.is_empty() && byte == b'c';
    }

    fn terminated(&self) -> bool {
        self.reset
    }
}

/// Bounded OSC observation and lossless forwarding using vte's terminal context.
///
/// Retains at most one OSC prefix plus a pending ESC. No method allocates.
/// Oversized, cancelled, and unfinished frames remain output, not parsed reports.
pub struct Adapter {
    parser: vte::Parser<MAX_SEQUENCE>,
    buffer: [u8; MAX_SEQUENCE + 1],
    length: usize,
    phase: Phase,
}

impl Default for Adapter {
    fn default() -> Self {
        Self {
            parser: vte::Parser::default(),
            buffer: [0; MAX_SEQUENCE + 1],
            length: 0,
            phase: Phase::Bytes,
        }
    }
}

impl Adapter {
    /// Delivers original bytes in order, retaining only possible OSC frames.
    ///
    /// A delivered segment stays consumed even when the callback returns Err.
    /// The caller must retain any segment it needs to retry. On error, resume at
    /// `error.consumed()`; that offset can be zero when flushing an earlier prefix.
    /// Call `finish` at EOF or between independent terminal streams.
    pub fn try_feed<E>(
        &mut self,
        input: &[u8],
        mut emit: impl FnMut(Segment<'_>) -> Result<(), E>,
    ) -> Result<(), Error<E>> {
        let mut consumed = 0;
        while consumed < input.len() {
            self.step(input, &mut consumed, &mut emit)
                .map_err(|source| Error { consumed, source })?;
        }
        Ok(())
    }

    fn step<E>(
        &mut self,
        input: &[u8],
        consumed: &mut usize,
        emit: &mut impl FnMut(Segment<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        let byte = input[*consumed];
        match self.phase {
            Phase::Bytes if byte == 0x1b => {
                self.advance(byte);
                *consumed += 1;
                self.phase = Phase::Escape;
            }
            Phase::Bytes => {
                let start = *consumed;
                let end = input[start..]
                    .iter()
                    .position(|&byte| byte == 0x1b)
                    .map_or(input.len(), |offset| start + offset);
                let mut dispatch = Dispatch::default();
                *consumed += self
                    .parser
                    .advance_until_terminated(&mut dispatch, &input[start..end]);
                emit(Segment {
                    kind: if dispatch.reset {
                        Kind::Reset
                    } else {
                        Kind::Bytes
                    },
                    bytes: &input[start..*consumed],
                })?;
            }
            Phase::Escape if byte == b']' => {
                self.advance(byte);
                *consumed += 1;
                self.buffer[..2].copy_from_slice(b"\x1b]");
                self.length = 2;
                self.phase = Phase::Osc;
            }
            Phase::Escape => {
                self.phase = Phase::Bytes;
                emit(Segment {
                    kind: Kind::Bytes,
                    bytes: b"\x1b",
                })?;
            }
            Phase::Osc if byte == 0x1b => {
                let dispatched = self.advance(byte).osc;
                *consumed += 1;
                // vte dispatches at ESC; only the following backslash confirms ST.
                self.phase = Phase::Terminator { dispatched };
            }
            Phase::Osc if matches!(byte, 0x18 | 0x1a) => {
                self.flush(Kind::Cancelled, Phase::Bytes, emit)?;
            }
            Phase::Osc if self.length == MAX_SEQUENCE => {
                self.flush(Kind::Oversized, Phase::Bytes, emit)?;
            }
            Phase::Osc => {
                let dispatch = self.advance(byte);
                self.buffer[self.length] = byte;
                self.length += 1;
                *consumed += 1;
                if byte == 7 {
                    self.flush(
                        if dispatch.osc { Kind::Osc } else { Kind::Bytes },
                        Phase::Bytes,
                        emit,
                    )?;
                }
            }
            Phase::Terminator { dispatched } if byte == b'\\' => {
                if self.length > MAX_SEQUENCE - 2 {
                    // Keep ESC pending so it is forwarded exactly once after this prefix.
                    self.flush(Kind::Oversized, Phase::Escape, emit)?;
                } else {
                    self.advance(byte);
                    *consumed += 1;
                    self.buffer[self.length..self.length + 2].copy_from_slice(b"\x1b\\");
                    self.length += 2;
                    self.flush(
                        if dispatched { Kind::Osc } else { Kind::Bytes },
                        Phase::Bytes,
                        emit,
                    )?;
                }
            }
            Phase::Terminator { .. } => {
                self.flush(Kind::Cancelled, Phase::Escape, emit)?;
            }
        }
        Ok(())
    }

    /// Flushes retained bytes and resets VT state for a new independent stream.
    /// The reset and delivery take effect even when the callback returns Err.
    pub fn finish<E>(
        &mut self,
        mut emit: impl FnMut(Segment<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        if matches!(self.phase, Phase::Escape | Phase::Terminator { .. }) {
            self.buffer[self.length] = 0x1b;
            self.length += 1;
        }
        self.parser = vte::Parser::default();
        let kind = if self.length > MAX_SEQUENCE {
            Kind::Oversized
        } else {
            Kind::Incomplete
        };
        self.flush(kind, Phase::Bytes, &mut emit)
    }

    fn advance(&mut self, byte: u8) -> Dispatch {
        let mut dispatch = Dispatch::default();
        self.parser.advance(&mut dispatch, &[byte]);
        dispatch
    }

    fn flush<E>(
        &mut self,
        kind: Kind,
        phase: Phase,
        emit: &mut impl FnMut(Segment<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        let length = self.length;
        self.length = 0;
        self.phase = phase;
        if length != 0 {
            emit(Segment {
                kind,
                bytes: &self.buffer[..length],
            })?;
        }
        Ok(())
    }
}
