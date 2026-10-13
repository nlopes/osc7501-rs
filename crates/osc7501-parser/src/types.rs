use crate::{Error, MAX_APP, MAX_DEPTH, MAX_ID, MAX_SEGMENT, base64};

/// A record's state. Clearing is represented separately by [`Action::Clear`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// Waiting for a new instruction, including after cancellation.
    Idle,
    /// Performing work without user intervention.
    Working,
    /// A result is ready and has not yet been acknowledged.
    Done,
    /// Cannot continue without user intervention.
    Blocked,
    /// Work failed and stopped.
    Error,
}

impl State {
    /// Returns the protocol spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Error => "error",
        }
    }

    /// Recognizes a revision 0.4 state, preserving unknown states as absent.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Why a blocked record needs the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// Approval to perform an action.
    Permission,
    /// An answer to a question.
    Question,
    /// Authentication or credentials.
    Auth,
}

impl Kind {
    /// Returns the protocol spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }

    /// Recognizes a revision 0.4 blocking reason.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "permission" => Some(Self::Permission),
            "question" => Some(Self::Question),
            "auth" => Some(Self::Auth),
            _ => None,
        }
    }
}

/// An update or a subtree removal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    /// Completely replaces the addressed record.
    Set(State),
    /// Removes the record and its descendants; root clears everything.
    Clear,
}

impl Action {
    /// Returns the spelling used in the `state` field.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Set(state) => state.as_str(),
            Self::Clear => "clear",
        }
    }
}

/// A validated percentage in the inclusive range 0..=100.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress(u8);

impl Progress {
    /// Validates a percentage without allocating.
    ///
    /// # Errors
    /// Returns [`Error::InvalidProgress`] for percentages above 100.
    pub const fn new(value: u8) -> Result<Self, Error> {
        if value <= 100 {
            Ok(Self(value))
        } else {
            Err(Error::InvalidProgress)
        }
    }

    /// Returns the percentage.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Progress {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

pub(crate) fn segment_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
}

/// A validated hierarchical identifier. Root is represented by `None`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordId<'a>(&'a str);

impl<'a> RecordId<'a> {
    /// Validates a non-root identifier without allocating.
    /// Segments use ASCII letters, digits, `_`, `.`, `+`, or `-`.
    ///
    /// # Errors
    /// Returns [`Error::InvalidRecordId`] if the identifier or any segment is
    /// empty, contains forbidden characters, or exceeds a length or depth limit.
    pub fn new(value: &'a str) -> Result<Self, Error> {
        if value.is_empty() || value.len() > MAX_ID {
            return Err(Error::InvalidRecordId);
        }
        let mut depth = 0;
        for segment in value.split('/') {
            depth += 1;
            if depth > MAX_DEPTH
                || segment.is_empty()
                || segment.len() > MAX_SEGMENT
                || !segment.bytes().all(segment_byte)
            {
                return Err(Error::InvalidRecordId);
            }
        }
        Ok(Self(value))
    }

    /// Returns the borrowed identifier.
    #[must_use]
    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> TryFrom<&'a str> for RecordId<'a> {
    type Error = Error;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// A validated stable application identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppId<'a>(&'a str);

impl<'a> AppId<'a> {
    /// Validates a stable identifier without allocating.
    ///
    /// # Errors
    /// Returns [`Error::InvalidAppId`] if the identifier is empty, exceeds
    /// [`MAX_APP`], or contains characters other than ASCII letters, digits,
    /// `_`, `.`, `+`, or `-`.
    pub fn new(value: &'a str) -> Result<Self, Error> {
        if value.is_empty() || value.len() > MAX_APP || !value.bytes().all(segment_byte) {
            Err(Error::InvalidAppId)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the borrowed identifier.
    #[must_use]
    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> TryFrom<&'a str> for AppId<'a> {
    type Error = Error;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Base64 text whose UTF-8 and absence of controls have already been validated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedText<'a> {
    pub(crate) encoded: &'a str,
    pub(crate) decoded_len: usize,
}

impl<'a> EncodedText<'a> {
    /// Returns the original encoded text.
    #[must_use]
    pub const fn as_base64(self) -> &'a str {
        self.encoded
    }

    /// Exact output capacity needed to decode this value.
    #[must_use]
    pub const fn decoded_len(self) -> usize {
        self.decoded_len
    }

    /// Decodes into caller-owned storage, returning a UTF-8 view of that storage.
    ///
    /// # Errors
    /// Returns [`Error::BufferTooSmall`] when the destination is too small.
    pub fn decode_into(self, output: &mut [u8]) -> Result<&str, Error> {
        let length = base64::decode(self.encoded.as_bytes(), output)?;
        core::str::from_utf8(&output[..length]).map_err(|_| Error::InvalidUtf8)
    }
}

/// A validated report. Missing fields remove the prior record's fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report<'a> {
    /// Complete replacement or subtree clear.
    pub action: Action,
    /// Addressed record; `None` means root.
    pub id: Option<RecordId<'a>>,
    /// Present only for `blocked`.
    pub kind: Option<Kind>,
    /// Present only for `working` or `blocked`; absent means indeterminate.
    pub progress: Option<Progress>,
    /// Explicit application identifier, before ancestor inheritance.
    pub app: Option<AppId<'a>>,
    /// Validated, lazily decoded label.
    pub title: Option<EncodedText<'a>>,
    /// Validated, lazily decoded status message.
    pub message: Option<EncodedText<'a>>,
}

/// Feature detection reply. Explicit lists retain future, unknown tokens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities<'a> {
    /// Raw advertised list; absence means the fixed revision 0.4 defaults.
    pub states: Option<&'a str>,
    /// Raw advertised blocking reasons; absence means the fixed defaults.
    pub kinds: Option<&'a str>,
}

impl Capabilities<'_> {
    /// Iterates advertised states, including future states.
    pub fn states(&self) -> impl Iterator<Item = &str> {
        self.states
            .unwrap_or("idle,working,done,blocked,error")
            .split(',')
            .filter(|value| !value.is_empty())
    }

    /// Iterates advertised blocking reasons, including future reasons.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.kinds
            .unwrap_or("permission,question,auth")
            .split(',')
            .filter(|value| !value.is_empty())
    }

    /// Checks whether a known state is supported.
    #[must_use]
    pub fn supports_state(&self, state: State) -> bool {
        self.states().any(|value| value == state.as_str())
    }

    /// Checks whether a known blocking reason is supported.
    #[must_use]
    pub fn supports_kind(&self, kind: Kind) -> bool {
        self.kinds().any(|value| value == kind.as_str())
    }
}

/// A recognized protocol event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event<'a> {
    /// A valid report, ready to apply atomically.
    Report(Report<'a>),
    /// Bare `?`: a query when read from a program, or a default reply from a terminal.
    /// Direction is deliberately left to the caller because the bytes are identical.
    Query,
    /// A feature reply with extension pairs.
    Capabilities(Capabilities<'a>),
}

/// A hard protocol bound that was exceeded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Limit {
    /// Complete OSC frame.
    Sequence,
    /// Any key, including unknown keys.
    Key,
    /// Encoded or decoded message.
    Message,
    /// Encoded or decoded title.
    Title,
    /// Application identifier.
    App,
    /// Identifier length, segment length, or depth.
    Id,
}
