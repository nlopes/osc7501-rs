# osc7501-rs

Rust libraries for **OSC 7501 Program Status Protocol, revision 0.4**. A program
can tell its terminal that it is idle, working, waiting for the user, done, or
has failed. The terminal decides how to display that status.

| Crate | What it does |
| --- | --- |
| `osc7501` | Send status reports from a terminal application |
| `osc7501-parser` | Parse status reports and terminal capability replies |

The parser can be used on its own and works without `std`. Both crates process
reports without heap allocation.

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

## Develop

Both crates support Rust 1.88 and later. CI runs strict Clippy, nextest, and
doctests with Rust 1.88 and current stable on Linux, macOS, and Windows. The
parser is also checked on `thumbv7em-none-eabihf`, a target without `std`, with
Rust 1.88 and current stable.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
cargo test --workspace --doc --locked
cargo deny --all-features --locked check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo +1.88.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.88.0 nextest run --workspace --all-targets --locked
cargo +1.88.0 test --workspace --doc --locked
cargo check -p osc7501-parser --lib --target thumbv7em-none-eabihf --locked
```

The checks use [cargo-nextest](https://nexte.st/docs/installation/pre-built-binaries/)
and [cargo-deny](https://embarkstudios.github.io/cargo-deny/).

- [Protocol notes](docs/protocol.md)
- [Design decisions](docs/design.md)
- [Performance measurements](docs/performance.md)
- [Protocol source](https://www.superlogical.com/rex/docs/build/program-status)
