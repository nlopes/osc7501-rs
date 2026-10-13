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
