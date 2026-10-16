//! Producer round trips preserve field meaning rather than input spelling.
use osc7501::{
    AppId, Kind, Progress, RecordId, State, Status,
    parser::{self, Action, EncodedText, Event, parse_sequence},
};
use proptest::{
    option,
    prelude::*,
    sample::select,
    test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn decode(text: Option<EncodedText<'_>>) -> Result<Option<String>, parser::Error> {
    text.map(|text| {
        let mut bytes = vec![0; text.decoded_len()];
        text.decode_into(&mut bytes).map(str::to_owned)
    })
    .transpose()
}

#[test]
fn generated_statuses_preserve_all_fields_after_encoding() -> TestResult {
    let strategy = (
        select(vec![
            Action::Clear,
            Action::Set(State::Idle),
            Action::Set(State::Working),
            Action::Set(State::Blocked),
            Action::Set(State::Done),
            Action::Set(State::Error),
        ]),
        option::of(select(vec!["a", "a/b", "release/test"])),
        option::of(select(vec!["cargo", "my-app", "test_1"])),
        option::of("[ -~é🦀]{0,32}"),
        option::of("[ -~é🦀]{0,64}"),
        option::of(select(vec![Kind::Auth, Kind::Permission, Kind::Question])),
        option::of(0..=100_u8),
    );
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    runner.run(
        &strategy,
        |(action, id, app, title, message, kind, progress)| {
            let mut status = match action {
                Action::Set(state) => Status::new(state),
                Action::Clear => Status::clear(),
            };
            let id = id.map(RecordId::new).transpose()?;
            let app = app.map(AppId::new).transpose()?;
            let kind = kind.filter(|_| action == Action::Set(State::Blocked));
            let progress = progress
                .filter(|_| matches!(action, Action::Set(State::Working | State::Blocked)))
                .map(Progress::new)
                .transpose()?;
            if let Some(id) = id {
                status = status.id(id);
            }
            if let Some(app) = app {
                status = status.app(app);
            }
            if let Some(title) = &title {
                status = status.title(title);
            }
            if let Some(message) = &message {
                status = status.message(message);
            }
            if let Some(kind) = kind {
                status = status.kind(kind);
            }
            if let Some(progress) = progress {
                status = status.progress(progress);
            }
            let mut bytes = Vec::new();
            status.write_to(&mut bytes)?;
            let Some(Event::Report(report)) = parse_sequence(&bytes)? else {
                return Err(proptest::test_runner::TestCaseError::fail(
                    "expected report",
                ));
            };
            prop_assert_eq!(report.action(), action);
            prop_assert_eq!(report.id(), id);
            prop_assert_eq!(report.app(), app);
            prop_assert_eq!(report.kind(), kind);
            prop_assert_eq!(report.progress(), progress);
            prop_assert_eq!(decode(report.title())?, title);
            prop_assert_eq!(decode(report.message())?, message);
            Ok(())
        },
    )?;
    Ok(())
}

#[test]
fn normalizing_an_accepted_report_preserves_meaning_and_stabilizes() -> TestResult {
    let input = b"\x1b]7501;msg=T0s:future=value:state=done:id=task:title=:app=cargo\x07";
    let Some(Event::Report(original)) = parse_sequence(input)? else {
        return Err("expected report".into());
    };
    let title = decode(original.title())?;
    let message = decode(original.message())?;
    let status = Status::new(State::Done)
        .id(original.id().ok_or("missing id")?)
        .app(original.app().ok_or("missing app")?)
        .title(title.as_deref().ok_or("missing title")?)
        .message(message.as_deref().ok_or("missing message")?);
    let mut canonical = Vec::new();
    status.write_to(&mut canonical)?;
    assert_ne!(canonical, input);
    let Some(Event::Report(parsed)) = parse_sequence(&canonical)? else {
        return Err("expected canonical report".into());
    };
    assert_eq!(parsed.action(), original.action());
    assert_eq!(parsed.id(), original.id());
    assert_eq!(parsed.app(), original.app());
    assert_eq!(decode(parsed.title())?, title);
    assert_eq!(decode(parsed.message())?, message);
    let title = decode(parsed.title())?;
    let message = decode(parsed.message())?;
    let mut encoded_again = Vec::new();
    Status::new(State::Done)
        .id(parsed.id().ok_or("missing id")?)
        .app(parsed.app().ok_or("missing app")?)
        .title(title.as_deref().ok_or("missing title")?)
        .message(message.as_deref().ok_or("missing message")?)
        .write_to(&mut encoded_again)?;
    assert_eq!(encoded_again, canonical);
    Ok(())
}
