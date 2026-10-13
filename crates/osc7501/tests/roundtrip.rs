//! Producer-to-parser conformance and validation before writes.
use osc7501::{AppId, Kind, Progress, RecordId, SendError, State, Status, write_query};
use osc7501_parser::{Action, Event, MAX_MESSAGE, parse_sequence};
use std::io::{self, Write};

#[test]
fn emitter_round_trips_every_state() {
    let app = AppId::new("example").unwrap();
    let id = RecordId::new("build/test").unwrap();
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
            .write_to(&mut output)
            .unwrap();
        let Some(Event::Report(value)) = parse_sequence(&output).unwrap() else {
            panic!("expected report");
        };
        assert_eq!(value.action, Action::Set(state));
        assert_eq!(value.id, Some(id));
        assert_eq!(value.app, Some(app));
        let mut decoded = [0; MAX_MESSAGE];
        assert_eq!(
            value.message.unwrap().decode_into(&mut decoded).unwrap(),
            "España"
        );
        assert_eq!(
            value.title.unwrap().decode_into(&mut decoded).unwrap(),
            "Test 🦀"
        );
    }
}

#[test]
fn emitter_round_trips_blocking_reason_and_progress() {
    let mut output = Vec::new();
    Status::new(State::Blocked)
        .kind(Kind::Permission)
        .progress(Progress::new(100).unwrap())
        .write_to(&mut output)
        .unwrap();
    let Some(Event::Report(value)) = parse_sequence(&output).unwrap() else {
        panic!("expected report");
    };
    assert_eq!(value.kind, Some(Kind::Permission));
    assert_eq!(value.progress.unwrap().get(), 100);
}

#[test]
fn root_clear_and_subtree_clear_round_trip() {
    for id in [None, RecordId::new("build/test")] {
        let mut output = Vec::new();
        let mut status = Status::clear();
        if let Some(id) = id {
            status = status.id(id);
        }
        status.write_to(&mut output).unwrap();
        let Some(Event::Report(value)) = parse_sequence(&output).unwrap() else {
            panic!("expected report");
        };
        assert_eq!(value.action, Action::Clear);
        assert_eq!(value.id, id);
    }
}

#[test]
fn invalid_text_is_rejected_before_any_output() {
    for text in ["a\nb", "\u{0}", "\u{7f}", "\u{9f}"] {
        let mut output = Vec::new();
        assert!(matches!(
            Status::new(State::Done).message(text).write_to(&mut output),
            Err(SendError::ControlCharacter)
        ));
        assert_eq!(output, [] as [u8; 0]);
    }
    let text = "x".repeat(2049);
    let mut output = Vec::new();
    assert!(matches!(
        Status::new(State::Done)
            .message(&text)
            .write_to(&mut output),
        Err(SendError::TextTooLong)
    ));
    assert_eq!(output, [] as [u8; 0]);
    let text = "x".repeat(193);
    assert!(matches!(
        Status::new(State::Done).title(&text).write_to(&mut output),
        Err(SendError::TextTooLong)
    ));
    assert_eq!(output, [] as [u8; 0]);
}

#[test]
fn incompatible_fields_are_rejected_before_output() {
    let mut output = Vec::new();
    assert!(matches!(
        Status::new(State::Working)
            .kind(Kind::Auth)
            .write_to(&mut output),
        Err(SendError::IncompatibleField)
    ));
    assert!(matches!(
        Status::new(State::Done)
            .progress(Progress::new(42).unwrap())
            .write_to(&mut output),
        Err(SendError::IncompatibleField)
    ));
    assert_eq!(output, [] as [u8; 0]);
}

#[test]
fn maximum_legal_producer_report_fits_and_round_trips() {
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
        .app(AppId::new(&app).unwrap())
        .id(RecordId::new(&id).unwrap())
        .kind(Kind::Permission)
        .progress(Progress::new(100).unwrap())
        .title(&title)
        .message(&message);
    let mut frame = [0; 4096];
    let mut cursor = io::Cursor::new(frame.as_mut_slice());
    let info = allocation_counter::measure(|| {
        status.write_to(&mut cursor).unwrap();
    });
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
    let length = usize::try_from(cursor.position()).unwrap();
    assert!(length < 3300);
    let Some(Event::Report(value)) = parse_sequence(&frame[..length]).unwrap() else {
        panic!("expected report");
    };
    assert_eq!(value.message.unwrap().decoded_len(), 2048);
    assert_eq!(value.title.unwrap().decoded_len(), 192);
}

#[test]
fn optional_query_has_the_correct_wire_format() {
    let mut output = Vec::new();
    write_query(&mut output).unwrap();
    assert_eq!(output, b"\x1b]7501;?\x1b\\");
    assert_eq!(parse_sequence(&output), Ok(Some(Event::Query)));
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
fn io_failures_are_returned_to_the_caller() {
    assert!(matches!(
        Status::new(State::Idle).write_to(&mut FailingWriter),
        Err(SendError::Io(_))
    ));
}
