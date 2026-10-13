# Rust interface and resource design

## Implemented workspace

All workspace members live in `crates/`:

| Crate | Responsibility | Runtime dependencies |
| --- | --- | --- |
| `osc7501-parser` | Wire parsing, validation, types, capabilities, bounded streaming | `thiserror` without default features; `no_std` |
| `osc7501` | Borrowed producer builder, bounded encoding, output-writer integration | Local parser and `thiserror` without default features |

The parser separates **ignore** (`Ok(None)`) from **reject** (`Err`) and accepted
events (`Ok(Some(...))`). It never exposes partially validated record updates.
`Action::Clear` prevents confusing subtree deletion with a stored state.
Validated `RecordId`, `AppId`, and `Progress` reject invalid producer identities
or percentages without allocating. State-specific producer fields are checked
before output begins.

Parsing, streaming, and text decoding share `Error`. `BufferTooSmall` means the
caller supplied too little decode storage; `Limit` means input exceeded a
protocol cap. Both crates inherit `thiserror` from the workspace manifest with
its `std` feature disabled. The generated error implementations do not allocate.
The derive macro adds host build dependencies. The producer's `Error::Io`
preserves the writer error as its source.

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

Send through the writer/lock already used for rendering. `write_to` first
validates and encodes into a fixed stack frame, then calls `write_all` on the
caller writer. No implicit output channel, flush, background thread, signal
handler, input reader, or terminal mode change is introduced. Writer failures
propagate; a transport error may leave a partial frame. The caller determines
TTY policy, flush timing, output serialization, and error policy.

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
