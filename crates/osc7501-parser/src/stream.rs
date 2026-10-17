use core::convert::Infallible;

use crate::{Error as ParserError, Event, Limit, MAX_SEQUENCE, parse_sequence};

const PREFIX: &[u8] = b"\x1b]7501;";

/// A callback failure from [`Decoder::try_feed`], with the position for resuming.
///
/// The callback can return any error or stop value. This wrapper implements
/// [`core::error::Error`] when `E: core::error::Error + 'static`.
#[derive(Debug, thiserror::Error)]
#[error("event callback failed after {consumed} input bytes")]
pub struct Error<E> {
    consumed: usize,
    source: E,
}

impl<E> Error<E> {
    /// Bytes consumed from this call's input, including the byte that triggered
    /// the callback. Resume with `&input[failure.consumed()..]` on the same decoder.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// Borrows the original callback error or stop value.
    #[must_use]
    pub const fn get_ref(&self) -> &E {
        &self.source
    }

    /// Returns the original callback error or stop value.
    #[must_use]
    pub fn into_inner(self) -> E {
        self.source
    }
}

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

/// Bounded OSC scanner for chunks of terminal output.
///
/// Holds at most one 4096-byte frame, never allocates, and skips other OSCs
/// through their terminator. Each callback borrows this decoder's buffer only
/// for that callback; decode or copy what you need before it returns.
/// Oversized target frames produce one error and are drained through ST.
/// Only seven-bit OSC/ST and BEL are recognized, as specified by revision 0.4.
/// This scanner does not track full VT context or forward unrelated bytes.
/// See the crate's terminal-stream example for that integration.
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
    pub fn feed(&mut self, input: &[u8], mut on_event: impl FnMut(Result<Event<'_>, ParserError>)) {
        if let Err(failure) = self.try_feed(input, |event| {
            on_event(event);
            Ok::<(), Infallible>(())
        }) {
            match failure.source {}
        }
    }

    /// Consumes bytes until the input ends or the callback returns an error.
    /// Ignored reports and unrelated output produce no callback.
    ///
    /// On success, all input was consumed. On callback failure, the delivered
    /// event stays consumed and the remaining input is untouched. Resume with
    /// `&input[failure.consumed()..]` on this decoder. Callback effects are not
    /// rolled back. The callback must copy any event data it needs to retain.
    ///
    /// An oversized frame produces one parser error when it exceeds the cap.
    /// If the callback stops before the terminator, resuming drains the rest
    /// of that frame without delivering the error again.
    ///
    /// # Errors
    /// Returns the callback's error and the consumed byte count in [`Error`].
    /// Parser errors are passed to the callback; they do not stop feeding unless
    /// the callback returns an error.
    ///
    /// # Examples
    /// Record one event before pausing, then resume with the remaining input.
    /// The recorded text owns its data because events borrow the decoder.
    ///
    /// ```
    /// use osc7501_parser::{Decoder, Error, Event};
    ///
    /// let mut decoder = Decoder::new();
    /// let input = b"\x1b]7501;?\x07\x1b]7501;state=done\x07";
    /// let mut events = Vec::new();
    /// let mut record = |event: Result<Event<'_>, Error>| {
    ///     events.push(event.map(|event| format!("{event:?}")));
    /// };
    ///
    /// let result = decoder.try_feed(input, |event| {
    ///     record(event);
    ///     Err(())
    /// });
    /// if let Err(stopped) = result {
    ///     decoder.feed(&input[stopped.consumed()..], record);
    /// }
    /// assert_eq!(events.len(), 2);
    /// ```
    pub fn try_feed<E>(
        &mut self,
        input: &[u8],
        mut on_event: impl FnMut(Result<Event<'_>, ParserError>) -> Result<(), E>,
    ) -> Result<(), Error<E>> {
        for (index, &byte) in input.iter().enumerate() {
            let failure = |source| Error {
                consumed: index + 1,
                source,
            };
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
                        self.phase = if terminated {
                            Phase::Ground
                        } else {
                            skip(byte, escaped)
                        };
                        self.length = 0;
                        on_event(Err(ParserError::Limit(Limit::Sequence))).map_err(failure)?;
                    } else {
                        self.buffer[self.length] = byte;
                        self.length += 1;
                        if terminated {
                            let length = self.length;
                            self.reset();
                            match parse_sequence(&self.buffer[..length]) {
                                Ok(Some(event)) => on_event(Ok(event)).map_err(failure)?,
                                Err(error) => on_event(Err(error)).map_err(failure)?,
                                Ok(None) => {}
                            }
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
        Ok(())
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
