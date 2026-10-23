//! Generated wire inputs checked against independent text and framing oracles.
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use hegel::{Generator as _, TestCase, generators as gs};
use osc7501_parser::{
    Action, Decoder, Error, Event, Kind, Limit, MAX_MESSAGE, MAX_MESSAGE_ENCODED, MAX_SEQUENCE,
    MAX_TITLE, MAX_TITLE_ENCODED, Progress, Report, State, parse_body, parse_sequence,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Observation = Result<String, Error>;

fn report(body: &[u8]) -> Result<Report<'_>, Box<dyn std::error::Error>> {
    match parse_body(body)? {
        Some(Event::Report(report)) => Ok(report),
        _ => Err("expected report".into()),
    }
}

fn observe(event: Result<Event<'_>, Error>) -> Observation {
    event.map(|event| format!("{event:?}"))
}

fn frame(body: &[u8], bel: bool) -> Vec<u8> {
    [
        b"\x1b]7501;".as_slice(),
        body,
        if bel { b"\x07" } else { b"\x1b\\" },
    ]
    .concat()
}

fn decoded(encoded: &str, cap: usize, encoded_cap: usize, limit: Limit) -> Result<Vec<u8>, Error> {
    if encoded.len() > encoded_cap {
        return Err(Error::Limit(limit));
    }
    let bytes = STANDARD
        .decode(encoded)
        .or_else(|_| STANDARD_NO_PAD.decode(encoded))
        .map_err(|_| Error::InvalidBase64)?;
    if bytes.len() > cap {
        return Err(Error::Limit(limit));
    }
    let text = str::from_utf8(&bytes).map_err(|_| Error::InvalidUtf8)?;
    if text.chars().any(char::is_control) {
        return Err(Error::ControlCharacter);
    }
    Ok(bytes)
}

fn check_text(encoded: &str) -> TestResult {
    for (key, cap, encoded_cap, limit) in [
        ("title", MAX_TITLE, MAX_TITLE_ENCODED, Limit::Title),
        ("msg", MAX_MESSAGE, MAX_MESSAGE_ENCODED, Limit::Message),
    ] {
        let body = format!("state=done:{key}={encoded}");
        let expected = decoded(encoded, cap, encoded_cap, limit);
        match expected {
            Err(error) => assert_eq!(parse_body(body.as_bytes()), Err(error)),
            Ok(bytes) => {
                let value = report(body.as_bytes())?;
                let text = if key == "title" {
                    value.title()
                } else {
                    value.message()
                }
                .ok_or("missing text")?;
                assert_eq!(text.as_base64(), encoded);
                assert_eq!(text.decoded_len(), bytes.len());
                let mut output = vec![0xa5; bytes.len() + 2];
                assert_eq!(
                    text.decode_into(&mut output[1..=bytes.len()])?.as_bytes(),
                    bytes
                );
                assert_eq!(output[0], 0xa5);
                assert_eq!(output[bytes.len() + 1], 0xa5);
                if !bytes.is_empty() {
                    assert_eq!(
                        text.decode_into(&mut output[1..bytes.len()]),
                        Err(Error::BufferTooSmall)
                    );
                }
            }
        }
    }
    Ok(())
}

#[hegel::test]
fn text_validation_matches_the_base64_and_utf8_oracles(tc: TestCase) {
    let bytes = tc.draw(hegel::one_of![
        gs::binary().max_size(256),
        gs::text().max_size(100).map(String::into_bytes),
    ]);
    for encoded in [STANDARD.encode(&bytes), STANDARD_NO_PAD.encode(&bytes)] {
        let result = check_text(&encoded);
        assert!(result.is_ok(), "{result:?}");
    }
}

#[hegel::test]
fn malformed_base64_matches_the_reference_decoder(tc: TestCase) {
    let encoded = tc.draw(
        gs::text()
            .alphabet("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=-_,.")
            .max_size(100),
    );
    let result = check_text(&encoded);
    assert!(result.is_ok(), "{result:?}");
}

#[hegel::test]
fn decoded_byte_limits_hold_for_multibyte_text_and_both_padding_forms(tc: TestCase) {
    let token = tc.draw(gs::sampled_from(&["x", "é", "€", "🦀"]));
    let cap = tc.draw(gs::sampled_from(&[MAX_TITLE, MAX_MESSAGE]));
    let size = tc.draw(
        gs::integers::<usize>()
            .min_value(cap - 4)
            .max_value(cap + 4),
    );
    let text = format!(
        "{}{}",
        token.repeat(size / token.len()),
        "x".repeat(size % token.len())
    );
    for encoded in [
        STANDARD.encode(text.as_bytes()),
        STANDARD_NO_PAD.encode(text.as_bytes()),
    ] {
        let result = check_text(&encoded);
        assert!(result.is_ok(), "{result:?}");
    }
}

#[hegel::test]
fn invalid_text_cannot_be_hidden_by_later_duplicates_or_ignored_reports(tc: TestCase) {
    let invalid = tc.draw(gs::sampled_from(&[
        ("A", Error::InvalidBase64),
        ("AB==", Error::InvalidBase64),
        ("/w==", Error::InvalidUtf8),
        ("AA==", Error::ControlCharacter),
        ("wp8=", Error::ControlCharacter),
    ]));
    let key = tc.draw(gs::sampled_from(&["title", "msg"]));
    let state = tc.draw(gs::sampled_from(&["done", "clear", "future", ""]));
    let id = tc.draw(gs::sampled_from(&["task", "bad!", ""]));
    let body = format!("state={state}:id={id}:{key}={}:{key}=T0s=", invalid.0);
    assert_eq!(parse_body(body.as_bytes()), Err(invalid.1));
}

#[hegel::test]
fn field_order_and_malformed_duplicates_preserve_report_meaning(tc: TestCase) {
    let result = check_field_order(&tc);
    assert!(result.is_ok(), "{result:?}");
}

fn check_field_order(tc: &TestCase) -> TestResult {
    let state = tc.draw(gs::sampled_from(&[
        State::Idle,
        State::Working,
        State::Blocked,
        State::Done,
        State::Error,
    ]));
    let percentage = tc.draw(gs::integers::<u8>().max_value(100));
    let fields = vec![
        format!("state={}", state.as_str()),
        format!("progress={percentage}"),
        "id=build/test".into(),
        "app=cargo".into(),
        "kind=auth".into(),
        "title=8J+mgA==".into(),
        "msg=T0s=".into(),
        "future=a=b/c_-+".into(),
    ];
    let original = fields.join(":");
    let shuffled = tc.draw(gs::permutations(fields));
    let padding = tc.draw(gs::sampled_from(&["", " ", "\t\r\n"]));
    let shuffled = shuffled
        .iter()
        .map(|pair| pair.replacen('=', &format!("{padding}={padding}"), 1))
        .collect::<Vec<_>>()
        .join(":");
    // Malformed values must not override an earlier syntactically valid pair.
    let body = format!("{shuffled}:state=bad!:id=build/test:app=bad!:progress=bad!:kind=bad!");
    let value = report(body.as_bytes())?;
    assert_eq!(value, report(original.as_bytes())?);
    assert_eq!(value.action(), Action::Set(state));
    assert_eq!(
        value.kind(),
        (state == State::Blocked).then_some(Kind::Auth)
    );
    assert_eq!(
        value.progress().map(Progress::get),
        matches!(state, State::Working | State::Blocked).then_some(percentage)
    );
    Ok(())
}

#[hegel::test]
fn arbitrary_bodies_have_equivalent_framed_results(tc: TestCase) {
    let body = tc.draw(gs::binary().max_size(MAX_SEQUENCE - 9));
    let expected = if body.iter().any(|b| matches!(b, 0x1b | 0x07)) {
        Err(Error::Framing)
    } else {
        parse_body(&body)
    };
    for bel in [false, true] {
        let bytes = frame(&body, bel);
        assert_eq!(parse_sequence(&bytes), expected);
    }
}

#[hegel::test]
fn streaming_matches_direct_parsing_across_chunks_stops_and_oversize_recovery(tc: TestCase) {
    let result = check_stream(&tc);
    assert!(result.is_ok(), "{result:?}");
}

fn check_stream(tc: &TestCase) -> TestResult {
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(16));
    let mut input = Vec::new();
    let mut expected = Vec::new();
    for _ in 0..count {
        let body = tc.draw(hegel::one_of![
            gs::sampled_from(&[
                "?",
                "?:states=done,future:kinds=",
                "state=working:progress=42",
                "state=blocked:id=a/b:kind=auth",
                "state=future",
                "state=done:id=bad!",
                "state=done:msg=AA==",
                "state=done:msg=8J+mgA==",
                "state=done:msg=AB==",
                "state=done:msg=/w==",
                "state=done:future=\x1bx",
            ])
            .map(str::to_owned),
            gs::integers::<usize>()
                .min_value(MAX_SEQUENCE - 30)
                .max_value(MAX_SEQUENCE + 3)
                .map(|length| format!("state=done:future={}", "x".repeat(length))),
            gs::text()
                .alphabet("state=done:msg?_/+x,\t\r\n")
                .max_size(100),
        ]);
        let bytes = frame(body.as_bytes(), tc.draw(gs::booleans()));
        match parse_sequence(&bytes) {
            Ok(None) => {}
            Ok(Some(event)) => expected.push(observe(Ok(event))),
            Err(error) => expected.push(Err(error)),
        }
        input.extend_from_slice(&bytes);
        input.extend_from_slice(b"ordinary output\x1b]0;unrelated\x07");
    }
    input.extend_from_slice(b"\x1b]7501;state=done\x07");
    expected.push(observe(Ok(Event::Report(report(b"state=done")?))));
    let mut decoder = Decoder::new();
    let mut actual = Vec::new();
    let mut offset = 0;
    while offset < input.len() {
        let size = tc.draw(gs::integers::<usize>().min_value(1).max_value(512));
        let end = (offset + size).min(input.len());
        let pause = tc.draw(gs::booleans());
        decoder.feed(&[], |event| actual.push(observe(event)));
        let mut remaining = &input[offset..end];
        loop {
            match decoder.try_feed(remaining, |event| {
                actual.push(observe(event));
                if pause { Err(()) } else { Ok(()) }
            }) {
                Ok(()) => break,
                Err(stopped) => {
                    assert!((1..=remaining.len()).contains(&stopped.consumed()));
                    remaining = &remaining[stopped.consumed()..];
                }
            }
        }
        offset = end;
    }
    assert_eq!(actual, expected);
    Ok(())
}

#[hegel::test]
fn sequence_cap_includes_the_selected_terminator(tc: TestCase) {
    let size = tc.draw(
        gs::integers::<usize>()
            .min_value(MAX_SEQUENCE - 12)
            .max_value(MAX_SEQUENCE + 2),
    );
    let body = format!("state=done:future={}", "x".repeat(size - 18));
    for bel in [false, true] {
        let bytes = frame(body.as_bytes(), bel);
        if bytes.len() > MAX_SEQUENCE {
            assert_eq!(parse_sequence(&bytes), Err(Error::Limit(Limit::Sequence)));
        } else {
            assert!(
                matches!(parse_sequence(&bytes), Ok(Some(Event::Report(value)))
                if value.action() == Action::Set(State::Done))
            );
        }
    }
}

#[hegel::test]
fn capability_lists_preserve_known_tokens_and_skip_unknowns(tc: TestCase) {
    let states = tc.draw(
        gs::vecs(gs::sampled_from(&[
            "idle", "working", "blocked", "done", "error", "future", "", "DONE",
        ]))
        .max_size(20),
    );
    let kinds = tc.draw(
        gs::vecs(gs::sampled_from(&[
            "auth",
            "permission",
            "question",
            "future",
            "",
            "AUTH",
        ]))
        .max_size(20),
    );
    let body = format!(
        "?:states=idle:states={}:kinds=auth:kinds={}",
        states.join(","),
        kinds.join(",")
    );
    let parsed = parse_body(body.as_bytes());
    assert!(matches!(parsed, Ok(Some(Event::Capabilities(_)))));
    if let Ok(Some(Event::Capabilities(value))) = parsed {
        let expected_states: Vec<_> = states
            .iter()
            .filter_map(|s| match *s {
                "idle" => Some(State::Idle),
                "working" => Some(State::Working),
                "blocked" => Some(State::Blocked),
                "done" => Some(State::Done),
                "error" => Some(State::Error),
                _ => None,
            })
            .collect();
        let expected_kinds: Vec<_> = kinds
            .iter()
            .filter_map(|s| match *s {
                "auth" => Some(Kind::Auth),
                "permission" => Some(Kind::Permission),
                "question" => Some(Kind::Question),
                _ => None,
            })
            .collect();
        assert_eq!(value.states().collect::<Vec<_>>(), expected_states);
        assert_eq!(value.kinds().collect::<Vec<_>>(), expected_kinds);
        for state in [
            State::Idle,
            State::Working,
            State::Blocked,
            State::Done,
            State::Error,
        ] {
            assert_eq!(
                value.supports_state(state),
                expected_states.contains(&state)
            );
        }
        for kind in [Kind::Auth, Kind::Permission, Kind::Question] {
            assert_eq!(value.supports_kind(kind), expected_kinds.contains(&kind));
        }
    }
}

#[hegel::test]
fn reset_discards_arbitrary_partial_stream_state(tc: TestCase) {
    let prefix = tc.draw(gs::sampled_from(&[
        "",
        "\x1b",
        "\x1b]75",
        "\x1b]0;ignored",
        "\x1b]0;ignored\x1b",
        "\x1b]7501;state=",
        "\x1b]7501;state=done\x1b",
    ]));
    let tail = tc.draw(gs::binary().max_size(MAX_SEQUENCE + 16));
    let cut = tc.draw(gs::integers::<usize>().max_value(tail.len()));
    let mut decoder = Decoder::new();
    decoder.feed(prefix.as_bytes(), |_| {});
    decoder.feed(&tail[..cut], |_| {});
    decoder.reset();
    let mut events = Vec::new();
    decoder.feed(b"\x1b]7501;?\x1b\\", |event| events.push(observe(event)));
    assert_eq!(events, [observe(Ok(Event::Query))]);
}
