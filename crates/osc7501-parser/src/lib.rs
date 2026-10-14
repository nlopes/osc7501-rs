#![doc = include_str!("../README.md")]
#![no_std]

mod base64;
mod constants;
mod error;
mod parse;
mod stream;
mod types;

pub use constants::{
    MAX_APP, MAX_DEPTH, MAX_ID, MAX_KEY, MAX_MESSAGE, MAX_MESSAGE_ENCODED, MAX_SEGMENT,
    MAX_SEQUENCE, MAX_TITLE, MAX_TITLE_ENCODED,
};
pub use error::Error;
pub use parse::{parse_body, parse_sequence};
pub use stream::Decoder;
pub use types::{
    Action, AppId, Capabilities, EncodedText, Event, Kind, Limit, Progress, RecordId, Report, State,
};
