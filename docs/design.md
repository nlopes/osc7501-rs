# Rust interface and resource design

## Implemented workspace

All workspace members live in `crates/`:

| Crate | Responsibility | Runtime dependencies |
| --- | --- | --- |
| `osc7501-parser` | Wire parsing, validation, types, capabilities, bounded streaming | `thiserror` without default features; `no_std` |
| `osc7501` | Borrowed producer builder, bounded encoding, output-writer integration | Local parser and `thiserror` without default features |

The producer's Cargo package is `osc7501-rs`; its Rust library is `osc7501`.

`osc7501::parser` exposes the complete parser crate. `AppId`, `RecordId`,
`Progress`, `State`, and `Kind` are also available at the emission crate's root
for building reports. Parser-only types, functions, and limits use the `parser`
namespace. Code that only parses reports can depend on `osc7501-parser` directly.

The parser separates **ignore** (`Ok(None)`) from **reject** (`Err`) and accepted
events (`Ok(Some(...))`). It never exposes partially validated record updates.
Capability replies read only `states` and `kinds`. Other pairs are ignored,
including keys and values that exceed report-field limits. The complete-frame
cap still applies to replies.
`Report` and `Capabilities` are constructed by parsing. Their fields are
read-only outside the parser crate. Report getters preserve the parser's
validation guarantees, including the states that allow `kind` and `progress`.
Each report replaces the record without checking its previous state. Applications
can enforce their own workflow rules.
`Action::Clear` prevents confusing subtree deletion with a stored state.
`RecordId::new`, `AppId::new`, and `Progress::new` return `Result` without
allocating. Invalid inputs return `Error::InvalidRecordId`, `Error::InvalidAppId`,
or `Error::InvalidProgress`. `TryFrom<&str>` for identifiers and `TryFrom<u8>`
for progress use the same validation. The parser still discards reports with
invalid ids and treats invalid apps or progress as absent. Hard-cap violations
still reject the report with `Error::Limit`. State-specific producer fields are
checked before output begins.

Value construction, parsing, streaming, and text decoding share `Error`.
`BufferTooSmall` means the caller supplied too little decode storage; `Limit`
means input exceeded a protocol cap. Both crates inherit `thiserror` from the
workspace manifest with its `std` feature disabled. The generated error
implementations do not allocate. The derive macro adds host build dependencies.
The producer's `Error::Parser` wraps parser errors, so `?` can propagate value
construction, parsing, and text decoding through `osc7501::Error`. Its `Error::Io`
wraps writer failures. Both variants preserve the original error as their source.
Both crates' error enums allow new variants without a breaking API change.

`State`, `Kind`, `Event`, and `Limit` are non-exhaustive so protocol support can
grow without breaking caller matches. `Action` stays exhaustive: `Set(State)`
covers state updates and `Clear` covers removal.

`Report<'a>` borrows identifiers and base64 input. `EncodedText` is validated
eagerly, then decoded into caller storage only when displayed. This avoids
allocating strings for states the host never renders. Streaming callbacks
borrow the decoder for their duration, making accidental retention across the
next frame impossible in safe Rust.

## TUI integration

Use `Status::new(State::Working)` and replace the complete record when its
meaning changes. Use `Status::new(State::Blocked).kind(Kind::Permission)` for
approval, `Question` for typed answers, and `Auth` for credentials. Omit progress
when the amount of work is unknown. Repeat root `app` on every update.

Send through the writer/lock already used for rendering. Both `Status::write_to`
and `write_query` accept concrete writers and `&mut dyn Write` directly.
`write_to` first validates and encodes into a fixed stack frame, then calls
`write_all` on the caller writer. No implicit output channel, flush, background
thread, signal handler, input reader, or terminal mode change is introduced.
Writer failures propagate; a transport error may leave a partial frame. The
caller determines TTY policy, flush timing, output serialization, and error policy.

Feature queries are explicit. Route responses through the existing TUI input
loop; never install a competing reader. Track a pending query, interpret bare
`Event::Query` as a default reply on terminal-to-program input, and use a caller
deadline. Programs unable to read replies can emit statuses directly.

On cancellation, explicitly send idle. On completion, explicitly send done or
error. Avoid automatic clear-on-drop: it could erase the result a user needs to
see after the process exits. Cleanup semantics should be visible in application
code and tested with its actual shutdown path.

For concurrent tasks, use validated hierarchical ids such as `build/test` and
`build/docs`. Emit independent records rather than forcing them into one global
percentage. Choose one owner for root updates so workers cannot overwrite each
other. `Status::clear().id(id)` removes a finished subtree when appropriate.

## Memory contract

- Core parsing uses no allocator or `alloc` crate and has no retained state.
- Text validation uses a reusable 2048-byte local decode buffer per call.
- The scanner owns a single 4096-byte inline frame buffer plus small metadata.
- Consumers provide text destinations; 2048 bytes handles the largest message.
- The producer owns a 4096-byte local encoding buffer during emission.
- No recursion, per-pair collections, dynamic strings, caches, or task registries.

The largest report this producer emits is 3220 bytes, with each supported field
present once at its limit. The specification sets a 4096-byte complete-frame
cap and says the largest legal report is under 3300 bytes. The parser keeps the
4096-byte bound for frames with unknown keys or repeated pairs, which the wire
grammar permits. The producer test checks its exact maximum length, parses the
result, and verifies zero heap allocations.

These are library buffers, not a whole-process RSS promise. The Rust standard
runtime, dynamic loader, TUI framework, threads, and caller-retained reports
have their own memory. Allocation-counter tests exercise maximal text, complete
frames, chunked input, and producer output with **zero heap allocations**.
Measured sizes and throughput are recorded in `performance.md`.

## Terminal-side record store: next independent crate

A future `osc7501-store` belongs in `crates/`, behind a separate opt-in dependency.
It should use caller-selected capacity (64–256), fixed slots or caller storage,
bounded owned text, atomic full replacement, nearest-ancestor app lookup,
slash-aware subtree clearing, and a monotonically ordered update counter for
eviction. An update must validate completely before choosing or mutating a slot.

At 256 records, raw maximum field storage is
`256 * (2048 + 192 + 128 + 32) = 614400` bytes. Fixed metadata, scanner, and
validation scratch can fit comfortably within a 1,000,000-byte library budget.
Enforce that budget with layout assertions and measured maximal-load tests
before claiming a store-level guarantee. This store is designed, not implemented.

Expose explicit `on_prompt`, `on_process_exit`, `on_ris`, and `on_decstr`
transitions. Terminal screen switches do not trigger cleanup. Display consumers
handle invisible formatting, origin attribution, and notification rate limits.
Keep report parsing independent of terminal UX and unrelated OSC mapping.

## Performance policy

Start with a portable safe byte parser and a 256-byte base64 lookup table.
Use independent decoding-oracle tests before optimizing. Bench body dispatch,
complete framing, and chunked streaming separately. Keep parsing and text
decoding allocation-free; benchmark/test dependencies never enter users' binaries.
Add SIMD or architecture-specific paths only when comparative evidence shows
a useful benefit without compromising portability, bounds, or binary size.
