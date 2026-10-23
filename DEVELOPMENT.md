# Development

The three crates share one workspace and test setup. Run the commands below
from the repository root. The checkout uses stable Rust; the minimum supported
version is Rust 1.88.

## Run the tests

Install [cargo-nextest](https://nexte.st/docs/installation/pre-built-binaries/)
to run the tests. Run documentation examples separately, because nextest does
not run doctests:

```sh
cargo nextest run --workspace --all-targets --all-features --locked
cargo test --workspace --doc --all-features --locked
```

`--all-features` includes the producer's optional store integration and owned
store storage. Omit it to check the default features too.

To focus on one crate, replace `--workspace` with `-p osc7501-parser`,
`-p osc7501-store`, or `-p osc7501-rs`. Keep `--all-targets` on nextest runs
to include example tests, including the parser's terminal-stream adapter.

### Property tests

[Hegel](https://hegel.dev/) checks parser text validation against the `base64`
crate, framing and streaming consistency, byte-preserving terminal forwarding,
producer round trips and writes, and store histories against an independent
model. Hegel and its static engine are development dependencies.

[hegel.toml](hegel.toml) sets 256 cases per property locally and 512 in CI,
with deterministic generation and no failure database. For a longer parser
run, select the extended profile:

```sh
HEGEL_DEFAULT_PROFILE=extended cargo nextest run -p osc7501-parser --all-targets --locked
```

This runs 10,000 cases per property. Use `--workspace --all-features` in place
of `-p osc7501-parser` to extend the producer and store tests too.
Set `HEGEL_SEED` to explore another seed. Hegel shrinks failures and prints a
`#[hegel::reproduce_failure("...")]` attribute; add it below the failing test's
`#[hegel::test]` to replay the case. Keep a small regression test when fixing
a defect.

## Check a change

Run the tests above, then check formatting and Clippy before opening a pull
request:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

[CI](.github/workflows/ci.yml) defines the full check matrix: default and all
features on Rust 1.88 and stable across Linux, macOS, and Windows. It also
checks rustdoc, dependencies, and the parser and store without `std` on
`thumbv7em-none-eabihf`.

Crate READMEs also serve as rustdoc. After changing them, run the doctests and
build the documentation with warnings treated as errors:

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

For allocation and bounded-stack checks, see the
[performance guide](docs/performance.md#recheck-resource-use).
For packaging and publishing, follow the [release guide](docs/releasing.md).

## Examples

When changing an interactive example, run it in a terminal too: tests cannot
tell you whether the screen is useful. Both playgrounds need at least 80
columns and 24 rows. Unset `NO_COLOR` for colors. Protocol support is optional;
a supporting terminal can also display their reports.

### Release playground

The [producer TUI](crates/osc7501/examples/tui.rs) simulates a release with three
tasks, approval prompts, failures, and retries. The screen shows reports and
the builder calls that produce them:

```sh
cargo run -p osc7501-rs --example tui
```

Press `s` to start and `q` to quit. The on-screen help lists the other controls.

### Terminal host

The [host playground](crates/osc7501/examples/host.rs) parses fragmented output
and shows the records the host keeps after updates and process exit:

```sh
cargo run -p osc7501-rs --example host --features store-alloc
```

Press `s` to start. Select a task with the arrow keys or `Tab`; `f` fails it
and `r` retries. Press `i` to switch between decoded fields and the store API.
After the release, press `X` to simulate process exit. Completed and failed
results remain visible. Press `R` to clear everything and `q` to quit.

Piped output is plain. Add `-- --plain` to request a text trace in a terminal.

### Text demo

The [text demo](crates/osc7501/examples/demo.rs) runs a short, paced release
simulation. Pipe its output for a plain trace without pauses:

```sh
cargo run -p osc7501-rs --example demo
```

### Terminal stream

The [terminal-stream example](crates/osc7501-parser/examples/terminal_stream.rs)
forwards stdin unchanged to stdout and reports OSC 7501 observations on stderr:

```sh
cargo run -p osc7501-parser --example terminal_stream --quiet < recording.bin > forwarded.bin
```

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
library keeps its allocation-free `no_std` API.

### Record observer

The [observer example](crates/osc7501-store/examples/observe.rs) routes reports
and lifecycle events through one handler. It copies selected fields into an
application-owned view that remains usable after the store is dropped:

```sh
cargo run -p osc7501-store --example observe --features alloc
```
