# osc7501-rs

Tell your terminal when your program is working, needs input, or has finished.
This crate sends [OSC 7501](https://www.superlogical.com/rex/docs/build/program-status)
revision 0.4 reports through your existing output writer. A supporting terminal
decides how to display them.

## Send a status

```toml
[dependencies]
osc7501 = { package = "osc7501-rs", version = "0.1" }
```

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

In a TUI, use the same writer as your screen rendering to keep writes in order.
You control flushing. Validation and encoding finish before writing starts;
an I/O error can still leave a partial frame.

## Update or clear a status

- Each report replaces all fields of its record. Repeat fields you want to keep.
- A report without an id describes the whole program. Use `RecordId::new`
  and `.id(id)` for child tasks such as `build/test`.
- Use `Progress::new` for percentages from 0 to 100 on `working` or `blocked`
  reports. Omit progress when the amount of work is unknown.
- A blocking kind applies only to `blocked`. Send `idle` after cancellation,
  or `done` or `error` when work ends.
- `Status::clear().id(id)` removes that task and its children.
  `Status::clear()` removes every record.

## Reply from a terminal host

Use `CapabilityReply` with the writer that sends input to the program.
Advertise only what your host implements:

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

Omitted lists use the revision 0.4 defaults. Empty lists advertise no support.
Replies follow the same writing rules as reports and must fit within 4096 bytes.

A default reply has the same bytes as `write_query`; both parse as
`parser::Event::Query`. Use stream direction and pending-query state to tell
them apart.

## Parse and keep reports

`osc7501::parser` exposes the [parser](https://docs.rs/osc7501-parser).
Add `features = ["store"]` to the dependency above to also use
`osc7501::store`. Use `features = ["store-alloc"]` to also enable owned storage
with `Store::new(capacity)`. The [store guide](https://docs.rs/osc7501-store) shows how to
keep records and handle terminal cleanup.

The parser and store also work as separate dependencies without `std`.

## Try it

From a repository checkout, run the interactive release playground:

```sh
cargo run -p osc7501-rs --example tui
```

Press `s` to start and `q` to quit. The screen shows task progress, reports,
and the builder calls that produce them. Use a terminal of at least 80 columns
and 24 rows; protocol support is optional.

See the [repository README](https://github.com/nlopes/osc7501-rs#try-the-examples)
for the host and text demos, or the [API documentation](https://docs.rs/osc7501-rs)
for the full interface.
