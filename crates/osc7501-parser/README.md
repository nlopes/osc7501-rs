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
| OSC scanning across reads | `Decoder::feed` |
| OSC scanning whose event consumer can fail or pause | `Decoder::try_feed` |
| VT context and original frame bytes | The terminal-stream example below |

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

`Decoder::try_feed` stops when its callback returns `Err`. The callback can use
any error type or a simple stop value such as `()`. Its `stream::Error` provides
`consumed()` for the number of bytes consumed, `get_ref()` to inspect the original
value, and `into_inner()` to recover it by value. The wrapper implements
`core::error::Error` with a source chain when the callback error implements
`core::error::Error + 'static`.

To propagate only the callback error, use `.map_err(stream::Error::into_inner)?`.
This also works with callbacks returning `anyhow::Result` or boxed errors.
Keep the wrapper when you need the consumed position to resume.

Resume on the same decoder with `&input[failure.consumed()..]`. The delivered
event stays consumed, so copy any data you need before returning an error.
Callback effects are not rolled back. If feeding stops at an oversized frame,
resuming drains the rest of that frame without reporting its error again.
The `Decoder::try_feed` API documentation includes a stop/resume example.

Splitting input across calls does not change the event or error sequence.
An empty input changes nothing. These rules also apply when you stop and resume
with `try_feed`.

Capability replies skip unknown names and pairs. Missing lists use revision 0.4
defaults; empty lists remain empty. A bare `Event::Query` can also be a default
reply, so use stream direction and pending-query state to tell them apart.

Equality compares parsed representations. Reports with padded and unpadded
base64 can compare unequal even when their decoded text matches. Compare
decoded fields for report meaning. Capability equality includes the original
lists; use their iterators or support queries to compare known capabilities.

The parser returns events without keeping records. For current status and
terminal cleanup, use [osc7501-store](https://docs.rs/osc7501-store).

## Observe a terminal stream

The supported
[terminal-stream example](https://github.com/nlopes/osc7501-rs/tree/main/crates/osc7501-parser/examples/terminal_stream.rs)
forwards stdin unchanged to stdout and reports OSC 7501 observations on stderr:

```sh
cargo run -p osc7501-parser --example terminal_stream --quiet < recording.bin > forwarded.bin
```

Use this example when you need VT cancellation, original frame bytes, or full
reset notifications. `Decoder` is an OSC scanner; it does not track the complete
VT parser state or retain unrelated output. If your host already has a terminal
parser, use its dispatcher and pass the original OSC body to `parse_body`.

The example uses `vte` 0.15 with default features disabled and bounded storage.
It recognizes literal ESC-`]` frames ending in BEL or ESC-backslash. Cancelled,
oversized, and unfinished prefixes pass through without becoming reports.
Unknown OSCs, ignored reports, rejected reports, and ordinary output keep their
original bytes. It follows `vte`'s UTF-8 terminal model; it does not decode tmux
passthrough or eight-bit OSC/ST framing.

`Adapter::try_feed` delivers borrowed segments in order. A callback can parse,
copy, forward, or replace each segment. After a callback stops, resume on the
same adapter at `error.consumed()`. A retained prefix can be delivered before
any new byte is consumed, so this offset can be zero. The delivered segment
stays consumed. Call `finish` at EOF to flush pending bytes and reset VT state
before an independent stream. The filter aborts on I/O errors; a failed write
can leave partial output and is not retried as a whole segment.

The adapter remains example code. `vte` is a development dependency; the parser
library keeps its existing dependencies and allocation-free `no_std` API.

See the [API documentation](https://docs.rs/osc7501-parser) for the full interface.
