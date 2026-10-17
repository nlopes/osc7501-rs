use crate::{AppId, RecordId, State, Status};

/// Borrowed fields to repeat in each report for a record.
///
/// [`status`](Self::status) starts a fresh complete replacement with this app,
/// id, and title. Each report starts without a message, progress, or blocking kind.
/// Creating a context or a report does not allocate or write output.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    id: Option<RecordId<'a>>,
    app: Option<AppId<'a>>,
    title: Option<&'a str>,
}

impl<'a> Context<'a> {
    /// Creates a root context with no app or title.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            id: None,
            app: None,
            title: None,
        }
    }

    /// Addresses this validated child record in each report.
    #[must_use]
    pub const fn id(mut self, id: RecordId<'a>) -> Self {
        self.id = Some(id);
        self
    }

    /// Repeats this validated application identifier in each report.
    #[must_use]
    pub const fn app(mut self, app: AppId<'a>) -> Self {
        self.app = Some(app);
        self
    }

    /// Repeats this single-line label, validated when a report is sent.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Begins a fresh report with the context's app, id, and title.
    ///
    /// The report borrows the original fields independently of this context.
    /// Its setters can override those fields without changing subsequent reports.
    /// Validation and output happen in [`Status::write_to`].
    #[must_use]
    pub const fn status(&self, state: State) -> Status<'a> {
        Status {
            id: self.id,
            app: self.app,
            title: self.title,
            ..Status::new(state)
        }
    }
}
