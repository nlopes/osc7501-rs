# osc7501-store

Keep the current status of programs and tasks for a terminal panel or tab
indicator. This crate implements record storage for
[OSC 7501](https://www.superlogical.com/rex/docs/build/program-status), revision 0.4.

Works without `std`. Enable `alloc` to let the store own its storage, or provide
storage yourself to work without an allocator. The default feature set is empty.

## Keep a report

```toml
[dependencies]
osc7501-store = { version = "0.1", features = ["alloc"] }
```

The crate exposes its parser, so you do not need a separate parser dependency:

```rust
use osc7501_store::{
    MIN_HOST_CAPACITY, Store,
    parser::{Event, parse_sequence},
};

# #[cfg(feature = "alloc")]
fn main() -> Result<(), osc7501_store::Error> {
    let mut store = Store::new(MIN_HOST_CAPACITY)?;
    let input = b"\x1b]7501;state=done:app=cargo:msg=T0s=\x1b\\";
    if let Some(Event::Report(report)) = parse_sequence(input)? {
        store.apply(report)?;
    }
    if let Some(record) = store.get(None) {
        assert_eq!(record.message(), Some("OK"));
    }
    Ok(())
}
# #[cfg(not(feature = "alloc"))]
# fn main() {}
```

`Store::new(capacity)` allocates storage once. Store operations do not allocate
or grow that storage. The store frees it when dropped.

If you already use the producer, enable its `store-alloc` feature and import
`osc7501::store` instead. Its `store` feature exposes caller-provided storage
without enabling `alloc`.

## Provide storage without an allocator

Use the default features and supply slots with `Store::from_slots`:

```rust
use osc7501_store::{MIN_HOST_CAPACITY, Slot, Store};

# fn main() -> Result<(), osc7501_store::Error> {
let mut slots = [const { Slot::EMPTY }; MIN_HOST_CAPACITY];
let mut store = Store::from_slots(&mut slots)?;
# Ok(())
# }
```

This constructor is also available with `alloc` enabled. It starts empty,
even with reused slots. Choose storage that fits your memory budget,
especially on small stacks.
See the [storage measurements](https://github.com/nlopes/osc7501-rs/blob/main/docs/performance.md#record-store-memory)
for sizes and allocation checks.

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
  Every set refreshes update order, even when its fields are unchanged.
- `apply` and the plain lifecycle methods update the store without collecting
  notifications. Use their `_with_changes` variants to observe changes.

Equal current records do not imply equal future behavior: update order also
determines eviction. Preserve report order when replaying input. Removing
unchanged updates or restoring records in an arbitrary order can change which
record is evicted next.

Both constructors accept capacities from 1 to 256 records, including root.
A conforming terminal host needs at least `MIN_HOST_CAPACITY` (64); smaller
stores are useful for tests.

## Observe record changes

Use `apply_with_changes` to keep a UI or a history of visible record states in
sync without copying and comparing the whole store. These notifications describe
changes to current records and resolved apps:

- An unchanged report emits no notification, but still refreshes eviction order.
- A child can receive a set notification because its ancestor's app changed,
  even when the child sent no report.

Keep the incoming reports and lifecycle events when you need every update or
must reproduce the store's eviction order. Change notifications alone cannot
restore that history.

The callback receives:

- `Change::Remove(record)` before a record is removed.
- `Change::Set { record, effective_app }` for each new record, changed explicit
  field, or changed inherited app. Each surviving record is delivered at most
  once, after the complete operation.

All removals precede sets. Order within either group is unspecified. Effective
apps reflect the final store, including ancestor insertion, replacement, and
eviction. An unchanged app value produces no descendant notification, even if
the ancestor supplying it changes.

This example maintains a view of states and resolved apps. Copy other fields
in the callback if your application needs them:

```rust
use std::collections::BTreeMap;
use osc7501_store::{Change, Slot, Store, parser::{Event, parse_body}};

# fn main() -> Result<(), osc7501_store::Error> {
let mut slots = [const { Slot::EMPTY }; 2];
let mut store = Store::from_slots(&mut slots)?;
let mut view = BTreeMap::new();
let mut update = |change: Change<'_>| match change {
    Change::Set { record, effective_app } => {
        let id = record.id().map(|id| id.as_str().to_owned());
        let app = effective_app.map(|app| app.as_str().to_owned());
        view.insert(id, (record.state(), app));
    }
    Change::Remove(record) => {
        let id = record.id().map(|id| id.as_str().to_owned());
        view.remove(&id);
    }
    _ => {}
};
for body in [b"state=done:id=build/test".as_slice(), b"state=working:app=cargo"] {
    if let Some(Event::Report(report)) = parse_body(body)? {
        store.apply_with_changes(report, &mut update)?;
    }
}
store.on_prompt_with_changes(&mut update);
# assert_eq!(view.len(), 1);
# assert_eq!(view.get(&Some("build/test".into())), Some(&(osc7501_store::parser::State::Done, None)));
# Ok(())
# }
```

The child inherits `cargo` when the root arrives. Prompt cleanup removes the
working root and updates the surviving child to have no effective app.
Use the same callback for every operation that affects an observed view,
including `on_prompt_with_changes`, `on_process_exit_with_changes`, and
`on_full_reset_with_changes`. Calling a plain method changes the store without
updating that view. If observation starts on a populated store, first initialize
the view from `iter()` and `effective_app()`.

The runnable [observer example](https://github.com/nlopes/osc7501-rs/blob/main/crates/osc7501-store/examples/observe.rs)
routes reports and lifecycle events through one handler. It copies selected
fields into an application-owned view, which remains usable after the store is
dropped:

```sh
cargo run -p osc7501-store --example observe --features alloc
```

Use `apply(report)`, `on_prompt()`, `on_process_exit()`, and `on_full_reset()`
when you do not need notifications; they avoid change detection.

Callbacks borrow records only for the call. Store operations remain allocation-free;
the map above allocates because it is application-owned. Change detection uses
bounded scratch storage for effective apps, and compares decoded field values.

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
cargo run -p osc7501-rs --example host --features store-alloc
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
