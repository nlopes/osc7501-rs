# osc7501-store

Keep the current status of programs and tasks for a terminal panel or tab
indicator. This crate implements record storage for
[OSC 7501](https://www.superlogical.com/rex/docs/build/program-status), revision 0.4.

Works without `std` or an allocator. You provide the storage.

## Keep a report

```toml
[dependencies]
osc7501-store = "0.1"
```

The crate exposes its parser, so you do not need a separate parser dependency:

```rust
use osc7501_store::{
    MIN_HOST_CAPACITY, Slot, Store,
    parser::{Event, parse_sequence},
};

fn main() -> Result<(), osc7501_store::Error> {
    let mut slots = [const { Slot::EMPTY }; MIN_HOST_CAPACITY];
    let mut store = Store::new(&mut slots)?;
    let input = b"\x1b]7501;state=done:app=cargo:msg=T0s=\x1b\\";
    if let Some(Event::Report(report)) = parse_sequence(input)? {
        store.apply(report, |_| {})?;
    }
    if let Some(record) = store.get(None) {
        assert_eq!(record.message(), Some("OK"));
    }
    Ok(())
}
```

If you already use the producer, enable its `store` feature and import
`osc7501::store` instead.

## How records change

Each report replaces every explicit field. Omitted fields disappear.
The store copies and decodes all fields before changing records, so it can
retain reports after the parser's input is gone. Record views borrow the store.

- Clearing a task removes it and its slash-delimited descendants.
  Clearing root removes everything.
- `effective_app` finds the nearest app on the record or its ancestors,
  even when intermediate parents are absent.
- At capacity, a new record evicts the least recently updated record.
  Reads do not change that order; updating an existing record does not evict.
- Removal callbacks receive the old record before clearing, eviction, or
  lifecycle cleanup. Ordinary replacement does not call them.
  Use `|_| {}` if you do not need that information.

`Store::new` starts empty, even with reused slots. It accepts 1 to 256 slots,
including root. A conforming terminal host needs at least `MIN_HOST_CAPACITY`
(64); smaller stores are useful for tests. Choose storage that fits your
memory budget, especially on small stacks. See the
[storage measurements](https://github.com/nlopes/osc7501-rs/blob/main/docs/performance.md#record-store-memory)
for sizes and allocation checks.

## Handle terminal events

Your shell integration and escape-sequence handler supply these events.
The store does not infer them from reports or record ids.

| Event | What to do | Records kept |
| --- | --- | --- |
| New shell prompt | Call `on_prompt` | Idle, done, error |
| Attached process exits | Call `on_process_exit` | Idle, done, error |
| Full reset (RIS) | Call `on_full_reset` | None |
| Soft reset (DECSTR) or screen switch | No action | All |

One store represents one terminal. You own history, display, notifications,
and transport. Treat decoded text as plain, untrusted text.

## Run a terminal host example

From a repository checkout, run the
[host playground](https://github.com/nlopes/osc7501-rs/blob/main/crates/osc7501/examples/host.rs):

```sh
cargo run -p osc7501-rs --example host --features store
```

It simulates a release, sends reports through a fragmented stream, and shows
the records the host keeps. Try this:

1. Press `s` to start. Select a task with the arrow keys or `Tab`;
   `f` fails it and `r` retries.
2. Press `i` to switch between decoded fields and the store API.
3. Finish the release, then press `X` to simulate process exit.
   Completed and failed results remain visible. Press `R` to clear everything.
4. Press `q` to quit. The on-screen help lists the other controls.

Use a terminal of at least 80 columns and 24 rows. Protocol support is optional.
Unset `NO_COLOR` for colors. Piped output is plain; add `-- --plain` to request
a text trace in a terminal.

See the [API documentation](https://docs.rs/osc7501-store) for the full interface.
