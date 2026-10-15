# Rust interface and resource design

## Implemented workspace

All workspace members live in `crates/`:

| Crate | Responsibility | Runtime dependencies |
| --- | --- | --- |
| `osc7501-parser` | Wire parsing, validation, types, capabilities, bounded streaming | `thiserror` without default features; `no_std` |
| `osc7501` | Report emission, capability replies, optional store access | Local parser and `thiserror` without default features; optional local store |
| `osc7501-store` | Current records, app inheritance, eviction, explicit lifetime cleanup | Local parser and `thiserror` without default features; `no_std` |

The producer's Cargo package is `osc7501-rs`; its Rust library is `osc7501`.

`osc7501::parser` exposes the complete parser crate. `AppId`, `RecordId`,
`Progress`, `State`, and `Kind` are also available at the emission crate's root
for building reports. `CapabilityReply` builds terminal capability replies.
Parser-only types, functions, and limits use the `parser` namespace. Code that
only parses reports can depend on `osc7501-parser` directly.

The optional `store` feature exposes `osc7501::store`. It is off by default.
The standalone store depends only on the parser and `thiserror`; its `parser`
re-export supplies the types used by its public API. Both standalone crates
remain `no_std`. The combined host example belongs to the producer package,
requires `store`, and uses the producer's re-exports throughout. This keeps
normal and development dependencies acyclic. CI checks default and all
features; docs.rs builds all features.

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
means input exceeded a protocol cap. All crates inherit `thiserror` from the
workspace manifest with its `std` feature disabled. The generated error
implementations do not allocate. The derive macro adds host build dependencies.
The producer's `Error::Parser` wraps parser errors, so `?` can propagate value
construction, parsing, and text decoding through `osc7501::Error`. Its `Error::Io`
wraps writer failures. Both variants preserve the original error as their source.
All crates' error enums allow new variants without a breaking API change.

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

## Terminal capability replies

`CapabilityReply` borrows typed state and kind slices. `new()` and `Default`
omit both lists and emit the bare default reply. Builder methods make a list
explicit, including an empty list. Parsed `Capabilities` stays read-only.
The host chooses when to reply and must advertise only implemented features.

Reply emission preserves list order and duplicates, encodes the full frame
before caller output, and accepts `&mut dyn Write`. Buffer exhaustion returns
`Error::Parser(Error::Limit(Limit::Sequence))`; caller writer failures return
`Error::Io`. It does not allocate or flush. A transport error can leave a
partial frame. The bare reply is interpreted as `Event::Query` on input; the
caller supplies stream direction and pending-query state.

## Memory contract

- Core parsing uses no allocator or `alloc` crate and has no retained state.
- Text validation uses a reusable 2048-byte local decode buffer per call.
- The scanner owns a single 4096-byte inline frame buffer plus small metadata.
- Consumers provide text destinations; 2048 bytes handles the largest message.
- The producer owns a 4096-byte local encoding buffer during emission.
- Parsing and emission use no recursion, per-pair collections, dynamic strings,
  caches, or task registries. The separate store retains bounded records.

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

## Terminal-side record store

`osc7501-store` depends on the parser. The producer exposes it through an
optional dependency; the parser has no store dependency. `Store::new` borrows
caller-owned `Slot` storage and starts empty. Capacities from 1 through 256
are accepted; a conforming terminal host needs at least 64, including root.

`apply(report, on_remove)` copies identifiers and decodes text into one prepared
record before choosing or mutating a slot. Omitted fields disappear. `Record`
views expose validated identifiers and decoded text through read-only methods.
`get` and `iter` borrow the store. `effective_app` resolves the nearest ancestor
with an app at lookup time, so parent updates and removals affect inheritance
without changing descendants' explicit fields. Parents may be absent.

Clear removes the addressed record and slash-delimited descendants; root clear
removes every record. At capacity, insertion evicts the least recently updated
record. Existing-record updates never evict another record. Active update ranks
stay dense within the slot count. Moving an updated record to the newest rank
and closing gaps on removal avoids a wrapping global counter. Operations scan
bounded slots; batch removal can scan them once per removed record.

`apply` calls `on_remove` with the old record before clearing or evicting it.
Ordinary replacement does not call it. The callback can observe root or child
identity and old fields without an owned change list. Lifecycle cleanup uses
the same callback contract. Notification and history policy stay with the host.

`on_prompt` and `on_process_exit` remove working and blocked records. They retain
idle, done, and error. `on_full_reset` removes every record. Soft resets and
screen switches require no store action. Events apply to the represented
terminal, not guessed child process ids. Display consumers handle invisible
formatting, origin attribution, and notification rate limits.

On macOS aarch64, 256 slots occupy 634880 bytes. Store metadata, decoder storage,
one conservative replacement buffer, and parser validation scratch total
643544 bytes. The maximum-field workload also completes on a thread configured
with a 64 KiB stack. These checks cover library storage and an observed stack
limit, not whole-process RSS or exact peak stack depth. See `performance.md`
for the components and reproduction command.

## Performance policy

Start with a portable safe byte parser and a 256-byte base64 lookup table.
Use independent decoding-oracle tests before optimizing. Bench body dispatch,
complete framing, and chunked streaming separately. Keep parsing and text
decoding allocation-free; benchmark/test dependencies never enter users' binaries.
Add SIMD or architecture-specific paths only when comparative evidence shows
a useful benefit without compromising portability, bounds, or binary size.
