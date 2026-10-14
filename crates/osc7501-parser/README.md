# osc7501-parser

Parse **OSC 7501 Program Status Protocol, revision 0.4** reports and terminal
capability replies. This crate works without `std` or an allocator.

```toml
[dependencies]
osc7501-parser = "0.1"
```

## Parse a report

```rust
use osc7501_parser::{Event, MAX_MESSAGE, parse_sequence};

fn main() -> Result<(), osc7501_parser::Error> {
    let input = b"\x1b]7501;state=done:app=cargo:msg=T0s=\x1b\\";
    if let Some(Event::Report(report)) = parse_sequence(input)? {
        let mut storage = [0_u8; MAX_MESSAGE];
        if let Some(message) = report.message() {
            assert_eq!(message.decode_into(&mut storage)?, "OK");
        }
    }
    Ok(())
}
```

`parse_sequence` accepts one complete OSC sequence with a BEL or ESC-backslash
terminator. Use `parse_body` when a terminal dispatcher already handles
framing. `Decoder::feed` handles input split across reads. Its callback borrows
the decoder's storage; decode or copy what you need before the callback returns.

Reports borrow their input. Messages and titles remain base64 encoded, but the
parser validates their UTF-8 and rejects control characters before returning
a report. Decode text into caller-owned storage when you need to display it.

`Ok(None)` means a report was ignored because its state is missing or unknown,
or its record id is invalid. Invalid app ids and progress values are treated
as absent. Invalid text or a report limit violation returns `Err`; no partial
report is returned. Every report replaces all fields of its addressed record.

Capability replies retain future state and kind tokens and ignore other pairs.
Missing lists use the fixed revision 0.4 defaults; explicit empty lists remain
empty. A bare `Event::Query` can be a query or a default reply. Use stream
direction and pending-query state to distinguish them.

Complete frames are limited to 4096 bytes. Decoded messages are limited to
2048 bytes and titles to 192 bytes. Parsing, streaming, and text decoding use
fixed buffers and no heap allocation. This crate does not retain records or
implement terminal display and cleanup policy.

Rust 1.88 or later is required.

- [API documentation](https://docs.rs/osc7501-parser)
- [Protocol specification](https://www.superlogical.com/rex/docs/build/program-status)
