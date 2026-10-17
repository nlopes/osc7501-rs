//! Fresh complete reports with repeated producer metadata.

use osc7501::{
    AppId, Context, Error, Kind, Progress, RecordId, State, Status,
    parser::{Action, Event, MAX_TITLE, Report, parse_sequence},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn encode(status: Status<'_>) -> Result<Vec<u8>, Error> {
    let mut output = Vec::new();
    status.write_to(&mut output)?;
    Ok(output)
}

fn report(bytes: &[u8]) -> Result<Report<'_>, Box<dyn std::error::Error>> {
    match parse_sequence(bytes)? {
        Some(Event::Report(report)) => Ok(report),
        _ => Err("expected report".into()),
    }
}

#[test]
fn repeated_reports_keep_metadata_and_drop_transient_fields() -> TestResult {
    let app = AppId::new("cargo")?;
    let id = RecordId::new("build/tests")?;
    let context = Context::new().app(app).id(id).title("Tests 🦀");
    let bytes = encode(
        context
            .status(State::Blocked)
            .message("Continue?")
            .kind(Kind::Permission)
            .progress(Progress::new(40)?),
    )?;
    let first = report(&bytes)?;
    assert!(first.message().is_some());
    assert_eq!(first.kind(), Some(Kind::Permission));
    assert_eq!(first.progress(), Some(Progress::new(40)?));

    for state in [
        State::Working,
        State::Blocked,
        State::Done,
        State::Error,
        State::Idle,
    ] {
        let bytes = encode(context.status(state))?;
        let fresh = report(&bytes)?;
        assert_eq!(fresh.action(), Action::Set(state));
        assert_eq!(fresh.app(), Some(app));
        assert_eq!(fresh.id(), Some(id));
        let mut decoded = [0; MAX_TITLE];
        assert_eq!(
            fresh
                .title()
                .ok_or("missing title")?
                .decode_into(&mut decoded)?,
            "Tests 🦀"
        );
        assert_eq!(fresh.message(), None);
        assert_eq!(fresh.progress(), None);
        assert_eq!(fresh.kind(), None);
    }
    Ok(())
}

#[test]
fn absent_and_empty_context_fields_remain_distinct() -> TestResult {
    for context in [Context::new(), Context::default()] {
        assert_eq!(
            encode(context.status(State::Done))?,
            b"\x1b]7501;state=done\x1b\\"
        );
    }
    let bytes = encode(Context::new().title("").status(State::Done))?;
    let value = report(&bytes)?;
    assert_eq!(value.title().ok_or("missing empty title")?.decoded_len(), 0);
    assert_eq!(value.app(), None);
    assert_eq!(value.id(), None);

    let id = RecordId::new("build")?;
    let bytes = encode(Context::new().id(id).status(State::Working))?;
    let value = report(&bytes)?;
    assert_eq!(value.id(), Some(id));
    assert_eq!(value.app(), None);
    assert_eq!(value.title(), None);
    Ok(())
}

#[test]
fn report_overrides_do_not_change_the_context() -> TestResult {
    let app = AppId::new("cargo")?;
    let id = RecordId::new("build")?;
    let other_app = AppId::new("rustdoc")?;
    let other_id = RecordId::new("docs")?;
    let context = Context::new().app(app).id(id).title("Build");
    let expected = encode(context.status(State::Done))?;
    let bytes = encode(
        context
            .status(State::Done)
            .app(other_app)
            .id(other_id)
            .title("Docs"),
    )?;
    let value = report(&bytes)?;
    assert_eq!(value.app(), Some(other_app));
    assert_eq!(value.id(), Some(other_id));
    let mut decoded = [0; MAX_TITLE];
    assert_eq!(
        value
            .title()
            .ok_or("missing title")?
            .decode_into(&mut decoded)?,
        "Docs"
    );
    assert_eq!(encode(context.status(State::Done))?, expected);
    Ok(())
}

#[test]
fn reports_borrow_the_fields_without_borrowing_the_context() -> TestResult {
    const ROOT: Context<'static> = Context::new().title("Build");

    let title = String::from("Build");
    let status = {
        let context = Context::new().title(&title);
        context.status(State::Working)
    };
    let message = String::from("Compiling");
    let bytes = encode(status.message(&message))?;
    let value = report(&bytes)?;
    let mut decoded = [0; MAX_TITLE];
    assert_eq!(
        value
            .title()
            .ok_or("missing title")?
            .decode_into(&mut decoded)?,
        title
    );
    assert_eq!(
        value
            .message()
            .ok_or("missing message")?
            .decode_into(&mut decoded)?,
        message
    );

    let bytes = encode(ROOT.status(State::Working).message(&message))?;
    assert_eq!(
        report(&bytes)?
            .message()
            .ok_or("missing message")?
            .decode_into(&mut decoded)?,
        message
    );
    Ok(())
}

#[test]
fn context_reports_validate_before_writing() -> TestResult {
    let too_long = "x".repeat(MAX_TITLE + 1);
    for title in ["line\nbreak", too_long.as_str()] {
        let context = Context::new().title(title);
        let mut output = Vec::new();
        let error = context
            .status(State::Done)
            .write_to(&mut output)
            .expect_err("invalid title");
        assert!(matches!(
            error,
            Error::ControlCharacter | Error::TextTooLong
        ));
        assert_eq!(output, []);
        context
            .status(State::Done)
            .title("Fixed")
            .write_to(&mut output)?;
        assert!(report(&output)?.title().is_some());
    }
    let context = Context::new().title("Build");
    for status in [
        context.status(State::Working).kind(Kind::Auth),
        context.status(State::Done).progress(Progress::new(50)?),
    ] {
        let mut output = Vec::new();
        assert!(matches!(
            status.write_to(&mut output),
            Err(Error::IncompatibleField)
        ));
        assert_eq!(output, []);
    }
    Ok(())
}

#[test]
fn context_creation_and_reuse_do_not_allocate() -> TestResult {
    let app = AppId::new("cargo")?;
    let id = RecordId::new("build/tests")?;
    let mut output = std::io::Cursor::new([0; 1024]);
    let mut result = Ok(());
    let allocations = allocation_counter::measure(|| {
        let context = Context::new().app(app).id(id).title("Tests 🦀");
        result = context
            .status(State::Working)
            .message("Running")
            .write_to(&mut output)
            .and_then(|()| context.status(State::Done).write_to(&mut output));
    });
    result?;
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_max, 0);
    assert!(output.position() > 0);
    Ok(())
}
