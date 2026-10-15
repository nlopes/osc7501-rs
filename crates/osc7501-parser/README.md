# osc7501-parser

Read status reports and terminal capability replies from
[OSC 7501](https://www.superlogical.com/rex/docs/build/program-status), revision 0.4.
Works without `std` or an allocator.

## Parse a report

```toml
[dependencies]
osc7501-parser = "0.1"
```

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

## Choose a parser

| You have | Use |
| --- | --- |
| A complete OSC sequence ending in BEL or ESC-backslash | `parse_sequence` |
| A body from your terminal's escape-sequence handler | `parse_body` |
| A stream split across reads | `Decoder::feed` |

Reports borrow their input. Messages and titles stay base64 encoded until you
decode them, but the parser checks UTF-8 and rejects control characters first.
In a decoder callback, copy or decode what you need before the callback returns.

## Handle the result

The two parsing functions return:

- `Ok(Some(event))` for an accepted report, query, or capability reply.
- `Ok(None)` for a report with a missing or unknown state, or an invalid record id.
- `Err(error)` for invalid framing, invalid text, or an exceeded limit.

Invalid app ids and progress values are treated as absent. Complete frames
are limited to 4096 bytes; decoded messages to 2048 bytes and titles to 192.
`Decoder::feed` calls you for accepted events and errors, and skips ignored reports.

Capability replies skip unknown names and pairs. Missing lists use revision 0.4
defaults; empty lists remain empty. A bare `Event::Query` can also be a default
reply, so use stream direction and pending-query state to tell them apart.

The parser returns events without keeping records. For current status and
terminal cleanup, use [osc7501-store](https://docs.rs/osc7501-store).

See the [API documentation](https://docs.rs/osc7501-parser) for the full interface.
