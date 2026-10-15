# osc7501-rs

Rust libraries for **OSC 7501 Program Status Protocol, revision 0.4**. A program
can tell its terminal that it is idle, working, waiting for the user, done, or
has failed. The terminal decides how to display that status.

| Cargo package | Rust library | What it does |
| --- | --- | --- |
| `osc7501-rs` | `osc7501` | Send reports and replies; optionally retain records with `store` |
| `osc7501-parser` | `osc7501_parser` | Parse status reports and terminal capability replies |
| `osc7501-store` | `osc7501_store` | Keep current records for a terminal host |

The parser and store work without `std`. All crates process reports without
heap allocation. The store is an opt-in dependency; the parser and producer
remain useful on their own.

## Install

To send reports, add:

```toml
[dependencies]
osc7501 = { package = "osc7501-rs", version = "0.1" }
```

For parsing alone, add `osc7501-parser = "0.1"` instead.
For sending and retaining records, enable the producer's `store` feature.
For retained records without the producer, add `osc7501-store = "0.1"`.

Each published crate includes its own README and license text. Its README also
provides the crate's API overview and executable example on docs.rs.

## Send from a TUI

```rust
use osc7501::{AppId, Kind, State, Status};
use std::io::Write;

fn awaiting_approval(output: &mut dyn Write) -> Result<(), osc7501::Error> {
    let app = AppId::new("my-tui")?;
    Status::new(State::Blocked)
        .app(app)
        .kind(Kind::Permission)
        .message("Apply these changes?")
        .write_to(output)
}
```

Use the same output writer as your TUI's screen rendering. The crate leaves
flushing to you. Invalid reports are rejected before any bytes are sent.

A report without an id describes the whole program. Use an id such as
`build/test` for a child task. Each report replaces all fields of its record,
so repeat fields you want to keep. `Status::clear().id(id)` removes that task
and its children.

Send `idle` after cancellation and `done` or `error` when work ends.

## Run the protocol demo

For a colorful interactive TUI, run:

```sh
cargo run -p osc7501-rs --example tui
```

The [TUI](crates/osc7501/examples/tui.rs) simulates three concurrent release
tasks. Press `s` to start, select a task with the arrow keys or `Tab`, then use
`f` to fail it and `r` to retry. Press `p`, `a`, or `?` to request permission,
authentication, or an answer. `Enter` resumes work. `c` cancels and clears the
child records. `q` or `Ctrl+C` quits.

The screen shows colored task progress, recent reports, and the builder calls
for the root or selected child. `b` switches the API preview. It sends real
OSC 7501 reports through the same writer as Ratatui. The TUI works without
terminal protocol support; a terminal with OSC 7501 support can also display
those reports. All work and prompts are simulated. Use a terminal of at least
80 columns and 24 rows. The report log appears when the terminal has at least
29 rows.

If your environment sets `NO_COLOR`, unset it to show colors.

For a paced text demonstration, run:

```sh
cargo run -p osc7501-rs --example demo
```

The [demo](crates/osc7501/examples/demo.rs) simulates a release with child tasks,
approval prompts, and a task that fails and retries. In a terminal, it sends a
report each second and takes about 15 seconds. A terminal with OSC 7501 support
can display the reports.

Redirected output contains a plain text trace and runs without pauses.

## Parse reports

Use `osc7501::parser` when you also send reports. For parsing alone, use the
separate `osc7501-parser` crate.

```rust
use osc7501::parser::{Event, parse_sequence};

let input = b"\x1b]7501;state=done:app=cargo:msg=T0s=\x1b\\";
if let Some(Event::Report(report)) = parse_sequence(input).expect("valid frame") {
    let mut storage = [0_u8; 2048];
    let message = report.message().expect("message present")
        .decode_into(&mut storage).expect("enough space");
    assert_eq!(message, "OK");
}
```

Use `parse_sequence` for a complete OSC sequence. If your terminal code already
handles escape sequences, use `parse_body`. `Decoder::feed` handles input split
across reads.

Messages and titles stay encoded so you can decode only the text you need.
The parser checks their contents before it returns the report.

The parser rejects the whole report if text is invalid or a protocol limit is
exceeded. Otherwise, it ignores reports with an unknown or missing state, or an
invalid record id. Invalid app ids and progress values are treated as absent.

## Answer support queries

Terminal hosts can use `CapabilityReply::new().states(&states).kinds(&kinds)`
and `write_to` with their program-input writer. Advertise only features the
host implements. Omitted lists use the fixed revision 0.4 defaults; empty
lists advertise no support. The default reply has the same bytes as a query,
so stream direction and pending-query state determine its meaning.

Replies are encoded within the 4096-byte frame cap before caller output. They
use no heap allocation and leave flushing to the caller, as status reports do.
See the [producer API overview](crates/osc7501/README.md) for an example.

## Keep terminal records

Enable the optional `store` feature to use `osc7501::store` alongside the
producer and parser:

```toml
osc7501 = { package = "osc7501-rs", version = "0.1", features = ["store"] }
```

The feature is off by default. For a host without the producer, depend on
`osc7501-store` directly and use its `parser` re-export. Both paths retain
validated reports from `parse_body`, `parse_sequence`, or a `Decoder::feed`
callback. The store borrows fixed slots, copies and decodes fields before
mutation, and exposes read-only record views.
It implements replacement, app inheritance, subtree clearing, oldest-update
eviction, and explicit terminal lifecycle cleanup.

The caller owns transport, history, and display policy. See the
[store API overview](crates/osc7501-store/README.md) for storage and lifecycle
contracts.

Run the [terminal host example](crates/osc7501/examples/host.rs) to see
the three crates work together:

```sh
cargo run -p osc7501-rs --example host --features store
```

This extends the producer TUI's release playground with a terminal host. The
same test, documentation, and package tasks send encoded reports through a
fragmented stream. The task cards and inspector read retained store records.

Use the same controls: `s` starts, `f` fails the selected task, `r` retries,
`p`/`a`/`?` blocks, `Enter` resumes, and `c` cancels. `b` selects the root or a
child; `i` switches between decoded fields and the store API. Finish a release
and press `X` to drop the simulated producer. Its results stay visible.
`P` simulates a shell prompt, `S` a soft reset, and `R` a full reset. See the
[host controls](crates/osc7501-store/README.md#run-a-terminal-host-example).

All events are simulated; no terminal protocol support is needed. Use at least
80 columns and 24 rows. Piped output stays plain; `-- --plain` requests a text
trace in a terminal.

## Develop

All crates support Rust 1.88 and later. CI runs strict Clippy, nextest, and
doctests with default and all features on Rust 1.88 and current stable on Linux,
macOS, and Windows. The parser and store are also checked on
`thumbv7em-none-eabihf`, a target without `std`, with Rust 1.88 and current stable.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-targets --all-features --locked
cargo test --workspace --doc --all-features --locked
cargo deny --all-features --locked check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo +1.88.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.88.0 nextest run --workspace --all-targets --locked
cargo +1.88.0 test --workspace --doc --locked
cargo +1.88.0 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.88.0 nextest run --workspace --all-targets --all-features --locked
cargo +1.88.0 test --workspace --doc --all-features --locked
cargo check -p osc7501-parser -p osc7501-store --lib --target thumbv7em-none-eabihf --locked
```

The checks use [cargo-nextest](https://nexte.st/docs/installation/pre-built-binaries/)
and [cargo-deny](https://embarkstudios.github.io/cargo-deny/).

- [Protocol notes](docs/protocol.md)
- [Design decisions](docs/design.md)
- [Performance measurements](docs/performance.md)
- [Release procedure](docs/releasing.md)
- [Protocol source](https://www.superlogical.com/rex/docs/build/program-status)
