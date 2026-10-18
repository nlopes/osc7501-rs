use crate::{
    Error, MAX_RECORDS, Record, Store,
    parser::{AppId, MAX_APP, Report},
};

/// A change to a current record or its inherited app.
///
/// Unchanged reports emit no change, even though they refresh eviction order.
/// An ancestor update can change a child's effective app without a child report.
/// Keep reports and lifecycle events to reproduce the store's full history.
///
/// Views borrow the store only for the callback. Copy any data you need to retain.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Change<'a> {
    /// A new record, changed explicit fields, or a changed effective app.
    Set {
        /// The record after the complete store operation.
        record: Record<'a>,
        /// The app resolved through the record and its remaining ancestors.
        effective_app: Option<AppId<'a>>,
    },
    /// An old record, delivered before its removal.
    Remove(Record<'a>),
}

#[derive(Clone, Copy)]
struct AppSnapshot {
    bytes: [u8; MAX_APP],
    length: usize,
}

impl AppSnapshot {
    fn new(app: AppId<'_>) -> Self {
        let value = app.as_str().as_bytes();
        let mut snapshot = Self {
            bytes: [0; MAX_APP],
            length: value.len(),
        };
        snapshot.bytes[..value.len()].copy_from_slice(value);
        snapshot
    }

    fn matches(&self, app: AppId<'_>) -> bool {
        self.bytes[..self.length] == *app.as_str().as_bytes()
    }
}

impl Store<'_> {
    /// Applies a report and delivers all resulting record and app changes.
    ///
    /// Removals arrive before each record is removed. Sets arrive after the
    /// complete operation, once per new or changed surviving record. Their
    /// effective apps include ancestor replacement and eviction effects.
    /// Order within removals or sets is unspecified; all removals precede sets.
    ///
    /// Equal decoded fields and effective apps produce no set notification.
    /// The report still refreshes update order, just as [`apply`](Self::apply) does.
    /// Does not allocate. Change detection copies effective apps into bounded
    /// scratch storage instead of copying complete records.
    ///
    /// # Errors
    /// Returns [`Error::Parser`] if field preparation fails, before any callback
    /// or mutation. Callbacks are infallible and their effects are not rolled back.
    pub fn apply_with_changes(
        &mut self,
        report: Report<'_>,
        mut on_change: impl FnMut(Change<'_>),
    ) -> Result<(), Error> {
        let mut apps = [None; MAX_RECORDS];
        self.snapshot_apps(&mut apps);
        let changed = self.apply_inner(report, |record| on_change(Change::Remove(record)), true)?;
        self.notify_sets(&apps, changed, &mut on_change);
        Ok(())
    }

    /// Removes active records and reports inherited-app changes on survivors.
    ///
    /// Uses [`on_prompt`](Self::on_prompt) retention rules. All removals precede
    /// sets, which contain the final records and effective apps. Does not allocate.
    pub fn on_prompt_with_changes(&mut self, mut on_change: impl FnMut(Change<'_>)) {
        let mut apps = [None; MAX_RECORDS];
        self.snapshot_apps(&mut apps);
        self.remove_active(&mut |record| on_change(Change::Remove(record)));
        self.notify_sets(&apps, None, &mut on_change);
    }

    /// Applies prompt cleanup and change reporting when the attached process exits.
    pub fn on_process_exit_with_changes(&mut self, on_change: impl FnMut(Change<'_>)) {
        self.on_prompt_with_changes(on_change);
    }

    /// Removes every record on a full terminal reset, delivering only removals.
    pub fn on_full_reset_with_changes(&mut self, mut on_change: impl FnMut(Change<'_>)) {
        self.remove_matching(|_| true, &mut |record| on_change(Change::Remove(record)));
    }

    fn snapshot_apps(&self, apps: &mut [Option<AppSnapshot>; MAX_RECORDS]) {
        for (index, slot) in self.slots.iter().enumerate() {
            apps[index] = slot
                .record
                .as_ref()
                .and_then(|record| self.effective_app(record.view().id()))
                .map(AppSnapshot::new);
        }
    }

    fn notify_sets(
        &self,
        apps: &[Option<AppSnapshot>; MAX_RECORDS],
        changed: Option<usize>,
        on_change: &mut impl FnMut(Change<'_>),
    ) {
        // Slots do not move; changed also marks a reused slot.
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(record) = slot.record.as_ref().map(super::record::StoredRecord::view) else {
                continue;
            };
            let effective_app = self.effective_app(record.id());
            let same_app = match (&apps[index], effective_app) {
                (Some(previous), Some(app)) => previous.matches(app),
                (None, None) => true,
                _ => false,
            };
            if changed == Some(index) || !same_app {
                on_change(Change::Set {
                    record,
                    effective_app,
                });
            }
        }
    }
}
