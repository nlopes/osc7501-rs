use crate::parser::{
    AppId, EncodedText, Error, Kind, MAX_APP, MAX_ID, MAX_MESSAGE, MAX_TITLE, Progress, RecordId,
    Report, State,
};

#[derive(Debug)]
struct Text<const N: usize> {
    bytes: [u8; N],
    length: usize,
}

impl<const N: usize> Text<N> {
    const EMPTY: Self = Self {
        bytes: [0; N],
        length: 0,
    };

    fn copy(value: &str) -> Result<Self, Error> {
        let mut text = Self {
            bytes: [0; N],
            length: value.len(),
        };
        text.bytes
            .get_mut(..value.len())
            .ok_or(Error::BufferTooSmall)?
            .copy_from_slice(value.as_bytes());
        Ok(text)
    }

    fn decode(&mut self, value: EncodedText<'_>) -> Result<(), Error> {
        value.decode_into(&mut self.bytes)?;
        self.length = value.decoded_len();
        Ok(())
    }

    fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.bytes[..self.length]).ok()
    }
}

#[derive(Debug)]
pub(crate) struct StoredRecord {
    state: State,
    id: Option<Text<MAX_ID>>,
    app: Option<Text<MAX_APP>>,
    title: Option<Text<MAX_TITLE>>,
    message: Option<Text<MAX_MESSAGE>>,
    kind: Option<Kind>,
    progress: Option<Progress>,
}

impl StoredRecord {
    pub(crate) fn prepare(state: State, report: Report<'_>) -> Result<Self, Error> {
        let mut prepared = Self {
            state,
            id: report.id().map(|id| Text::copy(id.as_str())).transpose()?,
            app: report
                .app()
                .map(|app| Text::copy(app.as_str()))
                .transpose()?,
            title: None,
            message: None,
            kind: report.kind(),
            progress: report.progress(),
        };
        // Decode into the prepared record to avoid large intermediate text copies.
        if let Some(title) = report.title() {
            prepared.title.insert(Text::EMPTY).decode(title)?;
        }
        if let Some(message) = report.message() {
            prepared.message.insert(Text::EMPTY).decode(message)?;
        }
        Ok(prepared)
    }

    pub(crate) fn matches_id(&self, id: Option<&str>) -> bool {
        match (&self.id, id) {
            (None, None) => true,
            (Some(stored), Some(id)) => stored.bytes[..stored.length] == *id.as_bytes(),
            _ => false,
        }
    }

    pub(crate) const fn view(&self) -> Record<'_> {
        Record { stored: self }
    }

    pub(crate) fn same_fields(&self, other: &Self) -> bool {
        let left = self.view();
        let right = other.view();
        left.state() == right.state()
            && left.id() == right.id()
            && left.app() == right.app()
            && left.title() == right.title()
            && left.message() == right.message()
            && left.kind() == right.kind()
            && left.progress() == right.progress()
    }
}

/// A read-only view of a current record. Text is decoded, plain, and untrusted.
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    stored: &'a StoredRecord,
}

impl<'a> Record<'a> {
    /// Returns the current state; clear is a removal action and is never stored.
    #[must_use]
    pub const fn state(self) -> State {
        self.stored.state
    }

    /// Returns the validated identifier; `None` means root.
    #[must_use]
    pub fn id(self) -> Option<RecordId<'a>> {
        RecordId::new(self.stored.id.as_ref()?.as_str()?).ok()
    }

    /// Returns the explicit app, before ancestor inheritance.
    #[must_use]
    pub fn app(self) -> Option<AppId<'a>> {
        AppId::new(self.stored.app.as_ref()?.as_str()?).ok()
    }

    /// Returns the decoded label, preserving absent and explicitly empty text.
    #[must_use]
    pub fn title(self) -> Option<&'a str> {
        self.stored.title.as_ref()?.as_str()
    }

    /// Returns the decoded message, preserving absent and explicitly empty text.
    #[must_use]
    pub fn message(self) -> Option<&'a str> {
        self.stored.message.as_ref()?.as_str()
    }

    /// Returns the blocking reason, present only for blocked records.
    #[must_use]
    pub const fn kind(self) -> Option<Kind> {
        self.stored.kind
    }

    /// Returns the percentage; absent means indeterminate.
    #[must_use]
    pub const fn progress(self) -> Option<Progress> {
        self.stored.progress
    }
}
