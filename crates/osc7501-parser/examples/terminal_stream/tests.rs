use std::io::{self, Read};

use hegel::{TestCase, generators as gs};
use osc7501_parser::{Event, MAX_SEQUENCE, parse_sequence};

use super::adapter::{Adapter, Kind, Segment};

const REPORT: &[u8] = b"\x1b]7501;state=working:app=cargo\x1b\\";

#[derive(Debug, Default, Eq, PartialEq)]
struct Observed {
    bytes: Vec<u8>,
    frames: Vec<(Kind, Vec<u8>)>,
    resets: usize,
}

impl Observed {
    fn record(&mut self, segment: Segment<'_>) {
        self.bytes.extend_from_slice(segment.bytes);
        match segment.kind {
            Kind::Bytes => {}
            Kind::Reset => self.resets += 1,
            kind => self.frames.push((kind, segment.bytes.to_vec())),
        }
    }
}

fn observe(chunks: &[&[u8]], pause: bool) -> Observed {
    let mut adapter = Adapter::default();
    let mut observed = Observed::default();
    for &chunk in chunks {
        let mut remaining = chunk;
        let mut calls = 0;
        loop {
            calls += 1;
            assert!(calls <= chunk.len() * 3 + 3, "resume must make progress");
            let result = adapter.try_feed(remaining, |segment| {
                observed.record(segment);
                if pause { Err(()) } else { Ok(()) }
            });
            match result {
                Ok(()) => break,
                Err(error) => remaining = &remaining[error.consumed()..],
            }
        }
    }
    let result = adapter.finish(|segment| {
        observed.record(segment);
        Ok::<_, ()>(())
    });
    assert!(result.is_ok());
    observed
}

fn check_splits(input: &[u8]) -> Observed {
    let expected = observe(&[input], false);
    assert_eq!(expected.bytes, input);
    for split in 0..=input.len() {
        let chunks = [&input[..split], &[], &input[split..]];
        assert_eq!(observe(&chunks, false), expected, "split {split}");
        assert_eq!(observe(&chunks, true), expected, "paused split {split}");
    }
    expected
}

#[test]
fn complete_frames_keep_original_bytes() -> Result<(), osc7501_parser::Error> {
    let bel = b"\x1b]7501;state=done:msg=T0s=\x07";
    let many_parameters = format!("\x1b]0;{}\x07", "part;".repeat(24));
    let input = [
        b"before".as_slice(),
        REPORT,
        bel,
        many_parameters.as_bytes(),
        b"after",
    ]
    .concat();
    let observed = check_splits(&input);
    assert_eq!(
        observed.frames,
        vec![
            (Kind::Osc, REPORT.to_vec()),
            (Kind::Osc, bel.to_vec()),
            (Kind::Osc, many_parameters.into_bytes()),
        ]
    );
    assert!(matches!(
        parse_sequence(&observed.frames[0].1)?,
        Some(Event::Report(_))
    ));
    Ok(())
}

#[test]
fn cancellation_is_never_a_complete_report() {
    let prefix = b"\x1b]7501;state=done";
    for ending in [b"\x18".as_slice(), b"\x1a", b"\x1bx", b"\x1b\x07", b"\x1b"] {
        let input = [prefix.as_slice(), ending, REPORT].concat();
        let observed = check_splits(&input);
        assert_eq!(
            observed.frames,
            vec![
                (Kind::Cancelled, prefix.to_vec()),
                (Kind::Osc, REPORT.to_vec()),
            ]
        );
    }
    let observed = check_splits(&[prefix.as_slice(), REPORT].concat());
    assert_eq!(
        observed.frames,
        vec![
            (Kind::Cancelled, prefix.to_vec()),
            (Kind::Osc, REPORT.to_vec()),
        ]
    );
}

#[test]
fn terminal_controls_and_utf8_preserve_context() {
    for prefix in [
        b"plain \xf0\x9f\xa6\x80\xff\xc2".as_slice(),
        b"\x1b[31mred\x1b[0m",
        b"\x1bP1;2q7501;state=done\x1b\\",
        b"\x1b_hidden\x1b\\",
        b"\x1b^hidden\x1b\\",
        b"\x1bXhidden\x1b\\",
        b"\x1b[123",
        b"\x1bPqpayload",
        b"\x1b_hidden",
    ] {
        // ESC exits these control contexts before the following ] starts an OSC.
        let observed = check_splits(&[prefix, REPORT].concat());
        assert_eq!(observed.frames, vec![(Kind::Osc, REPORT.to_vec())]);
    }
    let other = b"\x1b]0;7501;state=done\x07";
    assert_eq!(
        check_splits(other).frames,
        vec![(Kind::Osc, other.to_vec())]
    );
}

#[test]
fn full_reset_uses_vt_dispatch() {
    let observed = check_splits(b"a\x1bcfirst\x1b\0csecond\x1b!cnot-reset\x1b[!psoft");
    assert_eq!(observed.resets, 2);
    assert_eq!(observed.frames, []);
}

#[test]
fn frame_limit_includes_both_terminator_bytes() {
    for ending in [b"\x07".as_slice(), b"\x1b\\"] {
        for length in [
            MAX_SEQUENCE - 1,
            MAX_SEQUENCE,
            MAX_SEQUENCE + 1,
            MAX_SEQUENCE * 3,
        ] {
            let mut frame = b"\x1b]0;".to_vec();
            frame.resize(length - ending.len(), b'x');
            frame.extend_from_slice(ending);
            let input = [frame.as_slice(), REPORT].concat();
            let expected = observe(&[&input], false);
            assert_eq!(expected.bytes, input);
            assert_eq!(expected.frames.len(), 2);
            let (kind, bytes) = &expected.frames[0];
            if length <= MAX_SEQUENCE {
                assert_eq!(*kind, Kind::Osc);
                assert_eq!(*bytes, frame);
            } else {
                assert_eq!(*kind, Kind::Oversized);
                assert!(bytes.len() <= MAX_SEQUENCE);
            }
            assert_eq!(expected.frames[1], (Kind::Osc, REPORT.to_vec()));
            for size in [1, 3, 1024, MAX_SEQUENCE - 1, MAX_SEQUENCE, MAX_SEQUENCE + 1] {
                let chunks: Vec<_> = input.chunks(size).collect();
                assert_eq!(
                    observe(&chunks, true),
                    expected,
                    "length {length}, chunk {size}"
                );
            }
        }
    }
}

#[test]
fn eof_flushes_prefix_once_and_resets_context() {
    for input in [
        b"\x1b".as_slice(),
        b"\x1b]",
        b"\x1b]7501;state=done",
        b"\x1b]7501;state=done\x1b",
    ] {
        assert_eq!(
            check_splits(input).frames,
            vec![(Kind::Incomplete, input.to_vec())]
        );
    }
    let mut adapter = Adapter::default();
    assert!(
        adapter
            .try_feed(b"\x1b]pending\x1b", |_| Ok::<_, ()>(()))
            .is_ok()
    );
    let mut observed = Observed::default();
    assert_eq!(
        adapter.finish(|segment| {
            observed.record(segment);
            Err("stop")
        }),
        Err("stop")
    );
    assert!(
        adapter
            .finish(|segment| {
                observed.record(segment);
                Ok::<_, ()>(())
            })
            .is_ok()
    );
    assert!(
        adapter
            .try_feed(REPORT, |segment| {
                observed.record(segment);
                Ok::<_, ()>(())
            })
            .is_ok()
    );
    assert_eq!(
        observed.frames,
        vec![
            (Kind::Incomplete, b"\x1b]pending\x1b".to_vec()),
            (Kind::Osc, REPORT.to_vec()),
        ]
    );
}

#[test]
fn resume_can_consume_zero_new_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = Adapter::default();
    adapter.try_feed(b"\x1b", |_| Ok::<_, io::Error>(()))?;
    let reason = String::from("pause");
    let error = adapter
        .try_feed(b"plain", |segment| {
            assert_eq!(segment.bytes, b"\x1b");
            Err(reason.as_str())
        })
        .expect_err("the retained ESC is delivered first");
    assert_eq!(error.consumed(), 0);
    assert_eq!(*error.get_ref(), reason);
    let mut bytes = Vec::new();
    adapter.try_feed(&b"plain"[error.consumed()..], |segment| {
        bytes.extend_from_slice(segment.bytes);
        Ok::<_, io::Error>(())
    })?;
    assert_eq!(bytes, b"plain");
    Ok(())
}

#[test]
fn eof_discards_terminal_context_and_bounds_a_pending_escape() {
    let mut adapter = Adapter::default();
    assert!(adapter.try_feed(b"\x1b", |_| Ok::<_, ()>(())).is_ok());
    assert!(adapter.finish(|_| Ok::<_, ()>(())).is_ok());
    assert!(
        adapter
            .try_feed(b"c", |segment| {
                assert_eq!(segment.kind, Kind::Bytes);
                Ok::<_, ()>(())
            })
            .is_ok()
    );

    let mut input = b"\x1b]".to_vec();
    input.resize(MAX_SEQUENCE, b'x');
    input.push(0x1b);
    let observed = observe(&[&input], true);
    assert_eq!(observed.bytes, input);
    assert_eq!(observed.frames, vec![(Kind::Oversized, input)]);
}

#[test]
fn ignored_and_rejected_reports_are_forwarded() -> io::Result<()> {
    let mut input = b"\x1b]7501;state=future\x07\x1b]7501;state=done:msg=A\x07".as_slice();
    let expected = input;
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    super::run(&mut input, &mut output, &mut diagnostics)?;
    assert_eq!(output, expected);
    let diagnostics = String::from_utf8_lossy(&diagnostics);
    assert!(diagnostics.contains("ignored OSC 7501 report"));
    assert!(diagnostics.contains("rejected OSC 7501 report"));
    Ok(())
}

#[test]
fn adapter_does_not_allocate() {
    let mut input = b"\x1b]0;".to_vec();
    input.resize(MAX_SEQUENCE * 2, b'x');
    input.extend_from_slice(b"\x07\x1b]cancel\x18");
    input.extend_from_slice(REPORT);
    input.extend_from_slice(b"\x1b]unfinished\x1b");
    let mut count = 0;
    let allocations = allocation_counter::measure(|| {
        let mut adapter = Adapter::default();
        let mut remaining = input.as_slice();
        while !remaining.is_empty() {
            let result = adapter.try_feed(remaining, |segment| {
                count += segment.bytes.len();
                Err(())
            });
            match result {
                Ok(()) => break,
                Err(error) => remaining = &remaining[error.consumed()..],
            }
        }
        assert!(
            adapter
                .finish(|segment| {
                    count += segment.bytes.len();
                    Ok::<_, ()>(())
                })
                .is_ok()
        );
    });
    assert_eq!(count, input.len());
    assert_eq!(allocations.count_total, 0);
}

struct InterruptedReader<'a> {
    remaining: &'a [u8],
    interrupt: bool,
}

impl Read for InterruptedReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.interrupt = !self.interrupt;
        if self.interrupt {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let length = buffer.len().min(1);
        self.remaining.read(&mut buffer[..length])
    }
}

#[test]
fn filter_forwards_fragmented_input_and_retries_interrupted_reads() -> io::Result<()> {
    let input = [b"text\xff".as_slice(), REPORT, b"\x1bc\x1b]unfinished"].concat();
    let mut reader = InterruptedReader {
        remaining: &input,
        interrupt: false,
    };
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    super::run(&mut reader, &mut output, &mut diagnostics)?;
    assert_eq!(output, input);
    let diagnostics = String::from_utf8_lossy(&diagnostics);
    assert!(diagnostics.contains("Report("));
    assert!(diagnostics.contains("full terminal reset"));
    Ok(())
}

#[test]
fn filter_propagates_partial_write_failure() {
    let mut input = REPORT;
    let mut output = [0; 3];
    let error = super::run(&mut input, &mut output.as_mut_slice(), &mut Vec::new())
        .expect_err("the output cannot hold the frame");
    assert_eq!(error.kind(), io::ErrorKind::WriteZero);
    assert_eq!(output, REPORT[..3]);
}

#[hegel::test]
fn arbitrary_bytes_survive_chunking_and_pauses(tc: TestCase) {
    let parts = tc.draw(
        gs::vecs(hegel::one_of![
            gs::binary().max_size(127),
            gs::just(REPORT.to_vec()),
            gs::just(b"\x1b]7501;state=done\x18".to_vec()),
            gs::just(b"\x1b]partial\x1b".to_vec()),
            gs::just(b"\x1b\0c".to_vec()),
        ])
        .max_size(19),
    );
    let size = tc.draw(gs::integers::<usize>().min_value(1).max_value(63));
    let input = parts.concat();
    let expected = observe(&[&input], false);
    assert_eq!(&expected.bytes, &input);
    let chunks: Vec<_> = input.chunks(size).collect();
    assert_eq!(observe(&chunks, true), expected);
}
