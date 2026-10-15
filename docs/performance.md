# Performance and verification

Measured 2026-10-13 in this development environment:

- Rust `1.99.0 (b940084d7 2026-09-28)`, latest stable channel at execution.
- cargo-nextest `0.9.148`.
- Linux 6.18.44, x86-64, AMD EPYC 9V74 virtual CPU.
- Release optimization, thin LTO, one codegen unit, portable target defaults.
- Median of five samples, 100,000 operations each, following warmup.
- Results include validation; decoded text is materialized only for validation.

| Report | Body parse | Complete frame | Stream in 64-byte chunks |
| --- | ---: | ---: | ---: |
| Short state/app/progress | 92.3 ns | 107.0 ns | 186.2 ns |
| Permission block with message | 261.3 ns | 302.7 ns | 532.8 ns |
| 2048-byte message + 192-byte title | 6908.4 ns | 8573.9 ns | 15007.5 ns |

Body throughput: 361.8, 328.5, and 419.2 MiB/s respectively. Chunked-stream
throughput: 225.4, 177.2, and 193.6 MiB/s. This is an initial portable baseline,
not a cross-machine guarantee or a claim of optimality. Tiny reports are best
compared by latency; throughput grows with payload length.

## Memory

The allocation instrument observed **0 allocations and 0 peak heap bytes**
for repeated maximum text parsing, explicit decoding, streaming, and bounded
producer output. Struct sizes on this target:

| Component | Bytes |
| --- | ---: |
| `Decoder` including its frame storage | 4112 |
| Borrowed `Report` | 88 |
| Largest validation scratch buffer | 2048 |
| Caller buffer for one largest decoded message | 2048 |
| Producer frame buffer while emitting | 4096 |

Working storage is in kilobytes, well below 1,000,000 bytes. These rows describe
buffers and type layouts rather than exact peak call-stack depth. No recursion
or input-proportional heap storage exists. Whole-process RSS and compiler/test
runner memory are separate from library memory. The opt-in record store has
its own measurement below.

## Verification

47 tests passed under `cargo nextest run --workspace`. The suite covers states,
blocking kinds, dependent fields, both ST forms, exact 4096-byte boundaries,
every field cap, override validation, invalid ids, all C0/DEL/C1 controls,
UTF-8 failures, capability extensions/defaults/empty lists, every two-chunk
split, bytewise streaming, oversized-frame recovery, and producer round trips.

An independent base64 oracle tests every decoded message length from 0 through
2048, with padding and without padding. Exhaustive single-byte mutations of a
representative report compare streaming and complete-frame outcomes. The memory
tests use caller-owned destinations and assert zero heap allocation.

Formatting and workspace Clippy pass with pedantic enabled, warnings denied,
and `clippy::todo` denied. Unsafe code is forbidden in every production crate.

## Reproduce

```sh
cargo nextest run --workspace --locked
cargo run --release -p osc7501-parser --example benchmark
```

The benchmark has no network activity. Setup allocations and printing happen
outside the measured allocation region. Do not use this microbenchmark to
predict a whole TUI's performance; application scheduling and rendering dominate
normal status-report traffic.

## Capability reply memory

Measured 2026-10-14 on macOS, aarch64, Rust 1.99.0:

| Component | Bytes |
| --- | ---: |
| Borrowed `CapabilityReply` | 32 |
| Local frame buffer during reply emission | 4096 |

Allocation tests observed zero allocations and zero peak heap bytes for default,
empty, full known lists, and 4096-byte replies through a trait-object writer.
Oversized replies also allocated nothing and made no caller writes. The local
frame buffer size is not a measurement of peak stack depth or whole-process RSS.

## Record store memory

Measured 2026-10-15 on macOS, aarch64, Rust 1.99.0:

| Component | Bytes |
| --- | ---: |
| One `Slot`, including fields and update rank | 2480 |
| 256 caller-owned slots | 634880 |
| Borrowed `Store` metadata | 24 |
| `Decoder` including frame storage | 4112 |
| Conservative prepared-record storage, bounded by one slot | 2480 |
| Parser text-validation scratch | 2048 |
| Total storage and named scratch | 643544 |

The workload retains root and 255 children. Every record carries a 2048-byte
Unicode message, a 192-byte Unicode title, and a 32-byte app; child ids use all
128 bytes. States cover idle, working, blocked, done, and error. The tests cover
fragmented input, ignored and rejected frames at capacity, replacement,
eviction, lookup, app resolution, iteration, clearing, and all lifecycle calls.
The allocation counter observes zero allocations and zero peak heap bytes
throughout these operations, including store construction.

### Stack check

The workload runs on a thread configured with a 64 KiB stack, or 128 KiB on
Windows. Caller slots and input fixtures are allocated before spawning that
thread. This exercises compiler temporaries and nested parsing, replacement,
and removal calls. The Windows allowance includes room for temporary values in
debug builds and
[Windows stack overflow handling](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadstackguarantee).
Success confirms that the workload fits on the tested platform. It does not
measure exact peak stack depth.
With the measured layouts, slots, store metadata, decoder storage, and the
entire configured thread stack total 704552 bytes with 64 KiB, or 770088 bytes
with 128 KiB. Both are below the 1,000,000-byte library budget.
Caller-retained inputs, test setup allocations, allocator overhead, and
whole-process RSS are separate.

Reproduce the layout, allocation, and stack checks with:

```sh
cargo nextest run -p osc7501-store --test resources --locked --success-output immediate
cargo nextest run --release -p osc7501-store --test resources --locked --success-output immediate
```
