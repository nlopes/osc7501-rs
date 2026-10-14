//! Revision 0.4 conformance, adversarial bounds, and independent base64 oracle.
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use osc7501_parser::{
    Action, AppId, Decoder, Error, Event, Kind, Limit, MAX_MESSAGE, Progress, RecordId, Report,
    State, parse_body, parse_sequence,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn report(body: &[u8]) -> TestResult<Report<'_>> {
    let Some(Event::Report(report)) = parse_body(body)? else {
        return Err("expected report".into());
    };
    Ok(report)
}

#[test]
fn recognizes_all_states_and_clear() -> TestResult {
    for state in [
        State::Idle,
        State::Working,
        State::Done,
        State::Blocked,
        State::Error,
    ] {
        let body = format!("state={}", state.as_str());
        assert_eq!(report(body.as_bytes())?.action(), Action::Set(state));
    }
    assert_eq!(report(b"state=clear")?.action(), Action::Clear);
    Ok(())
}

#[test]
fn recognizes_all_blocking_reasons() -> TestResult {
    for kind in [Kind::Permission, Kind::Question, Kind::Auth] {
        let body = format!("state=blocked:kind={}", kind.as_str());
        assert_eq!(report(body.as_bytes())?.kind(), Some(kind));
    }
    Ok(())
}

#[test]
fn supports_both_terminators() -> TestResult {
    for frame in [
        b"\x1b]7501;state=idle\x07".as_slice(),
        b"\x1b]7501;state=idle\x1b\\",
    ] {
        assert_eq!(
            parse_sequence(frame)?,
            Some(Event::Report(report(b"state=idle")?))
        );
    }
    Ok(())
}

#[test]
fn rejects_incomplete_wrong_and_multiple_frames() {
    for frame in [
        b"state=idle".as_slice(),
        b"\x1b]7502;state=idle\x07",
        b"\x1b]7501;state=idle",
        b"\x1b]7501;state=idle\x07extra\x07",
        b"\x1b]7501;state=idle\x07\x1b]7501;state=done\x07",
    ] {
        assert_eq!(parse_sequence(frame), Err(Error::Framing));
    }
}

#[test]
fn missing_and_unknown_states_are_ignored() {
    for body in [b"app=cargo".as_slice(), b"", b"state=unknown", b"state="] {
        assert_eq!(parse_body(body), Ok(None));
    }
}

#[test]
fn last_syntactically_valid_pair_wins() -> TestResult {
    let value = report(b"state=done:state=blocked:progress=25:progress=80:app=cargo:app=rustc:kind=auth:kind=question")?;
    assert_eq!(value.action(), Action::Set(State::Blocked));
    assert_eq!(value.progress().ok_or("missing progress")?.get(), 80);
    assert_eq!(
        value.app().ok_or("missing application id")?.as_str(),
        "rustc"
    );
    assert_eq!(value.kind(), Some(Kind::Question));
    assert_eq!(parse_body(b"state=idle:state=future"), Ok(None));
    assert_eq!(
        report(b"state=idle:state=bad!")?.action(),
        Action::Set(State::Idle)
    );
    Ok(())
}

#[test]
fn malformed_pairs_do_not_stop_valid_pairs() -> TestResult {
    let value = report(b"broken:=empty:STATE=error:state=working:bad=has space:progress=40:msg=bad!value:app=cargo")?;
    assert_eq!(value.action(), Action::Set(State::Working));
    assert_eq!(value.progress().ok_or("missing progress")?.get(), 40);
    assert!(value.message().is_none());
    assert_eq!(
        value.app().ok_or("missing application id")?.as_str(),
        "cargo"
    );
    Ok(())
}

#[test]
fn trims_ascii_whitespace_around_keys_and_values() -> TestResult {
    let value = report(b" \tstate \r= working \n: progress = 42 : app = cargo ")?;
    assert_eq!(value.action(), Action::Set(State::Working));
    assert_eq!(value.progress().ok_or("missing progress")?.get(), 42);
    assert_eq!(
        value.app().ok_or("missing application id")?.as_str(),
        "cargo"
    );
    Ok(())
}

#[test]
fn unknown_keys_are_ignored_and_equals_is_part_of_value() -> TestResult {
    let value = report(b"future=a=b/c_-+:state=done:msg=T0s=")?;
    let mut output = [0; 2];
    assert_eq!(
        value
            .message()
            .ok_or("missing message")?
            .decode_into(&mut output)?,
        "OK"
    );
    Ok(())
}

#[test]
fn enforces_state_specific_fields() -> TestResult {
    for state in ["idle", "done", "error", "clear"] {
        let body = format!("state={state}:kind=permission:progress=20");
        let value = report(body.as_bytes())?;
        assert!(value.kind().is_none());
        assert!(value.progress().is_none());
    }
    assert!(report(b"state=working:kind=auth")?.kind().is_none());
    Ok(())
}

#[test]
fn progress_is_optional_and_strictly_bounded() -> TestResult {
    for value in ["0", "100", "00040"] {
        let body = format!("state=working:progress={value}");
        assert!(report(body.as_bytes())?.progress().is_some());
    }
    for value in ["", "101", "-1", "+1", "1.5", "2a", "999999999999"] {
        let body = format!("state=working:progress={value}");
        assert!(report(body.as_bytes())?.progress().is_none());
        let body = format!("state=working:progress=40:progress={value}");
        assert!(report(body.as_bytes())?.progress().is_none());
    }
    assert!(report(b"state=blocked")?.progress().is_none());
    assert_eq!(Progress::new(101), Err(Error::InvalidProgress));
    Ok(())
}

#[test]
fn unknown_kinds_and_invalid_apps_are_absent() -> TestResult {
    let value = report(b"state=blocked:kind=future:app=cargo/tool")?;
    assert!(value.kind().is_none());
    assert!(value.app().is_none());
    for app in ["", "cargo/tool", "cargo,tool", "cargo=tool"] {
        let body = format!("state=working:app=cargo:app={app}");
        assert!(report(body.as_bytes())?.app().is_none());
    }
    assert_eq!(AppId::new(""), Err(Error::InvalidAppId));
    assert_eq!(AppId::new("not an id"), Err(Error::InvalidAppId));
    assert!(AppId::new("a.b_c+d-e").is_ok());
    Ok(())
}

#[test]
fn invalid_id_never_falls_back_to_root() -> TestResult {
    for id in [
        "",
        "/",
        "build/",
        "/build",
        "build//test",
        "a=b",
        "a,b",
        "has space",
        "bad!id",
        "\u{ff}",
        "bad\tinside",
    ] {
        let body = format!("state=clear:id={id}");
        assert_eq!(parse_body(body.as_bytes()), Ok(None), "{id}");
    }
    assert_eq!(
        report(b"state=done:id=build/test")?
            .id()
            .ok_or("missing record id")?
            .as_str(),
        "build/test"
    );
    assert!(report(b"state=done")?.id().is_none());
    Ok(())
}

#[test]
fn identity_is_borrowed_and_a_later_update_has_no_retained_fields() -> TestResult {
    let bytes = b"state=working:id=build/test:app=cargo";
    let value = report(bytes)?;
    assert_eq!(
        value.id().ok_or("missing record id")?.as_str().as_ptr(),
        bytes[17..].as_ptr()
    );
    let next = report(b"state=done:id=build/test")?;
    assert!(next.app().is_none());
    assert!(next.message().is_none());
    Ok(())
}

#[test]
fn text_round_trips_unicode_and_optional_padding() -> TestResult {
    for text in [
        "",
        "f",
        "fo",
        "foo",
        "Hello, world!",
        "España 🦀 日本語",
        "a\u{202e}b",
    ] {
        for engine in [&STANDARD, &STANDARD_NO_PAD] {
            let encoded = engine.encode(text);
            let body = format!("state=done:msg={encoded}");
            let value = report(body.as_bytes())?
                .message()
                .ok_or("missing message")?;
            let mut output = [0; MAX_MESSAGE];
            assert_eq!(value.decoded_len(), text.len());
            assert_eq!(value.decode_into(&mut output)?, text);
        }
    }
    Ok(())
}

#[test]
fn rejects_bad_base64_padding_alphabet_and_trailing_bits() {
    for encoded in [
        "A", "=", "===", "Zg=", "Zg===", "=Zg=", "Zg==a", "Zg..", "Zh==", "Zm9=", "____", "----",
    ] {
        let body = format!("state=done:msg={encoded}");
        assert_eq!(
            parse_body(body.as_bytes()),
            Err(Error::InvalidBase64),
            "{encoded}"
        );
    }
}

#[test]
fn rejects_invalid_utf8() {
    for bytes in [
        vec![0xff],
        vec![0xc0, 0xaf],
        vec![0xed, 0xa0, 0x80],
        vec![0xe2, 0x82],
    ] {
        let body = format!("state=idle:msg={}", STANDARD.encode(bytes));
        assert_eq!(parse_body(body.as_bytes()), Err(Error::InvalidUtf8));
    }
}

#[test]
fn rejects_every_c0_del_and_c1_control_for_both_text_fields() -> TestResult {
    for code in (0..=31).chain(127..=159) {
        let text = char::from_u32(code)
            .ok_or("invalid control character code")?
            .to_string();
        for key in ["title", "msg"] {
            let body = format!("state=done:{key}={}", STANDARD.encode(&text));
            assert_eq!(parse_body(body.as_bytes()), Err(Error::ControlCharacter));
        }
    }
    Ok(())
}

#[test]
fn invalid_overridden_text_discards_entire_report() {
    for body in [
        b"state=done:msg=A:msg=T0s=".as_slice(),
        b"state=done:title=Cg==:title=T0s=",
        b"msg=/w==:state=future",
    ] {
        assert!(parse_body(body).is_err());
    }
}

#[test]
fn last_valid_text_wins_and_empty_text_is_present() -> TestResult {
    let value = report(b"state=done:msg=T0s=:msg=")?
        .message()
        .ok_or("missing message")?;
    assert_eq!(value.decoded_len(), 0);
    assert_eq!(value.decode_into(&mut [])?, "");
    Ok(())
}

#[test]
fn parsing_and_decoding_share_one_error_type() -> TestResult {
    fn decode_message<'a>(input: &[u8], output: &'a mut [u8]) -> Result<Option<&'a str>, Error> {
        let Some(Event::Report(report)) = parse_body(input)? else {
            return Ok(None);
        };
        report
            .message()
            .map(|text| text.decode_into(output))
            .transpose()
    }

    assert_eq!(
        decode_message(b"state=done:msg=T0s=", &mut [0; 2])?,
        Some("OK")
    );
    assert_eq!(
        decode_message(b"state=done:msg=T0s=", &mut [0; 1]),
        Err(Error::BufferTooSmall)
    );
    assert_eq!(
        decode_message(b"state=done:msg=A", &mut [0; 2]),
        Err(Error::InvalidBase64)
    );
    Ok(())
}

#[test]
fn decoding_requires_only_exact_destination_capacity() -> TestResult {
    let value = report(b"state=done:msg=T0s=")?
        .message()
        .ok_or("missing message")?;
    assert_eq!(value.decode_into(&mut [0; 1]), Err(Error::BufferTooSmall));
    assert_eq!(value.decode_into(&mut [0; 2])?, "OK");
    Ok(())
}

#[test]
fn accepts_text_at_both_decoded_limits() -> TestResult {
    let message = "m".repeat(2048);
    let title = "t".repeat(192);
    let body = format!(
        "state=working:title={}:msg={}",
        STANDARD.encode(&title),
        STANDARD.encode(&message)
    );
    let value = report(body.as_bytes())?;
    assert_eq!(value.title().ok_or("missing title")?.decoded_len(), 192);
    assert_eq!(
        value.message().ok_or("missing message")?.decoded_len(),
        2048
    );
    Ok(())
}

#[test]
fn checks_decoded_message_limit_independently_of_encoded_limit() {
    // 2049 bytes still encode to the permitted 2732 encoded bytes.
    let body = format!("state=done:msg={}", STANDARD.encode("x".repeat(2049)));
    assert_eq!(
        parse_body(body.as_bytes()),
        Err(Error::Limit(Limit::Message))
    );
}

#[test]
fn checks_every_key_limit_including_unknown_and_overridden_keys() {
    assert!(parse_body(b"state=idle:abcdefghijklmnop=x").is_ok());
    assert_eq!(
        parse_body(b"state=idle:abcdefghijklmnopq=x"),
        Err(Error::Limit(Limit::Key))
    );
}

#[test]
fn checks_every_known_encoded_limit_even_if_overridden() {
    for (key, cap, limit) in [
        ("msg", 2732, Limit::Message),
        ("title", 256, Limit::Title),
        ("app", 32, Limit::App),
        ("id", 128, Limit::Id),
    ] {
        let body = format!("state=idle:{key}={}:{key}=a", "a".repeat(cap + 1));
        assert_eq!(parse_body(body.as_bytes()), Err(Error::Limit(limit)));
    }
}

#[test]
fn enforces_id_depth_segment_and_total_limits() {
    let eight = "a/a/a/a/a/a/a/a";
    assert!(RecordId::new(eight).is_ok());
    assert_eq!(
        parse_body(b"state=idle:id=a/a/a/a/a/a/a/a/a"),
        Err(Error::Limit(Limit::Id))
    );
    let body = format!("state=idle:id={}", "a".repeat(33));
    assert_eq!(parse_body(body.as_bytes()), Err(Error::Limit(Limit::Id)));
    let maximum = format!(
        "{}/{}/{}/{}",
        "a".repeat(32),
        "b".repeat(32),
        "c".repeat(32),
        "d".repeat(29)
    );
    assert_eq!(maximum.len(), 128);
    assert!(RecordId::new(&maximum).is_ok());
}

#[test]
fn complete_sequence_limit_counts_prefix_and_each_terminator() -> TestResult {
    for terminator in ["\x07", "\x1b\\"] {
        let prefix = "\x1b]7501;state=idle:future=";
        let frame = format!(
            "{prefix}{}{terminator}",
            "a".repeat(4096 - prefix.len() - terminator.len())
        );
        assert_eq!(frame.len(), 4096);
        assert!(parse_sequence(frame.as_bytes())?.is_some());
        let too_long = format!(
            "{prefix}a{}{terminator}",
            "a".repeat(4096 - prefix.len() - terminator.len())
        );
        assert_eq!(
            parse_sequence(too_long.as_bytes()),
            Err(Error::Limit(Limit::Sequence))
        );
    }
    Ok(())
}

#[test]
fn query_and_default_capabilities() -> TestResult {
    assert_eq!(parse_body(b"?"), Ok(Some(Event::Query)));
    let Some(Event::Capabilities(defaults)) = parse_body(b"?:")? else {
        return Err("expected capabilities".into());
    };
    assert!(defaults.supports_state(State::Blocked));
    assert!(defaults.supports_kind(Kind::Auth));
    Ok(())
}

#[test]
fn capabilities_preserve_future_tokens_and_last_values() -> TestResult {
    let Some(Event::Capabilities(value)) = parse_body(
        b"? : states=idle : states=working,future : kinds=auth,biometric : extension=a",
    )?
    else {
        return Err("expected capabilities".into());
    };
    assert_eq!(value.states().collect::<Vec<_>>(), ["working", "future"]);
    assert!(value.supports_state(State::Working));
    assert!(!value.supports_state(State::Idle));
    assert_eq!(value.kinds().collect::<Vec<_>>(), ["auth", "biometric"]);
    Ok(())
}

#[test]
fn capability_extensions_ignore_report_limits() -> TestResult {
    for (key, value) in [
        ("app", "a".repeat(33)),
        ("id", "a".repeat(33)),
        ("id", "a/a/a/a/a/a/a/a/a".to_owned()),
        ("id", "a".repeat(129)),
        ("title", "a".repeat(257)),
        ("msg", "a".repeat(2733)),
        ("abcdefghijklmnopq", "future".to_owned()),
    ] {
        let body = format!("?:{key}={value}:states=working:kinds=auth");
        let Some(Event::Capabilities(capabilities)) = parse_body(body.as_bytes())? else {
            return Err("expected capabilities".into());
        };
        assert_eq!(capabilities.states().collect::<Vec<_>>(), ["working"]);
        assert_eq!(capabilities.kinds().collect::<Vec<_>>(), ["auth"]);

        let frame = format!("\x1b]7501;{body}\x1b\\");
        assert_eq!(
            parse_sequence(frame.as_bytes())?,
            Some(Event::Capabilities(capabilities))
        );
        let mut decoder = Decoder::new();
        let mut seen = 0;
        let mut result = Ok(());
        for chunk in frame.as_bytes().chunks(7) {
            decoder.feed(chunk, |event| match event {
                Ok(event) => {
                    assert_eq!(event, Event::Capabilities(capabilities));
                    seen += 1;
                }
                Err(error) => result = Err(error),
            });
        }
        result?;
        assert_eq!(seen, 1);
    }
    Ok(())
}

#[test]
fn capability_replies_keep_the_sequence_limit() -> TestResult {
    for terminator in ["\x07", "\x1b\\"] {
        let prefix = "\x1b]7501;?:states=working:future=";
        let frame = format!(
            "{prefix}{}{terminator}",
            "a".repeat(4096 - prefix.len() - terminator.len())
        );
        assert!(matches!(
            parse_sequence(frame.as_bytes())?,
            Some(Event::Capabilities(_))
        ));
        let too_long = format!(
            "{prefix}a{}{terminator}",
            "a".repeat(4096 - prefix.len() - terminator.len())
        );
        assert_eq!(
            parse_sequence(too_long.as_bytes()),
            Err(Error::Limit(Limit::Sequence))
        );
    }
    let mut body = b"?:states=working:future=".to_vec();
    body.resize(4087, b'a');
    assert!(matches!(parse_body(&body)?, Some(Event::Capabilities(_))));
    body.push(b'a');
    assert_eq!(parse_body(&body), Err(Error::Limit(Limit::Sequence)));
    Ok(())
}

#[test]
fn empty_capabilities_are_distinct_from_absent_lists() -> TestResult {
    let Some(Event::Capabilities(value)) = parse_body(b"?:states=:kinds=")? else {
        return Err("expected capabilities".into());
    };
    assert_eq!(value.states().count(), 0);
    assert_eq!(value.kinds().count(), 0);
    let Some(Event::Capabilities(value)) = parse_body(b"?:future=ok")? else {
        return Err("expected capabilities".into());
    };
    assert_eq!(value.states().count(), 5);
    assert_eq!(value.kinds().count(), 3);
    Ok(())
}

#[test]
fn streaming_works_across_every_two_chunk_split() -> TestResult {
    let frame = b"\x1b]7501;state=blocked:id=build/test:kind=question:msg=T0s=\x1b\\";
    let expected = parse_sequence(frame)?.ok_or("missing event")?;
    for split in 0..=frame.len() {
        let mut decoder = Decoder::new();
        let mut events = 0;
        let mut result = Ok(());
        let mut receive = |event: Result<Event<'_>, Error>| match event {
            Ok(event) => {
                assert_eq!(event, expected);
                events += 1;
            }
            Err(error) => result = Err(error),
        };
        decoder.feed(&frame[..split], &mut receive);
        decoder.feed(&frame[split..], &mut receive);
        result?;
        assert_eq!(events, 1);
    }
    Ok(())
}

#[test]
fn streaming_accepts_retries_and_reused_record_ids() -> TestResult {
    let mut decoder = Decoder::new();
    let mut actions = Vec::new();
    let id = RecordId::new("build/test")?;
    let mut result: TestResult = Ok(());
    let frames = b"\x1b]7501;state=error:id=build/test\x1b\\\x1b]7501;state=working:id=build/test\x1b\\\x1b]7501;state=done:id=build/test\x1b\\\x1b]7501;state=working:id=build/test\x1b\\";
    decoder.feed(frames, |event| match event {
        Ok(Event::Report(value)) => {
            assert_eq!(value.id(), Some(id));
            actions.push(value.action());
        }
        Ok(_) => result = Err("unexpected event".into()),
        Err(error) => result = Err(error.into()),
    });
    result?;
    assert_eq!(
        actions,
        [
            Action::Set(State::Error),
            Action::Set(State::Working),
            Action::Set(State::Done),
            Action::Set(State::Working),
        ]
    );
    Ok(())
}

#[test]
fn streaming_works_bytewise_and_skips_other_oscs() -> TestResult {
    let mut decoder = Decoder::new();
    let mut states = Vec::new();
    let bytes = b"text\x1b[31m\x1b]0;title containing state=done\x07\x1b]7501;state=working\x07\x1b]7501;state=done\x1b\\";
    let mut result = Ok(());
    for byte in bytes {
        decoder.feed(&[*byte], |event| match event {
            Ok(Event::Report(value)) => {
                states.push(value.action());
            }
            Ok(_) => {}
            Err(error) => result = Err(error),
        });
    }
    result?;
    assert_eq!(
        states,
        [Action::Set(State::Working), Action::Set(State::Done)]
    );
    Ok(())
}

#[test]
fn streaming_drains_oversized_frames_and_recovers() -> TestResult {
    let bytes = format!(
        "\x1b]7501;state=idle:future={}\x1b\\\x1b]7501;state=done\x07",
        "a".repeat(5000)
    );
    let mut decoder = Decoder::new();
    let mut errors = 0;
    let mut reports = 0;
    let mut result = Ok(());
    for chunk in bytes.as_bytes().chunks(31) {
        decoder.feed(chunk, |event| match event {
            Err(error) => {
                assert_eq!(error, Error::Limit(Limit::Sequence));
                errors += 1;
            }
            Ok(Event::Report(value)) => {
                assert_eq!(value.action(), Action::Set(State::Done));
                reports += 1;
            }
            Ok(_) => result = Err("unexpected event"),
        });
    }
    result?;
    assert_eq!((errors, reports), (1, 1));
    Ok(())
}

#[test]
fn streaming_accepts_exact_frame_limit_and_rejects_one_more_byte() {
    for terminator in ["\x07", "\x1b\\"] {
        for size in [4096, 4097] {
            let prefix = "\x1b]7501;state=idle:future=";
            let frame = format!(
                "{prefix}{}{terminator}",
                "x".repeat(size - prefix.len() - terminator.len())
            );
            let mut decoder = Decoder::new();
            let mut seen = 0;
            decoder.feed(frame.as_bytes(), |result| {
                assert_eq!(result.is_ok(), size == 4096);
                seen += 1;
            });
            assert_eq!(seen, 1);
        }
    }
}

#[test]
fn decoder_reset_discards_partial_frame() {
    let mut decoder = Decoder::new();
    let mut seen = 0;
    decoder.feed(b"\x1b]7501;state=", |_| seen += 1);
    decoder.reset();
    decoder.feed(b"idle\x07", |_| seen += 1);
    assert_eq!(seen, 0);
    decoder.feed(b"\x1b]7501;state=idle\x07", |_| seen += 1);
    assert_eq!(seen, 1);
}

#[test]
fn independent_oracle_for_every_message_length() -> TestResult {
    let mut output = [0; MAX_MESSAGE];
    for length in 0..=MAX_MESSAGE {
        let text: String = (b' '..=b'~').cycle().take(length).map(char::from).collect();
        for engine in [&STANDARD, &STANDARD_NO_PAD] {
            let body = format!("state=done:msg={}", engine.encode(&text));
            assert_eq!(
                report(body.as_bytes())?
                    .message()
                    .ok_or("missing message")?
                    .decode_into(&mut output)?,
                text
            );
        }
    }
    Ok(())
}

#[test]
fn byte_mutations_never_panic_and_streaming_matches_complete_parser() {
    let original =
        b"\x1b]7501;state=blocked:id=build/test:app=cargo:kind=question:progress=40:msg=T0s=\x1b\\";
    for position in 7..original.len() - 2 {
        for byte in 0..=255 {
            if matches!(byte, 0x07 | 0x1b) {
                continue;
            }
            let mut mutated = original.to_vec();
            mutated[position] = byte;
            let expected = parse_sequence(&mutated);
            let mut decoder = Decoder::new();
            let mut count = 0;
            decoder.feed(&mutated, |result| {
                count += 1;
                assert_eq!(result.map(Some), expected);
            });
            assert_eq!(count, usize::from(expected != Ok(None)));
        }
    }
}

#[test]
fn maximum_reports_streaming_and_text_decoding_allocate_zero_bytes() -> TestResult {
    let body = format!(
        "state=blocked:id=build/test:app=cargo:kind=auth:title={}:msg={}",
        STANDARD.encode("t".repeat(192)),
        STANDARD.encode("m".repeat(2048))
    );
    let frame = format!("\x1b]7501;{body}\x1b\\");
    let mut result = Ok(());
    let info = allocation_counter::measure(|| {
        result = (|| -> TestResult {
            let mut output = [0; MAX_MESSAGE];
            let mut decoder = Decoder::new();
            for _ in 0..100 {
                let value = report(std::hint::black_box(body.as_bytes()))?;
                std::hint::black_box(
                    value
                        .message()
                        .ok_or("missing message")?
                        .decode_into(&mut output)?,
                );
                let mut stream_result = Ok(());
                for chunk in frame.as_bytes().chunks(17) {
                    decoder.feed(chunk, |event| match event {
                        Ok(event) => {
                            std::hint::black_box(event);
                        }
                        Err(error) => stream_result = Err(error),
                    });
                }
                stream_result?;
            }
            Ok(())
        })();
    });
    result?;
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
    assert!(std::mem::size_of::<Decoder>() < 8192);
    assert!(std::mem::size_of::<Report<'_>>() < 256);
    Ok(())
}

#[test]
fn malformed_identifiable_id_is_never_treated_as_absent() -> TestResult {
    assert_eq!(parse_body(b"state=clear:id=bad!"), Ok(None));
    assert_eq!(parse_body(b"state=clear:id=good:id=bad!"), Ok(None));
    assert_eq!(parse_body(b"state=clear:id=\xff"), Ok(None));
    assert_eq!(
        report(b"state=clear:id=bad!:id=good")?
            .id()
            .ok_or("missing record id")?
            .as_str(),
        "good"
    );
    Ok(())
}
