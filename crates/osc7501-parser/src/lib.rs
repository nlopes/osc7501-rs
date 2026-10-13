//! Allocation-free parser for Program Status Protocol revision 0.4.
//!
//! [`parse_sequence`] parses a complete OSC; [`parse_body`] integrates with a
//! terminal's existing OSC dispatcher; [`Decoder`] accepts arbitrary byte chunks.
//! Reports borrow their input. Text is validated before any report is returned,
//! then decoded on demand into caller-owned storage.
#![no_std]

mod base64;
mod constants;
mod parse;
mod stream;
mod types;

pub use constants::{
    MAX_APP, MAX_DEPTH, MAX_ID, MAX_KEY, MAX_MESSAGE, MAX_MESSAGE_ENCODED, MAX_SEGMENT,
    MAX_SEQUENCE, MAX_TITLE, MAX_TITLE_ENCODED,
};
pub use parse::{parse_body, parse_sequence};
pub use stream::Decoder;
pub use types::{
    Action, AppId, Capabilities, DecodeError, EncodedText, Event, Kind, Limit, ParseError,
    Progress, RecordId, Report, State,
};
