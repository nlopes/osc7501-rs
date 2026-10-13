# osc7501-rs

Compact Rust libraries for **OSC 7501 Program Status Protocol, revision 0.4**.
Latest stable Rust, edition 2024. All workspace crates live in `crates/`.

| Crate | What it does |
| --- | --- |
| `osc7501-parser` | `no_std` parsing with borrowed results and bounded streaming |
| `osc7501` | Typed status emission through your application's existing writer |

## Send from a TUI

```rust
use osc7501::{AppId, Kind, State, Status};

fn awaiting_approval(output: &mut impl std::io::Write) -> Result<(), osc7501::Error> {
    let app = AppId::new("my-tui").expect("constant valid app id");
    Status::new(State::Blocked)
        .app(app)
        .kind(Kind::Permission)
        .message("Apply these changes?")
        .write_to(output)
}
```

Use the same writer/lock as screen rendering, and flush at your chosen boundary.
Each update completely replaces its record, so repeat fields you want to keep.
Report idle after cancellation, and done/error when work finishes. Child tasks
use validated slash-separated `RecordId`s; clear removes a whole subtree.

## Run the protocol demo

```sh
cargo run -p osc7501-rs --example demo
```

The [demo](crates/osc7501/examples/demo.rs) simulates a release in about 15 seconds.
It shows all five states, all three blocking reasons, unknown and percentage
progress, UTF-8 titles, and two child tasks that inherit the root's app.
One task fails and retries. Clearing `build` removes both child records; the
final root report remains `done`.

Each update is flushed before a one-second pause. A terminal with OSC 7501
support can display the status. The printed trace also explains each step.
Redirected output contains only the trace and runs without pauses. The demo
does not read input or query terminal capabilities; all prompts are simulated.

## Parse without allocation

```rust
use osc7501_parser::{Event, parse_sequence};

let input = b"\x1b]7501;state=done:app=cargo:msg=T0s=\x1b\\";
if let Some(Event::Report(report)) = parse_sequence(input).expect("valid frame") {
    let mut storage = [0_u8; 2048];
    let message = report.message.expect("message present")
        .decode_into(&mut storage).expect("enough space");
    assert_eq!(message, "OK");
}
```

Use `parse_body` from an existing OSC dispatcher. Use `Decoder::feed` for
arbitrary byte chunks; callback data borrows its 4096-byte inline buffer.
Missing/unknown state and invalid ids return `Ok(None)`. Hard-limit violations,
invalid base64/UTF-8, and decoded controls reject the entire report.

Parsing, streaming, and text decoding use `osc7501_parser::Error` for failures.
Both crates inherit `thiserror` from the workspace with default features disabled,
preserving the parser's `no_std` support.

## Resource target

**Zero heap allocations** for parsing, streaming, text decoding, and bounded
producer emission. Measured x86-64 sizes: decoder **4112 bytes**, report
**88 bytes**. Validation needs one 2048-byte decode scratch buffer.
This satisfies a sub-1-MB parser resource target; it does not constrain the
Rust runtime or the host TUI's total RSS. See the measurements and limits below.

## Develop

```sh
rustup update stable
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings -D clippy::todo
cargo nextest run --workspace --locked
cargo deny --all-features --locked check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo run --release -p osc7501-parser --example benchmark
```

Install nextest from [its official instructions](https://nexte.st/docs/installation/pre-built-binaries/).
Install [cargo-deny](https://embarkstudios.github.io/cargo-deny/) to check dependency advisories, licenses, bans, and sources with `deny.toml`.
Pedantic Clippy is enabled across the workspace; `clippy::todo` is denied.
Unsafe code is forbidden. Oracle and allocation dependencies are development-only.
Publishing is disabled while the private prototype is reviewed.

- [Specification extraction, every section](docs/protocol.md)
- [API and memory design](docs/design.md)
- [Measured performance and verification](docs/performance.md)
- [Protocol source](https://www.superlogical.com/rex/docs/build/program-status)
