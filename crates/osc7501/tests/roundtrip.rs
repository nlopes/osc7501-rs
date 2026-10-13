//! Producer-to-parser conformance and validation before writes.
use osc7501::{
    AppId, Error, Kind, Progress, RecordId, State, Status,
    parser::{self, Action, Event, MAX_MESSAGE, parse_sequence},
    write_query,
};
use std::{
    error::Error as _,
    io::{self, Write},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn emitter_round_trips_every_state() -> TestResult {
    let app = AppId::new("example")?;
    let id = RecordId::new("build/test")?;
    for state in [
        State::Idle,
        State::Working,
        State::Blocked,
        State::Done,
        State::Error,
    ] {
        let mut output = Vec::new();
        Status::new(state)
            .id(id)
            .app(app)
            .title("Test 🦀")
            .message("España")
            .write_to(&mut output)?;
        let Some(Event::Report(value)) = parse_sequence(&output)? else {
            return Err("expected report".into());
        };
        assert_eq!(value.action, Action::Set(state));
        assert_eq!(value.id, Some(id));
        assert_eq!(value.app, Some(app));
        let mut decoded = [0; MAX_MESSAGE];
        assert_eq!(
            value
                .message
                .ok_or("missing message")?
                .decode_into(&mut decoded)?,
            "España"
        );
        assert_eq!(
            value
                .title
                .ok_or("missing title")?
                .decode_into(&mut decoded)?,
            "Test 🦀"
        );
    }
    Ok(())
}

#[test]
fn emitter_round_trips_blocking_reason_and_progress() -> TestResult {
    let mut output = Vec::new();
    Status::new(State::Blocked)
        .kind(Kind::Permission)
        .progress(Progress::new(100)?)
        .write_to(&mut output)?;
    let Some(Event::Report(value)) = parse_sequence(&output)? else {
        return Err("expected report".into());
    };
    assert_eq!(value.kind, Some(Kind::Permission));
    assert_eq!(value.progress.ok_or("missing progress")?.get(), 100);
    Ok(())
}

#[test]
fn root_clear_and_subtree_clear_round_trip() -> TestResult {
    for id in [None, Some(RecordId::new("build/test")?)] {
        let mut output = Vec::new();
        let mut status = Status::clear();
        if let Some(id) = id {
            status = status.id(id);
        }
        status.write_to(&mut output)?;
        let Some(Event::Report(value)) = parse_sequence(&output)? else {
            return Err("expected report".into());
        };
        assert_eq!(value.action, Action::Clear);
        assert_eq!(value.id, id);
    }
    Ok(())
}

#[test]
fn invalid_text_is_rejected_before_any_output() {
    for text in ["a\nb", "\u{0}", "\u{7f}", "\u{9f}"] {
        let mut output = Vec::new();
        assert!(matches!(
            Status::new(State::Done).message(text).write_to(&mut output),
            Err(Error::ControlCharacter)
        ));
        assert_eq!(output, [] as [u8; 0]);
    }
    let text = "x".repeat(2049);
    let mut output = Vec::new();
    assert!(matches!(
        Status::new(State::Done)
            .message(&text)
            .write_to(&mut output),
        Err(Error::TextTooLong)
    ));
    assert_eq!(output, [] as [u8; 0]);
    let text = "x".repeat(193);
    assert!(matches!(
        Status::new(State::Done).title(&text).write_to(&mut output),
        Err(Error::TextTooLong)
    ));
    assert_eq!(output, [] as [u8; 0]);
}

#[test]
fn incompatible_fields_are_rejected_before_output() -> TestResult {
    let mut output = Vec::new();
    assert!(matches!(
        Status::new(State::Working)
            .kind(Kind::Auth)
            .write_to(&mut output),
        Err(Error::IncompatibleField)
    ));
    assert!(matches!(
        Status::new(State::Done)
            .progress(Progress::new(42)?)
            .write_to(&mut output),
        Err(Error::IncompatibleField)
    ));
    assert_eq!(output, [] as [u8; 0]);
    Ok(())
}

#[test]
fn maximum_legal_producer_report_fits_and_round_trips() -> TestResult {
    let message = "m".repeat(2048);
    let title = "t".repeat(192);
    let app = "a".repeat(32);
    let id = format!(
        "{}/{}/{}/{}",
        "a".repeat(32),
        "b".repeat(32),
        "c".repeat(32),
        "d".repeat(29)
    );
    let status = Status::new(State::Blocked)
        .app(AppId::new(&app)?)
        .id(RecordId::new(&id)?)
        .kind(Kind::Permission)
        .progress(Progress::new(100)?)
        .title(&title)
        .message(&message);
    let mut frame = [0; 4096];
    let mut cursor = io::Cursor::new(frame.as_mut_slice());
    let mut result = Ok(());
    let info = allocation_counter::measure(|| result = status.write_to(&mut cursor));
    result?;
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
    let length = usize::try_from(cursor.position())?;
    assert_eq!(length, 3220);
    let Some(Event::Report(value)) = parse_sequence(&frame[..length])? else {
        return Err("expected report".into());
    };
    assert_eq!(value.message.ok_or("missing message")?.decoded_len(), 2048);
    assert_eq!(value.title.ok_or("missing title")?.decoded_len(), 192);
    Ok(())
}

#[test]
fn optional_query_has_the_correct_wire_format() -> TestResult {
    let mut output = Vec::new();
    write_query(&mut output)?;
    assert_eq!(output, b"\x1b]7501;?\x1b\\");
    assert_eq!(parse_sequence(&output), Ok(Some(Event::Query)));
    Ok(())
}

struct FlushTrackingWriter {
    output: io::Cursor<[u8; 64]>,
    flushes: usize,
}

impl Write for FlushTrackingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.output.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
fn trait_object_emission_does_not_allocate_or_flush() -> TestResult {
    let mut writer = FlushTrackingWriter {
        output: io::Cursor::new([0; 64]),
        flushes: 0,
    };
    let output: &mut dyn Write = &mut writer;
    let mut status_result = Ok(());
    let mut query_result = Ok(());
    let info = allocation_counter::measure(|| {
        status_result = Status::new(State::Done).message("OK").write_to(output);
        query_result = write_query(output);
    });
    status_result?;
    query_result?;
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
    let length = usize::try_from(writer.output.position())?;
    assert_eq!(
        &writer.output.get_ref()[..length],
        b"\x1b]7501;state=done:msg=T0s=\x1b\\\x1b]7501;?\x1b\\"
    );
    assert_eq!(writer.flushes, 0);
    let output: &mut dyn Write = &mut writer;
    output.flush()?;
    assert_eq!(writer.flushes, 1);
    Ok(())
}

struct FailingWriter;
impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("test failure"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn constructor_errors_propagate_without_allocating_or_writing() -> TestResult {
    fn emit(output: &mut dyn Write, app: &str, id: &str, progress: u8) -> Result<(), Error> {
        Status::new(State::Working)
            .app(AppId::new(app)?)
            .id(RecordId::new(id)?)
            .progress(Progress::new(progress)?)
            .write_to(output)
    }

    let mut output = Vec::new();
    for (app, id, progress, expected) in [
        ("", "build/test", 40, parser::Error::InvalidAppId),
        ("example", "", 40, parser::Error::InvalidRecordId),
        ("example", "build/test", 101, parser::Error::InvalidProgress),
    ] {
        let mut result = Ok(());
        let info = allocation_counter::measure(|| result = emit(&mut output, app, id, progress));
        let error = result.err().ok_or("expected constructor failure")?;
        assert!(matches!(error, Error::Parser(actual) if actual == expected));
        assert_eq!(error.to_string(), expected.to_string());
        assert_eq!(
            error
                .source()
                .ok_or("missing constructor error source")?
                .downcast_ref::<parser::Error>(),
            Some(&expected)
        );
        assert_eq!(info.count_total, 0);
        assert_eq!(info.bytes_max, 0);
        assert_eq!(output, [] as [u8; 0]);
    }
    emit(&mut output, "example", "build/test", 40)?;
    let Some(Event::Report(report)) = parse_sequence(&output)? else {
        return Err("expected report".into());
    };
    assert_eq!(report.progress.ok_or("missing progress")?.get(), 40);
    Ok(())
}

#[test]
fn io_failures_are_returned_to_the_caller() -> TestResult {
    let error = Status::new(State::Idle)
        .write_to(&mut FailingWriter)
        .err()
        .ok_or("expected I/O failure")?;
    assert!(matches!(error, Error::Io(_)));
    assert_eq!(error.to_string(), "test failure");
    let source = error.source().ok_or("missing I/O error source")?;
    assert!(source.is::<io::Error>());
    assert_eq!(source.to_string(), "test failure");
    Ok(())
}
