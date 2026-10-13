use crate::{
    Action, AppId, Capabilities, EncodedText, Error, Event, Kind, Limit, MAX_APP, MAX_DEPTH,
    MAX_ID, MAX_KEY, MAX_MESSAGE, MAX_MESSAGE_ENCODED, MAX_SEGMENT, MAX_SEQUENCE, MAX_TITLE,
    MAX_TITLE_ENCODED, Progress, RecordId, Report, State, base64,
};

const PREFIX: &[u8] = b"\x1b]7501;";

/// Parses exactly one OSC 7501 sequence terminated by BEL or ESC-backslash.
///
/// `Ok(None)` means an ignored report (missing/unknown state or invalid id).
/// Malformed pairs and unknown keys are skipped, and duplicates use the last
/// syntactically valid pair. Every text pair and hard cap is checked, including
/// pairs overridden later. Nothing is returned until validation completes.
///
/// # Errors
/// Returns [`Error`] for bad framing or a report rejected as a whole.
pub fn parse_sequence(input: &[u8]) -> Result<Option<Event<'_>>, Error> {
    if input.len() > MAX_SEQUENCE {
        return Err(Error::Limit(Limit::Sequence));
    }
    let rest = input.strip_prefix(PREFIX).ok_or(Error::Framing)?;
    let body = rest
        .strip_suffix(b"\x1b\\")
        .or_else(|| rest.strip_suffix(b"\x07"))
        .ok_or(Error::Framing)?;
    if body.iter().any(|&byte| matches!(byte, 0x1b | 0x07)) {
        return Err(Error::Framing);
    }
    parse(body)
}

/// Parses a body after a terminal dispatcher has consumed `OSC 7501;` and ST.
///
/// Uses the longer two-byte ST when accounting for the whole-sequence cap.
/// Use [`parse_sequence`] to accept a BEL frame at the exact 4096-byte boundary.
///
/// # Errors
/// Returns [`Error`] if a report violates a hard cap or text rule.
pub fn parse_body(body: &[u8]) -> Result<Option<Event<'_>>, Error> {
    if body.len() > MAX_SEQUENCE - PREFIX.len() - 2 {
        return Err(Error::Limit(Limit::Sequence));
    }
    parse(body)
}

fn trim(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn value_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b',' | b'+' | b'/' | b'=' | b'-')
}

fn pairs(body: &[u8]) -> impl Iterator<Item = Result<(&[u8], &[u8]), Error>> {
    body.split(|&byte| byte == b':').filter_map(|pair| {
        let equals = pair.iter().position(|&byte| byte == b'=')?;
        let key = trim(&pair[..equals]);
        let value = trim(&pair[equals + 1..]);
        if key.len() > MAX_KEY {
            return Some(Err(Error::Limit(Limit::Key)));
        }
        let limit = match key {
            b"msg" if value.len() > MAX_MESSAGE_ENCODED => Some(Limit::Message),
            b"title" if value.len() > MAX_TITLE_ENCODED => Some(Limit::Title),
            b"app" if value.len() > MAX_APP => Some(Limit::App),
            b"id"
                if value.len() > MAX_ID
                    || value.split(|&byte| byte == b'/').count() > MAX_DEPTH
                    || value
                        .split(|&byte| byte == b'/')
                        .any(|segment| segment.len() > MAX_SEGMENT) =>
            {
                Some(Limit::Id)
            }
            _ => None,
        };
        if let Some(limit) = limit {
            return Some(Err(Error::Limit(limit)));
        }
        if key.is_empty()
            || !key.iter().all(u8::is_ascii_lowercase)
            || (key != b"id"
                && !value
                    .iter()
                    .copied()
                    .fold(true, |valid, byte| valid & value_byte(byte)))
        {
            return None;
        }
        Some(Ok((key, value)))
    })
}

fn ascii(bytes: &[u8]) -> Result<&str, Error> {
    core::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
}

fn text(value: &[u8], cap: usize, limit: Limit) -> Result<EncodedText<'_>, Error> {
    let mut scratch = [0; MAX_MESSAGE];
    let length = base64::decode(value, &mut scratch[..cap]).map_err(|error| match error {
        Error::BufferTooSmall => Error::Limit(limit),
        error => error,
    })?;
    let decoded = core::str::from_utf8(&scratch[..length]).map_err(|_| Error::InvalidUtf8)?;
    let controls = if decoded.is_ascii() {
        decoded.bytes().fold(0_u8, |found, byte| {
            found | u8::from(byte < 32 || byte == 127)
        }) != 0
    } else {
        decoded.chars().any(char::is_control)
    };
    if controls {
        return Err(Error::ControlCharacter);
    }
    Ok(EncodedText {
        encoded: ascii(value)?,
        decoded_len: length,
    })
}

fn progress(value: &[u8]) -> Option<Progress> {
    if value.is_empty() {
        return None;
    }
    let mut percentage: u8 = 0;
    for &byte in value {
        if !byte.is_ascii_digit() {
            return None;
        }
        percentage = percentage.checked_mul(10)?.checked_add(byte - b'0')?;
        if percentage > 100 {
            return None;
        }
    }
    Progress::new(percentage).ok()
}

fn parse(body: &[u8]) -> Result<Option<Event<'_>>, Error> {
    let body = trim(body);
    if body == b"?" {
        return Ok(Some(Event::Query));
    }
    if let Some(body) = body
        .strip_prefix(b"?")
        .map(trim)
        .and_then(|body| body.strip_prefix(b":"))
    {
        let mut capabilities = Capabilities {
            states: None,
            kinds: None,
        };
        for pair in pairs(body) {
            let (key, value) = pair?;
            match key {
                b"states" => capabilities.states = Some(ascii(value)?),
                b"kinds" => capabilities.kinds = Some(ascii(value)?),
                _ => {}
            }
        }
        return Ok(Some(Event::Capabilities(capabilities)));
    }
    let mut action = None;
    let mut raw_id = None;
    let mut report = Report {
        action: Action::Clear,
        id: None,
        kind: None,
        progress: None,
        app: None,
        title: None,
        message: None,
    };
    for pair in pairs(body) {
        let (key, value) = pair?;
        match key {
            b"state" => {
                action = if value == b"clear" {
                    Some(Action::Clear)
                } else {
                    State::parse(ascii(value)?).map(Action::Set)
                }
            }
            b"id" => raw_id = Some(value),
            b"kind" => report.kind = Kind::parse(ascii(value)?),
            b"progress" => report.progress = progress(value),
            b"app" => report.app = AppId::new(ascii(value)?).ok(),
            b"title" => report.title = Some(text(value, MAX_TITLE, Limit::Title)?),
            b"msg" => report.message = Some(text(value, MAX_MESSAGE, Limit::Message)?),
            _ => {}
        }
    }
    let Some(action) = action else {
        return Ok(None);
    };
    if let Some(id) = raw_id {
        let Some(id) = core::str::from_utf8(id)
            .ok()
            .and_then(|value| RecordId::new(value).ok())
        else {
            return Ok(None);
        };
        report.id = Some(id);
    }
    report.action = action;
    if action != Action::Set(State::Blocked) {
        report.kind = None;
    }
    if !matches!(action, Action::Set(State::Working | State::Blocked)) {
        report.progress = None;
    }
    Ok(Some(Event::Report(report)))
}
