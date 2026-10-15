# osc7501-rs

Let your terminal know what your program is doing: working, waiting for input,
done, or failed.

These Rust libraries implement the
[OSC 7501 Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status),
revision 0.4. Terminals that support it decide how to display that status.

## Send a status

Add the producer to your `Cargo.toml`:

```toml
[dependencies]
osc7501 = { package = "osc7501-rs", version = "0.1" }
```

Then send a report when your program needs attention:

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

In a TUI, use the same writer as your screen rendering so reports and screen
updates stay in order. You control flushing. Invalid reports are rejected
before any bytes are sent.

Each report replaces all fields of its record, so repeat fields you want to
keep. Send `idle` after cancellation, or `done` or `error` when work ends.
The [producer guide](crates/osc7501/README.md) covers child tasks, progress,
and terminal capability replies.

## Choose a crate

I keep sending, parsing, and storage separate so you can use just the parts
you need.

| Crate | Use it to |
| --- | --- |
| [osc7501-rs](crates/osc7501/README.md) | Send status reports and terminal capability replies |
| [osc7501-parser](crates/osc7501-parser/README.md) | Parse reports and replies, including input split across reads |
| [osc7501-store](crates/osc7501-store/README.md) | Keep current records for a terminal status display |

The producer exposes the parser as `osc7501::parser`. Add `features = ["store"]`
to the dependency above to also use `osc7501::store`.

The parser and store work without `std`. All three crates process reports
without heap allocation; the store uses storage you provide.

## Try the examples

From a checkout, I'd start with the interactive TUI:

```sh
cargo run -p osc7501-rs --example tui
```

It simulates a release with three tasks, approval prompts, failures, and
retries. You can see the reports and the builder calls that produce them.
Press `s` to start and `q` to quit; the on-screen help lists the other controls.
Use a terminal of at least 80 columns and 24 rows. Unset `NO_COLOR` for colors.

The TUI sends real OSC 7501 reports. You can use the playground without
protocol support, but a supporting terminal can also display those reports.

Two more examples cover different uses:

- [Terminal host](crates/osc7501-store/README.md#run-a-terminal-host-example):
  run `cargo run -p osc7501-rs --example host --features store` to see reports
  parsed and stored. The inspector shows what the host keeps after updates
  and process exit. Add `-- --plain` for a text trace.
- [Text demo](crates/osc7501/examples/demo.rs):
  run `cargo run -p osc7501-rs --example demo` for a short, paced release
  simulation. Pipe its output for a plain trace without pauses.

## Working on the code

The checkout uses stable Rust. Install
[cargo-nextest](https://nexte.st/docs/installation/pre-built-binaries/)
to run the tests. From the repository root, run the workspace tests and the
documentation examples:

```sh
cargo nextest run --workspace --all-targets --all-features --locked
cargo test --workspace --doc --all-features --locked
```

`--all-features` includes the optional store integration. When changing an
interactive example, run it in a terminal too: tests cannot tell you whether
the screen is useful.

Before opening a pull request, check formatting and Clippy:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

[CI](.github/workflows/ci.yml) defines the full check matrix: default and all
features on Rust 1.88 and stable across Linux, macOS, and Windows. It also
checks documentation, dependencies, and the parser and store without `std`.

For more context, see the [protocol notes](docs/protocol.md),
[design decisions](docs/design.md), [performance measurements](docs/performance.md),
and [release procedure](docs/releasing.md).
