# osc7501-store

Keep current **OSC 7501 Program Status Protocol, revision 0.4** records for one
terminal. This opt-in crate works without `std` or an allocator. It depends on
`osc7501-parser` and has no producer dependency. The producer's optional
`store` feature also exposes this crate as `osc7501::store`.

Use it for a terminal status panel, tab indicator, or test host that needs
current records. Applications that only send reports need the producer.
Consumers that process each event once, or already maintain their own record
model, can use the parser alone.

```toml
[dependencies]
osc7501-store = "0.1"
```

`osc7501_store::parser` exposes the parser and its types, so no separate parser
dependency is needed.

## Retain a report

```rust
use osc7501_store::{MIN_HOST_CAPACITY, Slot, Store, parser::{Event, parse_sequence}};

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

The caller provides fixed slots and chooses where they live. `Store::new`
starts empty, including when the slots were used by a previous store. Capacity
is 1 through 256, including root. A conforming terminal host needs at least
`MIN_HOST_CAPACITY` slots; smaller stores can serve tests.

Each report replaces every explicit field, so omitted fields disappear.
Text is copied and decoded before any record changes. Read-only `Record` views
borrow the store. `effective_app` resolves the nearest ancestor with an app
at lookup time, even when intermediate parents are absent.

Clear removes the addressed record and slash-delimited descendants. Root clear
removes every record. At capacity, a new record evicts the least recently updated
record. Reads do not change that order. Existing-record updates do not evict
another record. Update ordering uses bounded ranks and cannot wrap.

`apply` calls the supplied callback for each cleared or evicted record before
removal. It does not call the callback for ordinary replacement. Removal views
contain the old fields and cannot be retained across a store mutation. Use
`|_| {}` when the caller does not need removal information.

## Apply terminal lifecycle events

`on_prompt` and `on_process_exit` remove working and blocked records. They retain
idle, done, and error. `on_full_reset` clears all records. Cleanup methods call
the supplied callback before each removal. Soft resets and screen switches
need no store action. Events apply to the terminal represented by this store;
the library does not infer process identity from record ids.

The store holds current records. The caller owns history, display, notifications,
and transport. Decoded text remains plain, untrusted display data. Validation
does not make it markup or prove its source identity.

## Run a terminal host example

From the repository root, run:

```sh
cargo run -p osc7501-rs --example host --features store
```

The [host example](https://github.com/nlopes/osc7501-rs/blob/main/crates/osc7501/examples/host.rs)
extends the producer's
[release playground](https://github.com/nlopes/osc7501-rs/blob/main/crates/osc7501/examples/tui.rs).
It keeps the same three tasks, report ids, progress rates, prompts, and controls.
The task cards now read the host's retained records. The inspector shows decoded
fields, inherited app names, and the store API.

Try these steps:

1. Press `s` to start tests, documentation, and packaging.
2. Press `p`, `a`, or `?` to block for permission, authentication, or an answer.
   `Enter` resumes. The stored root loses its old blocked kind on replacement.
3. Select a task with the arrow keys or `Tab`. Press `f` to fail it and `r` to
   retry. `b` switches the inspector between root and child; `i` shows the API.
4. Complete the release, then press `X` to simulate process exit. The producer
   and its task model are dropped. Completed and failed results remain visible.
   Exiting during work removes working and blocked records.
5. Press `s` for a new release. `c` or `Esc` cancels and clears the `release`
   subtree. The separate root becomes idle.

Host controls use uppercase letters: `P` starts a shell prompt, `X` simulates
process exit, `S` applies a soft reset, and `R` applies a full reset. Soft reset
preserves records. Full reset clears them; a running producer can send new
reports afterwards. `q` or `Ctrl+C` quits the example.

Use a terminal of at least 80 columns and 24 rows. At 30 rows, the host event
log also appears. Unset `NO_COLOR` to enable colors. Protocol support is not
needed. Piped output stays plain. To request a plain trace in a terminal, run:

```sh
cargo run -p osc7501-rs --example host --features store -- --plain
```

The producer writes OSC reports into an in-memory stream mixed with ordinary
output. `Host::receive` feeds seven-byte fragments to `Decoder::feed`, applies
accepted reports with `Store::apply`, and answers queries through a separate
program-input writer. Each output buffer is dropped before the next redraw.
The task cards and inspector read `Store::get` and `effective_app`, including
after the producer is dropped. The caller owns the event log.

The producer simulation is in `host/program.rs`; `host/scenario.rs` connects
its output to the host. The example belongs to the producer package and uses
its `store` feature. It allocates caller slots on the heap. Store operations
still need no allocator, and the store has no TUI dependencies.

In a real host, pass program output to the terminal renderer as well as the
status decoder. The terminal's shell integration calls `on_prompt` or
`on_process_exit`. Its escape-sequence handler calls `on_full_reset` for Reset
to Initial State (RIS). A DEC Soft Terminal Reset (DECSTR) leaves the store
untouched. The example supplies lifecycle events explicitly; `Decoder` only
observes OSC 7501.

Rust 1.88 or later is required.

## Storage cost

On macOS aarch64, one slot occupies 2480 bytes and 256 slots occupy 634880 bytes.
Store metadata, decoder storage, and conservative preparation and validation
buffers total 643544 bytes. The maximum-field workload completes on a thread
configured with a 64 KiB stack, with caller slots stored outside that stack.
These are library resource checks, not whole-process RSS or exact stack peaks.
Large slot arrays need storage chosen by the caller; construction does not
create a hidden capacity-sized array.

- [API documentation](https://docs.rs/osc7501-store)
- [Protocol specification](https://www.superlogical.com/rex/docs/build/program-status)
