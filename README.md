# osc7501-rs

Let your terminal know what your program is doing: working, waiting for input,
done, or failed.

These Rust libraries implement the
[OSC 7501 Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status),
revision 0.4. Terminals that support it decide how to display that status.

> [!IMPORTANT]
> This was all built using Codex with a mix of the Sol 6.1 and Astra models in xhigh.
>
> The motivation to build these libraries was 3-fold:
> 1. have a pure rust implementation
> 2. build an experiment into [`acdc`](https://github.com/nlopes/acdc)'s html and terminal converters
> 3. learn osc7501 on the way
>
> I've steered, read the code, accepted some "fine" code, trashed some "horrible" code, and also decided to build an extra crate [`osc7501-store`](./crates/osc7501-store) which might end up not being very useful.

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
keep. Use `Context` to repeat app, id, and title across fresh status reports.
Send `idle` after cancellation, or `done` or `error` when work ends.
The [producer guide](crates/osc7501/README.md) covers reusable contexts, child
tasks, progress, and terminal capability replies.

## Choose a crate

I keep sending, parsing, and storage separate so you can use just the parts
you need.

| Crate | Use it to |
| --- | --- |
| [osc7501-rs](crates/osc7501/README.md) | Send status reports and terminal capability replies |
| [osc7501-parser](crates/osc7501-parser/README.md) | Parse reports and replies, including input split across reads |
| [osc7501-store](crates/osc7501-store/README.md) | Keep current records for a terminal status display |

The producer exposes the parser as `osc7501::parser`. Add `features = ["store"]`
to the dependency above to also use `osc7501::store`. Use `features = ["store-alloc"]`
to construct an owned store with `Store::new(capacity)`.

The parser and store work without `std`. All three crates process reports
without heap allocation. The store can borrow storage you provide or allocate
its own storage during construction with the optional `alloc` feature.

## Try the examples

| Example | What it shows |
| --- | --- |
| [Release playground](crates/osc7501/examples/tui.rs) | Task progress, approval prompts, failures, and retries |
| [Terminal host](crates/osc7501/examples/host.rs) | Reports parsed and stored for a terminal display |
| [Text demo](crates/osc7501/examples/demo.rs) | A short release simulation with plain output when piped |
| [Terminal stream](crates/osc7501-parser/examples/terminal_stream.rs) | Observe reports while forwarding the original bytes |
| [Record observer](crates/osc7501-store/examples/observe.rs) | Keep an owned view in sync with record changes |

For more context, see the [protocol notes](docs/protocol.md),
[design decisions](docs/design.md), and [performance measurements](docs/performance.md).
Run instructions and contributor checks are in [DEVELOPMENT.md](DEVELOPMENT.md).
