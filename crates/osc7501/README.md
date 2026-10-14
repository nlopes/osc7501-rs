# osc7501-rs

Send **OSC 7501 Program Status Protocol, revision 0.4** reports through your
terminal application's existing output writer. The terminal decides how to
display them.

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

Rust 1.88 or later is required. The maximum report this producer emits is
3220 bytes; the protocol's complete-frame cap is 4096 bytes.

- [API documentation](https://docs.rs/osc7501-rs)
- [Protocol specification](https://www.superlogical.com/rex/docs/build/program-status)
