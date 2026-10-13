use crate::Limit;

/// Failure to validate a value, parse OSC 7501 input, or decode its text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Invalid application identifier grammar or length.
    #[error("invalid application identifier")]
    InvalidAppId,
    /// Invalid record identifier grammar, length, or depth.
    #[error("invalid record identifier")]
    InvalidRecordId,
    /// A percentage exceeds 100.
    #[error("progress must be between 0 and 100")]
    InvalidProgress,
    /// Not exactly one complete OSC 7501 sequence.
    #[error("invalid OSC 7501 framing")]
    Framing,
    /// A hard protocol cap was exceeded.
    #[error("OSC 7501 {0:?} limit exceeded")]
    Limit(Limit),
    /// The caller's text destination does not have enough capacity.
    #[error("text destination too small")]
    BufferTooSmall,
    /// Invalid base64 alphabet, padding, length, or nonzero unused bits.
    #[error("invalid base64 text")]
    InvalidBase64,
    /// Decoded bytes are not valid UTF-8.
    #[error("invalid UTF-8 text")]
    InvalidUtf8,
    /// Decoded text contains a C0, DEL, or C1 control character.
    #[error("control character in text")]
    ControlCharacter,
}
