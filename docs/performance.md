# Performance measurements

These are dated measurements from specific machines. Use them to compare
parser paths and understand storage costs. For a code change, compare before
and after on the same machine with the same build settings.

## Parser throughput

Recorded on 2026-10-13:

- Linux 6.18.44, x86-64, AMD EPYC 9V74 virtual CPU.
- Rust `1.99.0 (b940084d7 2026-09-28)`; cargo-nextest `0.9.148`.
- Release build with thin LTO, one codegen unit, and portable target defaults.
- Median of five samples, each with 100,000 operations after warmup.

Parsing includes validation. Text is decoded for validation, but the returned
report still borrows its encoded input.

| Report | Body parse | Complete frame | Stream in 64-byte chunks |
| --- | ---: | ---: | ---: |
| Short state/app/progress | 92.3 ns | 107.0 ns | 186.2 ns |
| Permission block with message | 261.3 ns | 302.7 ns | 532.8 ns |
| 2048-byte message + 192-byte title | 6908.4 ns | 8573.9 ns | 15007.5 ns |

Body throughput was 361.8, 328.5, and 419.2 MiB/s respectively.
Stream throughput was 225.4, 177.2, and 193.6 MiB/s. Latency is the more useful
comparison for small reports; throughput increases with payload size.

Run the benchmark from the repository root:

```sh
cargo run --release -p osc7501-parser --example benchmark
```

Setup allocations and printing are outside the measured work. These numbers
describe parsing; application scheduling and rendering also affect TUI speed.

## Parser and producer memory

The recorded checks found zero allocations and zero peak heap bytes while
parsing maximum text, decoding, streaming, and sending reports.
Sizes on the Linux target above:

| Component | Bytes |
| --- | ---: |
| Decoder, including frame storage | 4112 |
| Borrowed report | 88 |
| Text-validation scratch buffer | 2048 |
| Caller buffer for a maximum decoded message | 2048 |
| Producer frame buffer | 4096 |

The 2026-10-13 baseline passed 47 tests, including frame limits, text validation,
stream splits, recovery, and producer round trips. An independent base64 oracle
covers every message length from 0 to 2048 bytes. Single-byte mutations compare
complete-frame and streaming results.

## Capability reply memory

Recorded on 2026-10-14 with Rust 1.99.0 on macOS aarch64:

| Component | Bytes |
| --- | ---: |
| Borrowed `CapabilityReply` | 32 |
| Reply frame buffer | 4096 |

Default, empty, full, and 4096-byte replies used zero allocations and zero peak
heap bytes. Oversized replies also allocated nothing and wrote no caller output.

## Record store memory

Recorded on 2026-10-15 with Rust 1.99.0 on macOS aarch64:

| Component | Bytes |
| --- | ---: |
| One slot | 2480 |
| 256 caller-owned slots | 634880 |
| Store metadata | 24 |
| Decoder, including frame storage | 4112 |
| Conservative preparation buffer, bounded by one slot | 2480 |
| Text-validation scratch buffer | 2048 |
| Total storage and named scratch | 643544 |

The workload keeps root plus 255 children. Each record has a 2048-byte Unicode
message, a 192-byte Unicode title, and a 32-byte app; child ids use all 128 bytes.
It exercises parsing, replacement, eviction, lookup, inheritance, clearing, and
terminal cleanup. Store construction and operations used zero allocations and
zero peak heap bytes.

### Stack check

The resource test uses a 64 KiB thread stack, or 128 KiB on Windows. Caller slots
and fixtures are allocated before the thread starts. Windows gets extra room
for debug-build copies and stack-overflow handling.

The macOS workload completed in debug and release with the 64 KiB stack.
Using the recorded layouts, slots, metadata, decoder, and the entire configured
stack total 704552 bytes with 64 KiB, or 770088 bytes with 128 KiB. Both fit the
1,000,000-byte library budget.

These figures cover library storage. They do not measure exact stack peaks or
whole-process RSS, which also includes the runtime, allocator, UI, and caller data.
A successful stack check proves only that the tested workload fit.

## Recheck resource use

Run the store resource tests in both build modes to check layouts, allocations,
and the bounded-stack workload:

```sh
cargo nextest run -p osc7501-store --test resources --locked --success-output immediate
cargo nextest run --release -p osc7501-store --test resources --locked --success-output immediate
```

For the full regression suite and documentation examples, follow the
[development workflow](../README.md#working-on-the-code).
