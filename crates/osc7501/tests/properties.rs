//! Producer validation and transport behavior over generated Unicode text.
use hegel::{TestCase, generators as gs};
use osc7501::{
    Error, State, Status,
    parser::{Action, Event, MAX_MESSAGE, MAX_TITLE, parse_sequence},
};
use std::io::{self, Write};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[hegel::test]
fn unicode_text_is_rejected_before_writing_or_round_trips(tc: TestCase) {
    let text = tc.draw(hegel::one_of![
        gs::text().max_size(150),
        gs::text()
            .alphabet(" a\té€🦀\0\n\u{7f}\u{9f}")
            .max_size(MAX_MESSAGE + 1),
    ]);
    let title = tc.draw(gs::booleans());
    let cap = if title { MAX_TITLE } else { MAX_MESSAGE };
    let status = if title {
        Status::new(State::Done).title(&text)
    } else {
        Status::new(State::Done).message(&text)
    };
    let mut output = Vec::new();
    let result = status.write_to(&mut output);
    if text.len() > cap {
        assert!(matches!(result, Err(Error::TextTooLong)));
        assert_eq!(output, [] as [u8; 0]);
    } else if text.chars().any(char::is_control) {
        assert!(matches!(result, Err(Error::ControlCharacter)));
        assert_eq!(output, [] as [u8; 0]);
    } else {
        assert!(result.is_ok(), "{result:?}");
        let result = check_text(&output, &text, title);
        assert!(result.is_ok(), "{result:?}");
    }
}

fn check_text(output: &[u8], text: &str, title: bool) -> TestResult {
    let Some(Event::Report(report)) = parse_sequence(output)? else {
        return Err("expected report".into());
    };
    assert_eq!(report.action(), Action::Set(State::Done));
    let encoded = if title {
        report.title()
    } else {
        report.message()
    }
    .ok_or("missing text")?;
    let mut buffer = vec![0; text.len()];
    assert_eq!(encoded.decode_into(&mut buffer)?, text);
    Ok(())
}

struct ShortWriter {
    bytes: Vec<u8>,
    width: usize,
    interrupt: bool,
    fail_at: Option<usize>,
}

impl Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let remaining = self
            .fail_at
            .map_or(usize::MAX, |limit| limit - self.bytes.len());
        if remaining == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let count = bytes.len().min(self.width).min(remaining);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[hegel::test]
fn short_writes_and_interruptions_preserve_the_frame_and_io_failure_prefix(tc: TestCase) {
    let message = tc.draw(gs::text().alphabet(" a:;=é€🦀").max_size(100));
    let width = tc.draw(gs::integers::<usize>().min_value(1).max_value(12));
    let interrupted = tc.draw(gs::booleans());
    let result = check_writes(&tc, &message, width, interrupted);
    assert!(result.is_ok(), "{result:?}");
}

fn check_writes(tc: &TestCase, message: &str, width: usize, interrupt: bool) -> TestResult {
    let status = Status::new(State::Done).message(message);
    let mut expected = Vec::new();
    status.write_to(&mut expected)?;
    let mut writer = ShortWriter {
        bytes: Vec::new(),
        width,
        interrupt,
        fail_at: None,
    };
    status.write_to(&mut writer)?;
    assert_eq!(writer.bytes, expected);
    let fail_at = tc.draw(gs::integers::<usize>().max_value(expected.len() - 1));
    writer.bytes.clear();
    writer.interrupt = interrupt;
    writer.fail_at = Some(fail_at);
    assert!(matches!(status.write_to(&mut writer), Err(Error::Io(error))
        if error.kind() == io::ErrorKind::BrokenPipe));
    assert_eq!(writer.bytes, expected[..fail_at]);
    Ok(())
}
