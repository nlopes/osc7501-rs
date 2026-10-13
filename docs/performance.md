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
runner memory are separate from library memory. A future retained record store
will need its own measured budget; it is not part of this implementation.

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
and `clippy::todo` denied. Unsafe code is forbidden in both production crates.

## Reproduce

```sh
cargo nextest run --workspace --locked
cargo run --release -p osc7501-parser --example benchmark
```

The benchmark has no network activity. Setup allocations and printing happen
outside the measured allocation region. Do not use this microbenchmark to
predict a whole TUI's performance; application scheduling and rendering dominate
normal status-report traffic.
