//! Producer round trips preserve field meaning rather than input spelling.
use hegel::{TestCase, generators as gs};
use osc7501::{
    AppId, Kind, Progress, RecordId, State, Status,
    parser::{self, Action, EncodedText, Event, parse_sequence},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn decode(text: Option<EncodedText<'_>>) -> Result<Option<String>, parser::Error> {
    text.map(|text| {
        let mut bytes = vec![0; text.decoded_len()];
        text.decode_into(&mut bytes).map(str::to_owned)
    })
    .transpose()
}

#[hegel::test]
fn generated_statuses_preserve_all_fields_after_encoding(tc: TestCase) {
    let result = check_generated_status(&tc);
    assert!(result.is_ok(), "{result:?}");
}

#[hegel::test_helper]
fn check_generated_status(tc: &TestCase) -> TestResult {
    let action = tc.draw(gs::sampled_from(&[
        Action::Clear,
        Action::Set(State::Idle),
        Action::Set(State::Working),
        Action::Set(State::Blocked),
        Action::Set(State::Done),
        Action::Set(State::Error),
    ]));
    let id = tc.draw(gs::optional(gs::sampled_from(&[
        "a",
        "a/b",
        "release/test",
    ])));
    let app = tc.draw(gs::optional(gs::sampled_from(&[
        "cargo", "my-app", "test_1",
    ])));
    let title = tc.draw(gs::optional(gs::from_regex("[ -~é🦀]{0,32}")));
    let message = tc.draw(gs::optional(gs::from_regex("[ -~é🦀]{0,64}")));
    let kind = tc.draw(gs::optional(gs::sampled_from(&[
        Kind::Auth,
        Kind::Permission,
        Kind::Question,
    ])));
    let progress = tc.draw(gs::optional(gs::integers::<u8>().max_value(100)));
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
        return Err("expected report".into());
    };
    assert_eq!(report.action(), action);
    assert_eq!(report.id(), id);
    assert_eq!(report.app(), app);
    assert_eq!(report.kind(), kind);
    assert_eq!(report.progress(), progress);
    assert_eq!(decode(report.title())?, title);
    assert_eq!(decode(report.message())?, message);
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
