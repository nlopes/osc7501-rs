//! Allocation-free parser for Program Status Protocol revision 0.4.
//!
//! [`parse_sequence`] parses a complete OSC; [`parse_body`] integrates with a
//! terminal's existing OSC dispatcher; [`Decoder`] accepts arbitrary byte chunks.
//! Reports borrow their input. Text is validated before any report is returned,
//! then decoded on demand into caller-owned storage.
#![no_std]

mod base64;
mod parse;
mod stream;
mod types;

pub use parse::{parse_body, parse_sequence};
pub use stream::Decoder;
pub use types::{
    Action, AppId, Capabilities, DecodeError, EncodedText, Event, Kind, Limit, ParseError,
    Progress, RecordId, Report, State,
};

/// Maximum complete sequence size, including OSC and terminator.
pub const MAX_SEQUENCE: usize = 4096;
/// Maximum encoded message length.
pub const MAX_MESSAGE_ENCODED: usize = 2732;
/// Maximum decoded message length.
pub const MAX_MESSAGE: usize = 2048;
/// Maximum encoded title length.
pub const MAX_TITLE_ENCODED: usize = 256;
/// Maximum decoded title length.
pub const MAX_TITLE: usize = 192;
/// Maximum key length.
pub const MAX_KEY: usize = 16;
/// Maximum application identifier length.
pub const MAX_APP: usize = 32;
/// Maximum record identifier length.
pub const MAX_ID: usize = 128;
/// Maximum identifier segment length.
pub const MAX_SEGMENT: usize = 32;
/// Maximum number of identifier segments.
pub const MAX_DEPTH: usize = 8;
