use crate::{Event, Limit, MAX_SEQUENCE, ParseError, parse_sequence};

const PREFIX: &[u8] = b"\x1b]7501;";

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    Ground,
    Escape,
    Prefix(u8),
    Body,
    BodyEscape,
    Ignore,
    IgnoreEscape,
}

/// Bounded scanner for arbitrary chunks of terminal output.
///
/// Holds at most one 4096-byte frame, never allocates, and skips other OSCs
/// through their terminator. Each callback borrows this decoder's buffer only
/// for that callback; decode or copy what you need before it returns.
/// Oversized target frames produce one error and are drained through ST.
/// Only seven-bit OSC/ST and BEL are recognized, as specified by revision 0.4.
pub struct Decoder {
    buffer: [u8; MAX_SEQUENCE],
    length: usize,
    phase: Phase,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// Creates an empty decoder with a fixed-size inline buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: [0; MAX_SEQUENCE],
            length: 0,
            phase: Phase::Ground,
        }
    }

    /// Discards a partial frame, for example after a terminal reset.
    pub fn reset(&mut self) {
        self.length = 0;
        self.phase = Phase::Ground;
    }

    /// Consumes bytes and invokes the callback for accepted events or rejected
    /// target frames. Ignored reports and unrelated output produce no callback.
    pub fn feed(&mut self, input: &[u8], mut on_event: impl FnMut(Result<Event<'_>, ParseError>)) {
        for &byte in input {
            match self.phase {
                Phase::Ground => {
                    if byte == 0x1b {
                        self.phase = Phase::Escape;
                    }
                }
                Phase::Escape => match byte {
                    b']' => {
                        self.buffer[..2].copy_from_slice(&PREFIX[..2]);
                        self.length = 2;
                        self.phase = Phase::Prefix(2);
                    }
                    0x1b => {}
                    _ => self.phase = Phase::Ground,
                },
                Phase::Prefix(index) => {
                    if byte == PREFIX[usize::from(index)] {
                        self.buffer[self.length] = byte;
                        self.length += 1;
                        self.phase = if self.length == PREFIX.len() {
                            Phase::Body
                        } else {
                            Phase::Prefix(index + 1)
                        };
                    } else {
                        self.phase = skip(byte, false);
                    }
                }
                Phase::Body | Phase::BodyEscape => {
                    let escaped = matches!(self.phase, Phase::BodyEscape);
                    let terminated = byte == 0x07 || (escaped && byte == b'\\');
                    if self.length == MAX_SEQUENCE {
                        on_event(Err(ParseError::Limit(Limit::Sequence)));
                        self.phase = if terminated {
                            Phase::Ground
                        } else {
                            skip(byte, escaped)
                        };
                        self.length = 0;
                    } else {
                        self.buffer[self.length] = byte;
                        self.length += 1;
                        if terminated {
                            match parse_sequence(&self.buffer[..self.length]) {
                                Ok(Some(event)) => on_event(Ok(event)),
                                Err(error) => on_event(Err(error)),
                                Ok(None) => {}
                            }
                            self.reset();
                        } else {
                            self.phase = if byte == 0x1b {
                                Phase::BodyEscape
                            } else {
                                Phase::Body
                            };
                        }
                    }
                }
                Phase::Ignore | Phase::IgnoreEscape => {
                    self.phase = skip(byte, matches!(self.phase, Phase::IgnoreEscape));
                }
            }
        }
    }
}

fn skip(byte: u8, escaped: bool) -> Phase {
    if byte == 0x07 || (escaped && byte == b'\\') {
        Phase::Ground
    } else if byte == 0x1b {
        Phase::IgnoreEscape
    } else {
        Phase::Ignore
    }
}
