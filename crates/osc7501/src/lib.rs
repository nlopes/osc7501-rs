//! OSC 7501 emission through an application's existing output writer.
//!
//! Validation happens before writing any bytes. Emission uses fixed-size stack
//! storage and no heap allocation. Share and lock your TUI's output writer so
//! status frames cannot interleave with screen updates. Flush at your chosen
//! output boundary; this crate does not flush implicitly.

mod constants;

use std::io::{self, Write};

use constants::ALPHABET;

pub use osc7501_parser::{
    Action, AppId, Capabilities, Decoder, Event, Kind, MAX_MESSAGE, MAX_SEQUENCE, MAX_TITLE,
    Progress, RecordId, State,
};

/// Plain-text report to encode. `None` means absent, not "keep prior value".
#[derive(Clone, Copy, Debug)]
pub struct Status<'a> {
    action: Action,
    id: Option<RecordId<'a>>,
    app: Option<AppId<'a>>,
    title: Option<&'a str>,
    message: Option<&'a str>,
    kind: Option<Kind>,
    progress: Option<Progress>,
}

impl<'a> Status<'a> {
    /// Begins a complete replacement for the root record.
    #[must_use]
    pub const fn new(state: State) -> Self {
        Self::action(Action::Set(state))
    }

    /// Removes the addressed subtree, or all records when no id is supplied.
    #[must_use]
    pub const fn clear() -> Self {
        Self::action(Action::Clear)
    }

    const fn action(action: Action) -> Self {
        Self {
            action,
            id: None,
            app: None,
            title: None,
            message: None,
            kind: None,
            progress: None,
        }
    }

    /// Addresses a validated child record.
    #[must_use]
    pub const fn id(mut self, id: RecordId<'a>) -> Self {
        self.id = Some(id);
        self
    }

    /// Adds a validated application identifier; repeat it on every root update.
    #[must_use]
    pub const fn app(mut self, app: AppId<'a>) -> Self {
        self.app = Some(app);
        self
    }

    /// Adds a single-line label, validated when sent.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Adds a single-line message, validated when sent.
    #[must_use]
    pub const fn message(mut self, message: &'a str) -> Self {
        self.message = Some(message);
        self
    }

    /// Adds the blocking reason. Sending this on another state is an error.
    #[must_use]
    pub const fn kind(mut self, kind: Kind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Adds a percentage. Sending this outside working/blocked is an error.
    #[must_use]
    pub const fn progress(mut self, progress: Progress) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Encodes a complete report and writes it through the caller's writer.
    /// This method does not flush the writer.
    ///
    /// # Errors
    /// Returns a validation error before writing if text is invalid or fields
    /// are incompatible with the state. Returns an I/O error if writing fails;
    /// I/O failures can leave a partial frame, as with any `write_all` operation.
    pub fn write_to<W: Write + ?Sized>(self, output: &mut W) -> Result<(), Error> {
        self.validate()?;
        let mut frame = [0; MAX_SEQUENCE];
        let mut cursor = io::Cursor::new(frame.as_mut_slice());
        cursor.write_all(b"\x1b]7501;state=")?;
        cursor.write_all(self.action.as_str().as_bytes())?;
        for (key, value) in [
            ("id", self.id.map(RecordId::as_str)),
            ("app", self.app.map(AppId::as_str)),
            ("kind", self.kind.map(Kind::as_str)),
        ] {
            if let Some(value) = value {
                write!(cursor, ":{key}={value}")?;
            }
        }
        if let Some(progress) = self.progress {
            write!(cursor, ":progress={}", progress.get())?;
        }
        if let Some(title) = self.title {
            cursor.write_all(b":title=")?;
            encode(title.as_bytes(), &mut cursor)?;
        }
        if let Some(message) = self.message {
            cursor.write_all(b":msg=")?;
            encode(message.as_bytes(), &mut cursor)?;
        }
        cursor.write_all(b"\x1b\\")?;
        let length = usize::try_from(cursor.position()).map_err(|_| Error::FrameTooLarge)?;
        output.write_all(&frame[..length])?;
        Ok(())
    }

    fn validate(self) -> Result<(), Error> {
        if self.kind.is_some() && self.action != Action::Set(State::Blocked) {
            return Err(Error::IncompatibleField);
        }
        if self.progress.is_some()
            && !matches!(self.action, Action::Set(State::Working | State::Blocked))
        {
            return Err(Error::IncompatibleField);
        }
        for (text, cap) in [(self.title, MAX_TITLE), (self.message, MAX_MESSAGE)] {
            if let Some(text) = text {
                if text.len() > cap {
                    return Err(Error::TextTooLong);
                }
                if text.chars().any(char::is_control) {
                    return Err(Error::ControlCharacter);
                }
            }
        }
        Ok(())
    }
}

fn encode(input: &[u8], output: &mut impl Write) -> io::Result<()> {
    for chunk in input.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        let encoded = [
            ALPHABET[usize::from(a >> 2)],
            ALPHABET[usize::from((a & 3) << 4 | b >> 4)],
            if chunk.len() > 1 {
                ALPHABET[usize::from((b & 15) << 2 | c >> 6)]
            } else {
                b'='
            },
            if chunk.len() > 2 {
                ALPHABET[usize::from(c & 63)]
            } else {
                b'='
            },
        ];
        output.write_all(&encoded)?;
    }
    Ok(())
}

/// Writes an optional feature query without reading input or imposing a timeout.
/// This function does not flush the writer.
///
/// Route replies through the TUI's existing input reader and [`Decoder`].
/// A bare query and a default reply have identical bytes; use stream direction
/// and a pending-query flag to distinguish them.
///
/// # Errors
/// Returns the writer's I/O error.
pub fn write_query<W: Write + ?Sized>(output: &mut W) -> io::Result<()> {
    output.write_all(b"\x1b]7501;?\x1b\\")
}

/// Validation, parsing, or transport failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A title or message exceeds the decoded byte cap.
    #[error("status text exceeds its byte cap")]
    TextTooLong,
    /// Text contains a forbidden control character.
    #[error("status text contains a control character")]
    ControlCharacter,
    /// Kind or progress is incompatible with this state.
    #[error("status field is incompatible with its state")]
    IncompatibleField,
    /// The encoded frame exceeds available space.
    #[error("status frame exceeds its byte cap")]
    FrameTooLarge,
    /// A value constructor, parser, or text decoder failed.
    #[error("{0}")]
    Parser(#[from] osc7501_parser::Error),
    /// The caller's output writer failed.
    #[error("{0}")]
    Io(#[from] io::Error),
}
