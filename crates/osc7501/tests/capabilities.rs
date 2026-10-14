//! Capability replies preserve list presence and validate before output.
use osc7501::{
    CapabilityReply, Error, Kind, State,
    parser::{self, Event, Limit, MAX_SEQUENCE, parse_sequence},
};
use std::{
    error::Error as _,
    io::{self, Write},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const STATES: &[State] = &[
    State::Idle,
    State::Working,
    State::Done,
    State::Blocked,
    State::Error,
];
const KINDS: &[Kind] = &[Kind::Permission, Kind::Question, Kind::Auth];

#[test]
fn default_reply_has_the_bare_query_wire_form() -> TestResult {
    for reply in [CapabilityReply::new(), CapabilityReply::default()] {
        let mut output = Vec::new();
        reply.write_to(&mut output)?;
        assert_eq!(output, b"\x1b]7501;?\x1b\\");
        assert_eq!(parse_sequence(&output)?, Some(Event::Query));
    }
    Ok(())
}

#[test]
fn explicit_lists_round_trip_every_known_value_in_order() -> TestResult {
    let states = [
        State::Error,
        State::Blocked,
        State::Done,
        State::Working,
        State::Idle,
        State::Error,
    ];
    let kinds = [Kind::Auth, Kind::Question, Kind::Permission, Kind::Auth];
    let mut output = Vec::new();
    CapabilityReply::new()
        .states(&states)
        .kinds(&kinds)
        .write_to(&mut output)?;
    assert_eq!(
        output,
        b"\x1b]7501;?:states=error,blocked,done,working,idle,error:kinds=auth,question,permission,auth\x1b\\"
    );
    let Some(Event::Capabilities(reply)) = parse_sequence(&output)? else {
        return Err("expected capabilities".into());
    };
    assert_eq!(reply.states().collect::<Vec<_>>(), states);
    assert_eq!(reply.kinds().collect::<Vec<_>>(), kinds);
    Ok(())
}

#[test]
fn absent_empty_and_populated_lists_remain_distinct() -> TestResult {
    let state_choices = [None, Some(&[][..]), Some(STATES)];
    let kind_choices = [None, Some(&[][..]), Some(KINDS)];
    for states in state_choices {
        for kinds in kind_choices {
            let mut reply = CapabilityReply::new();
            if let Some(states) = states {
                reply = reply.states(states);
            }
            if let Some(kinds) = kinds {
                reply = reply.kinds(kinds);
            }
            let mut output = Vec::new();
            reply.write_to(&mut output)?;
            if states.is_none() && kinds.is_none() {
                assert_eq!(parse_sequence(&output)?, Some(Event::Query));
                continue;
            }
            let Some(Event::Capabilities(reply)) = parse_sequence(&output)? else {
                return Err("expected capabilities".into());
            };
            assert_eq!(reply.states().collect::<Vec<_>>(), states.unwrap_or(STATES));
            assert_eq!(reply.kinds().collect::<Vec<_>>(), kinds.unwrap_or(KINDS));
            assert_eq!(
                output.windows(b":states=".len()).any(|w| w == b":states="),
                states.is_some()
            );
            assert_eq!(
                output.windows(b":kinds=".len()).any(|w| w == b":kinds="),
                kinds.is_some()
            );
        }
    }
    Ok(())
}

#[test]
fn complete_frame_cap_includes_lists_and_terminator() -> TestResult {
    let mut states = [State::Idle; 815];
    states[0] = State::Error;
    states[1] = State::Working;
    let kinds = [Kind::Auth; 816];
    let combined_kinds = [Kind::Auth; 813];
    for reply in [
        CapabilityReply::new().states(&states),
        CapabilityReply::new().kinds(&kinds),
        CapabilityReply::new()
            .states(&[State::Working])
            .kinds(&combined_kinds),
    ] {
        let mut writer = TestWriter::new();
        reply.write_to(&mut writer)?;
        assert_eq!(writer.length, MAX_SEQUENCE);
        assert!(matches!(
            parse_sequence(writer.bytes())?,
            Some(Event::Capabilities(_))
        ));
    }

    states[2] = State::Error;
    for reply in [
        CapabilityReply::new().states(&states),
        CapabilityReply::new().states(&[]).kinds(&kinds),
        CapabilityReply::new()
            .states(&[State::Working, State::Idle])
            .kinds(&combined_kinds),
    ] {
        let mut writer = TestWriter::new();
        writer.write_all(b"existing output")?;
        let prior_calls = writer.writes;
        let mut result = Ok(());
        let info = allocation_counter::measure(|| result = reply.write_to(&mut writer));
        let error = result.err().ok_or("expected oversized reply")?;
        assert!(matches!(
            error,
            Error::Parser(parser::Error::Limit(Limit::Sequence))
        ));
        assert_eq!(
            error.source().and_then(|source| source.downcast_ref()),
            Some(&parser::Error::Limit(Limit::Sequence))
        );
        assert_eq!(writer.bytes(), b"existing output");
        assert_eq!(writer.writes, prior_calls);
        assert_eq!(info.count_total, 0);
        assert_eq!(info.bytes_max, 0);
    }
    Ok(())
}

#[test]
fn trait_object_replies_do_not_allocate_or_flush() -> TestResult {
    let kinds = [Kind::Auth; 816];
    for (reply, expected_length) in [
        (CapabilityReply::new(), 10),
        (CapabilityReply::new().states(&[]).kinds(&[]), 25),
        (CapabilityReply::new().states(STATES).kinds(KINDS), 80),
        (CapabilityReply::new().kinds(&kinds), MAX_SEQUENCE),
    ] {
        let mut writer = TestWriter::new();
        let output: &mut dyn Write = &mut writer;
        let mut result = Ok(());
        let info = allocation_counter::measure(|| result = reply.write_to(output));
        result?;
        assert_eq!(info.count_total, 0);
        assert_eq!(info.bytes_max, 0);
        assert_eq!(writer.length, expected_length);
        assert_eq!(writer.flushes, 0);
        let output: &mut dyn Write = &mut writer;
        output.flush()?;
        assert_eq!(writer.flushes, 1);
    }
    Ok(())
}

#[test]
fn short_writes_and_interrupted_writes_complete_the_reply() -> TestResult {
    let mut writer = TestWriter::new();
    writer.chunk_size = 3;
    writer.interrupt_next = true;
    CapabilityReply::new()
        .states(&[State::Working, State::Done])
        .kinds(&[])
        .write_to(&mut writer)?;
    assert_eq!(
        writer.bytes(),
        b"\x1b]7501;?:states=working,done:kinds=\x1b\\"
    );
    assert!(writer.writes > 1);
    assert_eq!(writer.flushes, 0);
    Ok(())
}

#[test]
fn io_errors_keep_their_source_and_can_leave_a_partial_reply() -> TestResult {
    for fail_after in [0, 5] {
        let mut writer = TestWriter::new();
        writer.fail_after = fail_after;
        let error = CapabilityReply::new()
            .write_to(&mut writer)
            .err()
            .ok_or("expected transport failure")?;
        assert!(matches!(error, Error::Io(_)));
        let source = error
            .source()
            .and_then(|source| source.downcast_ref::<io::Error>())
            .ok_or("missing I/O source")?;
        assert_eq!(source.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(source.to_string(), "test transport failure");
        assert_eq!(writer.bytes(), &b"\x1b]7501;?\x1b\\"[..fail_after]);
        assert_eq!(writer.flushes, 0);
    }
    Ok(())
}

struct TestWriter {
    output: [u8; MAX_SEQUENCE],
    length: usize,
    chunk_size: usize,
    fail_after: usize,
    interrupt_next: bool,
    writes: usize,
    flushes: usize,
}

impl TestWriter {
    fn new() -> Self {
        Self {
            output: [0; MAX_SEQUENCE],
            length: 0,
            chunk_size: MAX_SEQUENCE,
            fail_after: usize::MAX,
            interrupt_next: false,
            writes: 0,
            flushes: 0,
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.output[..self.length]
    }
}

impl Write for TestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        if self.interrupt_next {
            self.interrupt_next = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        if self.length >= self.fail_after {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test transport failure",
            ));
        }
        let length = bytes
            .len()
            .min(self.chunk_size)
            .min(self.fail_after - self.length)
            .min(self.output.len() - self.length);
        self.output[self.length..self.length + length].copy_from_slice(&bytes[..length]);
        self.length += length;
        Ok(length)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}
