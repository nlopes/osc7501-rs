# osc7501-rs

Send **OSC 7501 Program Status Protocol, revision 0.4** reports through your
terminal application's existing output writer. Terminal hosts can also send
capability replies. The terminal decides how to display reports.

The Cargo package is `osc7501-rs`. Its Rust library is `osc7501`.

```toml
[dependencies]
osc7501 = { package = "osc7501-rs", version = "0.1" }
```

## Send a report

```rust
use osc7501::{AppId, Kind, State, Status};
use std::io::Write;

fn main() -> Result<(), osc7501::Error> {
    let mut output = std::io::stdout().lock();
    Status::new(State::Blocked)
        .app(AppId::new("my-tui")?)
        .kind(Kind::Permission)
        .message("Apply these changes?")
        .write_to(&mut output)?;
    output.flush()?;
    Ok(())
}
```

Use the same writer or lock as your screen rendering. Reports must not
interleave with screen updates. The crate validates and encodes the complete
report before writing, uses no heap allocation, and leaves flushing to you.
An I/O failure can leave a partial frame.

A report without an id describes the whole program. Use `RecordId::new` and
`.id(id)` for child tasks such as `build/test`. Each report replaces all fields
of its record, so repeat fields you want to keep. `Status::clear().id(id)`
removes that record and its children. Clearing root removes every record.

Use `Progress::new` for percentages from 0 through 100. Progress applies to
`working` and `blocked`; a blocking kind applies only to `blocked`. Omit
progress when the amount of work is unknown. Send `idle` after cancellation
and `done` or `error` when work ends.

`osc7501::parser` exposes report and capability parsing. For parsing alone,
use the separate `osc7501-parser` package, which works without `std`.

## Reply from a terminal host

```rust
use osc7501::{CapabilityReply, State};
use std::io::Write;

fn answer_query(output: &mut dyn Write) -> Result<(), osc7501::Error> {
    CapabilityReply::new()
        .states(&[State::Idle, State::Working, State::Done, State::Error])
        .kinds(&[])
        .write_to(output)
}
```

Advertise only features the host implements. Omitted lists use the fixed
revision 0.4 defaults. An explicit empty list advertises no support for that
category. Typed lists preserve order and duplicates and contain only known
states or kinds; `clear` is an action and is never advertised.

`CapabilityReply::new().write_to(output)` sends a bare default reply. Its bytes
match `write_query`; both parse as `parser::Event::Query`. Use stream direction
and pending-query state to decide whether to answer or consume that event.
The caller selects the writer and decides when to reply.

Replies use no heap allocation and do not flush. Replies above the 4096-byte
complete-frame cap fail with `Error::Parser(parser::Error::Limit)` before any
caller output. Writer failures preserve their I/O source and can leave a
partial frame.

## Keep terminal records

Enable the optional `store` feature to also use `osc7501::store`:

```toml
[dependencies]
osc7501 = { package = "osc7501-rs", version = "0.1", features = ["store"] }
```

```rust
# #[cfg(feature = "store")]
fn main() -> Result<(), osc7501::store::Error> {
    use osc7501::{
        parser::{Event, parse_sequence},
        store::{MIN_HOST_CAPACITY, Slot, Store},
    };

    let mut slots = [const { Slot::EMPTY }; MIN_HOST_CAPACITY];
    let mut store = Store::new(&mut slots)?;
    let input = b"\x1b]7501;state=done:app=cargo:msg=T0s=\x1b\\";
    if let Some(Event::Report(report)) = parse_sequence(input)? {
        store.apply(report, |_| {})?;
    }
    assert_eq!(store.get(None).and_then(|record| record.message()), Some("OK"));
    Ok(())
}
# #[cfg(not(feature = "store"))]
# fn main() {}
```

The feature is off by default. Sending and parsing reports does not require it.
The store's `parser` re-export exposes the same parser types. Applications that
only receive and retain reports can depend on `osc7501-store` directly; that
crate works without `std` and does not depend on this producer.

Run the [host example](https://github.com/nlopes/osc7501-rs/blob/main/crates/osc7501/examples/host.rs)
from the repository root:

```sh
cargo run -p osc7501-rs --example host --features store
```

It extends the TUI release playground with retained host records, decoded
fields, app inheritance, and process-exit cleanup. Press `s` to start, `f` to
fail a selected task, and `r` to retry. `X` drops the simulated producer while
completed and failed results remain visible. Use `i` to inspect the store API.
The [store overview](https://github.com/nlopes/osc7501-rs/blob/main/crates/osc7501-store/README.md#run-a-terminal-host-example)
lists all controls. Piped output stays plain; `-- --plain` requests a text trace.

Rust 1.88 or later is required. The maximum status report this producer emits
is 3220 bytes; the protocol's complete-frame cap is 4096 bytes.

- [API documentation](https://docs.rs/osc7501-rs)
- [Protocol specification](https://www.superlogical.com/rex/docs/build/program-status)
