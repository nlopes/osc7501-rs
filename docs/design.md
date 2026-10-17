# Design decisions

I keep sending, parsing, and storage separate so applications can use only
what they need. The [crate guides](../README.md#choose-a-crate) cover usage;
these notes explain the choices behind the API.

## Three crates, separate responsibilities

| Package | Responsibility | Runtime dependencies |
| --- | --- | --- |
| `osc7501-parser` | Parsing, validation, protocol types, streaming | `thiserror` without `std` |
| `osc7501-rs` | Sending reports and capability replies | Parser, `thiserror`, optional store |
| `osc7501-store` | Current records and terminal cleanup | Parser, `thiserror` without `std` |

The producer's Rust library is `osc7501`. It exposes `osc7501::parser` and,
with the optional `store` feature, `osc7501::store`. The parser and store
both work without `std`.

The host example belongs to the producer package and requires `store-alloc`.
This keeps the store independent of the producer and its TUI dependencies.
CI checks default and all features; docs.rs builds all features.

## Validate before exposing data

The parser distinguishes accepted events, ignored reports, and errors.
It checks the complete report before returning it. `Report` and
`Capabilities` have read-only fields so callers cannot bypass validation.

`AppId`, `RecordId`, and `Progress` validate values at construction.
The producer rejects invalid combinations before writing. Incoming reports
follow the protocol's more tolerant rules: invalid app ids and progress are
absent, while an invalid record id discards the report. Hard limits still
apply. See the [protocol notes](protocol.md) for the exact rules.

`Action::Clear` is separate from `State` because clearing removes records.
It is not a status to retain or advertise as a capability. Reports replace
all explicit fields; the library imposes no workflow restrictions on retries
or id reuse.

`Status` keeps a uniform builder because callers select states at runtime.
Kind and progress combinations are checked before writing. `State` remains a
plain capability value; it does not carry report fields.

## Borrow until storage is needed

Parsed reports borrow their input. Messages and titles remain base64 encoded,
with their text validated before use. Callers decode into their own buffers
when they need to display text. Streaming callbacks borrow the decoder's
buffer only for the duration of the callback.

`Decoder::try_feed` lets a consumer stop with its own error or stop value.
The wrapper keeps its fields private. `consumed()` includes the byte that
triggered the callback. `get_ref()` borrows the original value and `into_inner()`
returns it by value. Standard error chaining is available when the callback
error implements `core::error::Error + 'static`; feeding and recovery do not
require that bound. The delivered event stays consumed. The caller resumes
with the untouched suffix on the same decoder. Overflow draining also survives
a stop. Parser errors remain callback inputs, so each consumer decides whether
to continue after a rejected frame. `feed` uses the same scanner with a callback
that always continues.

Full terminal context belongs at the host boundary. The parser's supported
`terminal_stream` example combines `vte` dispatch with bounded retention of the
original bytes. It checks the actual terminator because a VT OSC dispatch can
also mean cancellation, and callback parameters need not retain every original
byte. Segment callbacks preserve unrelated output and allow consumer-directed
forwarding. This integration stays outside the allocation-free parsing core;
`vte` is a development dependency only.

The store is the point where data becomes independent of the input. It copies
identifiers and decodes text before changing any record. This keeps a failed
update from removing an old record or evicting another one.

`Store::from_slots` borrows caller-owned slots and starts empty without an
allocator. With the optional `alloc` feature, `Store::new(capacity)` allocates
and owns its slots. Both use the same record operations; neither creates a
capacity-sized stack array inside its constructor. Owned storage never grows
and is freed when the store is dropped.

Capacity is 1 to 256 records, including root. A conforming terminal host needs
at least 64. The producer's `store-alloc` feature enables `store` and forwards
the store's `alloc` feature. Default builds remain allocator-free.

## Let the application own terminal I/O

Report and reply writers accept concrete writers and `&mut dyn Write`.
They encode a complete frame before writing and leave flushing to the caller.
An I/O failure can still leave a partial frame.

Use the same writer as screen rendering to keep output in order. The
application owns input reading, locking, TTY detection, timeouts, signals,
and shutdown. Capability queries belong in its existing input loop.

I leave cleanup explicit because clearing on drop could erase a result the
user still needs. Send idle after cancellation and done or error when work
ends. For concurrent tasks, give each task an id and choose one owner for
root updates.

## Keep capability replies explicit

`CapabilityReply` borrows typed state and kind slices. Each list has three
meanings:

| List | Meaning |
| --- | --- |
| Omitted | Use the fixed revision 0.4 defaults |
| Empty | Support none of that category |
| Populated | Advertise the supplied known values |

Typed slices match the status API and prevent unknown names from being sent.
They preserve order and duplicates without allocating a set. The parser still
accepts replies containing future names and skips those names.

The caller chooses supported capabilities and when to reply. A bare reply
and a query have identical bytes, so stream direction and pending-query state
must determine their meaning.

## Keep storage bounded

The store resolves inherited app ids at lookup time. Ancestor updates
therefore affect children without rewriting their fields, even when an
intermediate parent is absent.

At capacity, a new record evicts the least recently updated record. Existing
record updates do not evict. Bounded ranks track update order without a
counter that can wrap. At the 256-record limit, bounded scans keep the code
simple; removing several records can require several scans.

Removal callbacks borrow the old record before clearing, eviction, or terminal
cleanup. Ordinary replacement does not call them. The host can record removals
without the store allocating a change list.

Prompt and process-exit cleanup retain idle, done, and error. Full reset clears
everything; soft reset and screen switches preserve records. The host supplies
these events for one terminal and owns history, notifications, and display.
See the [store guide](../crates/osc7501-store/README.md#handle-terminal-events)
for the event mapping.

## Define equality and composition

The tests distinguish three observations:

| Observation | What must match |
| --- | --- |
| Parsed representation | Fields, original base64, and original capability lists |
| Report meaning | Action, id, explicit fields, and decoded text, including absent versus empty |
| Store behavior | Current records, inherited apps, removals, and future eviction choices |

`Report` equality preserves base64 spelling. `Capabilities` equality preserves
list presence and unknown names. Semantic comparisons decode text or use known
capability values. Parsing and emission preserve the meaning of valid producer
reports; emission need not reproduce accepted input bytes. Field order,
terminators, ignored extensions, and optional base64 padding can differ.

A byte stream acts on decoder state. Feeding `x` followed by `y` must match
feeding their concatenation, including errors and behavior on later input.
Feeding empty input is the identity. Stopping after a callback and resuming at
the returned position must preserve that same sequence.

A report acts on the whole store, including update order. Applying commands in
groups must preserve their original order. Commands on different ids need not
commute: changing update order can change the next eviction. Reapplying an
unchanged record refreshes its age. Thus reading and writing back a record is
not an identity operation on the store.

Inherited apps remain queries over the current ancestors. Copying an inherited
app into a child would prevent later ancestor changes from reaching that child.
Borrowed and owned storage must give the same observations for the same capacity
and command history. Removal order within one cleanup is not specified.

Generated tests use fixed random seeds and shrink failing cases. Streaming
tests vary input and partitions, including stops and resumption. Store tests
compare both storage forms with a model ordered by last update, then insert new
records to expose differences in future eviction. These tests need `proptest`
only during development; it adds no runtime dependency.

## Preserve useful errors

Value construction, parsing, and decoding share the parser's error type.
Streaming passes parser errors to the callback and wraps callback failures in
`stream::Error`. `BufferTooSmall` means the destination is too small; `Limit` means
input exceeded a protocol limit. Producer and store errors wrap parser errors
without losing their source. Producer I/O errors preserve their source too.

All error enums are non-exhaustive, as are `State`, `Kind`, `Event`, and
`Limit`, so new protocol cases can be added. `Action` stays exhaustive:
`Set(State)` or `Clear`. The shared `thiserror` dependency has `std`
disabled, and error handling does not allocate.

## Measure before optimizing

Parsing, decoding, sending, and store operations use fixed buffers without
heap allocation after storage setup. The owned store constructor allocates once.
The decoder and producer each use a 4096-byte frame buffer.
Text validation uses a 2048-byte scratch buffer. Slots dominate store
memory; the [measurements](performance.md) record layouts and stack checks.

I would add SIMD or platform-specific code only after a measured improvement
that preserves bounds, portability, and binary size. Compare body parsing,
complete frames, and streaming separately. Keep the independent base64 oracle
and zero-allocation checks in place.
