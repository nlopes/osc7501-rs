#![doc = include_str!("../README.md")]
#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

mod change;
mod record;

pub use change::Change;
/// Parse and validate reports before applying them to the store.
pub use osc7501_parser as parser;
pub use record::Record;

use core::ops::{Deref, DerefMut};
use parser::{Action, AppId, RecordId, Report, State};
use record::StoredRecord;

/// Minimum record capacity required of a conforming terminal host.
pub const MIN_HOST_CAPACITY: usize = 64;
/// Maximum records per terminal, including root.
pub const MAX_RECORDS: usize = 256;

/// Caller-owned storage for one record. Fields cannot be mutated directly.
#[derive(Debug)]
pub struct Slot {
    record: Option<StoredRecord>,
    rank: u16,
}

impl Slot {
    /// An unused slot, suitable for `[const { Slot::EMPTY }; capacity]`.
    pub const EMPTY: Self = Self {
        record: None,
        rank: 0,
    };
}

enum Storage<'a> {
    Borrowed(&'a mut [Slot]),
    #[cfg(feature = "alloc")]
    Owned(alloc::vec::Vec<Slot>),
}

impl Storage<'_> {
    const fn as_slice(&self) -> &[Slot] {
        match self {
            Self::Borrowed(slots) => slots,
            #[cfg(feature = "alloc")]
            Self::Owned(slots) => slots.as_slice(),
        }
    }
}

impl Deref for Storage<'_> {
    type Target = [Slot];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for Storage<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Borrowed(slots) => slots,
            #[cfg(feature = "alloc")]
            Self::Owned(slots) => slots,
        }
    }
}

/// Current records for one terminal with a fixed capacity.
///
/// Use [`Store::from_slots`] to supply storage without an allocator.
/// With the `alloc` feature, `Store::new(capacity)` owns heap storage instead.
/// Updates replace every explicit field. Lookup does not affect eviction order.
/// Change callbacks borrow records only for the duration of the call.
/// Records belong to the terminal, so screen switches require no store action.
pub struct Store<'a> {
    slots: Storage<'a>,
    length: u16,
}

#[cfg(feature = "alloc")]
impl Store<'static> {
    /// Creates an empty store with heap storage for `capacity` records.
    /// Requires the `alloc` feature. Storage is allocated once and never grows.
    /// Capacities below 64 serve tests but do not meet the host requirement.
    ///
    /// # Errors
    /// Returns [`Error::InvalidCapacity`] for zero or more than 256 records,
    /// before allocating. Returns [`Error::Allocation`] if storage cannot be reserved.
    pub fn new(capacity: usize) -> Result<Self, Error> {
        validate_capacity(capacity)?;
        let mut slots = alloc::vec::Vec::new();
        slots.try_reserve_exact(capacity)?;
        slots.resize_with(capacity, || Slot::EMPTY);
        Ok(Self {
            slots: Storage::Owned(slots),
            length: 0,
        })
    }
}

impl<'a> Store<'a> {
    /// Borrows caller storage and discards any records previously in the slots.
    /// This constructor does not allocate, including with the `alloc` feature.
    /// Capacities below 64 serve tests but do not meet the host requirement.
    ///
    /// # Errors
    /// Returns [`Error::InvalidCapacity`] for zero slots or more than 256 slots,
    /// without changing the supplied storage.
    pub fn from_slots(slots: &'a mut [Slot]) -> Result<Self, Error> {
        validate_capacity(slots.len())?;
        for slot in &mut *slots {
            slot.record = None;
            slot.rank = 0;
        }
        Ok(Self {
            slots: Storage::Borrowed(slots),
            length: 0,
        })
    }

    /// Returns the number of current records, including root.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.length)
    }

    /// Returns whether there are no current records.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Returns the caller-selected record capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.slots.as_slice().len()
    }

    /// Looks up a root (`None`) or child record without updating its age.
    #[must_use]
    pub fn get(&self, id: Option<RecordId<'_>>) -> Option<Record<'_>> {
        self.find(id.map(RecordId::as_str)).map(StoredRecord::view)
    }

    /// Iterates current records. Iteration order is unspecified.
    pub fn iter(&self) -> impl Iterator<Item = Record<'_>> {
        self.slots
            .iter()
            .filter_map(|slot| slot.record.as_ref().map(StoredRecord::view))
    }

    /// Resolves a stored record's app through its nearest ancestor with an app.
    /// Returns `None` when the addressed record does not exist or no app applies.
    #[must_use]
    pub fn effective_app(&self, id: Option<RecordId<'_>>) -> Option<AppId<'_>> {
        let mut path = id.map(RecordId::as_str);
        self.find(path)?;
        loop {
            if let Some(app) = self.find(path).and_then(|record| record.view().app()) {
                return Some(app);
            }
            path = path?.rsplit_once('/').map(|(parent, _)| parent);
        }
    }

    /// Applies a validated report as a complete replacement or subtree clear.
    /// A new record at capacity evicts the least recently updated record.
    /// Existing-record updates never evict another record.
    /// Every set refreshes the record's update order, even when all fields match.
    ///
    /// Does not allocate or collect change notifications.
    /// Use [`apply_with_changes`](Self::apply_with_changes) to observe removals,
    /// replacements, and inherited-app changes.
    ///
    /// # Errors
    /// Returns [`Error::Parser`] if copying or decoding a field fails. All fields
    /// are prepared before any slot mutation.
    pub fn apply(&mut self, report: Report<'_>) -> Result<(), Error> {
        self.apply_inner(report, |_| {}, false).map(|_| ())
    }

    fn apply_inner(
        &mut self,
        report: Report<'_>,
        mut on_remove: impl FnMut(Record<'_>),
        track_changes: bool,
    ) -> Result<Option<usize>, Error> {
        let id = report.id().map(RecordId::as_str);
        let Action::Set(state) = report.action() else {
            self.remove_matching(
                |record| match id {
                    None => true,
                    Some(parent) => record.id().is_some_and(|child| {
                        let child = child.as_str();
                        child == parent
                            || child
                                .strip_prefix(parent)
                                .is_some_and(|suffix| suffix.starts_with('/'))
                    }),
                },
                &mut on_remove,
            );
            return Ok(None);
        };

        let prepared = StoredRecord::prepare(state, report)?;
        let existing = self.slots.iter().position(|slot| {
            slot.record
                .as_ref()
                .is_some_and(|record| record.matches_id(id))
        });
        let index = existing
            .or_else(|| self.slots.iter().position(|slot| slot.record.is_none()))
            .unwrap_or_else(|| {
                self.slots
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, slot)| slot.rank)
                    .map_or(0, |(index, _)| index)
            });
        let changed = track_changes
            && self.slots[index]
                .record
                .as_ref()
                .is_none_or(|record| !record.same_fields(&prepared));
        if let Some(record) = self.slots[index].record.as_ref() {
            if existing.is_none() {
                on_remove(record.view());
            }
            self.discard(index);
        }
        self.slots[index].record = Some(prepared);
        self.slots[index].rank = self.length;
        self.length += 1;
        Ok(changed.then_some(index))
    }

    /// Removes working and blocked records when a new shell prompt begins.
    /// Retains idle, done, and error records. Use
    /// [`on_prompt_with_changes`](Self::on_prompt_with_changes) for notifications.
    pub fn on_prompt(&mut self) {
        self.remove_active(&mut |_| {});
    }

    /// Applies prompt cleanup when the process attached to this terminal exits.
    /// This event is not inferred from record ids or child process ids.
    /// Use [`on_process_exit_with_changes`](Self::on_process_exit_with_changes)
    /// for notifications.
    pub fn on_process_exit(&mut self) {
        self.on_prompt();
    }

    /// Removes all records on a full terminal reset (RIS).
    /// Use [`on_full_reset_with_changes`](Self::on_full_reset_with_changes)
    /// for notifications.
    pub fn on_full_reset(&mut self) {
        self.remove_matching(|_| true, &mut |_| {});
    }

    fn find(&self, id: Option<&str>) -> Option<&StoredRecord> {
        self.slots
            .iter()
            .filter_map(|slot| slot.record.as_ref())
            .find(|record| record.matches_id(id))
    }

    fn remove_active(&mut self, on_remove: &mut impl FnMut(Record<'_>)) {
        self.remove_matching(
            |record| matches!(record.state(), State::Working | State::Blocked),
            on_remove,
        );
    }

    fn remove_matching(
        &mut self,
        matches: impl Fn(Record<'_>) -> bool,
        on_remove: &mut impl FnMut(Record<'_>),
    ) {
        for index in 0..self.slots.len() {
            if let Some(record) = self.slots[index].record.as_ref()
                && matches(record.view())
            {
                on_remove(record.view());
                self.discard(index);
            }
        }
    }

    fn discard(&mut self, index: usize) {
        let rank = self.slots[index].rank;
        self.slots[index].record = None;
        self.length -= 1;
        // Active ranks stay dense in 0..length; repeated updates cannot overflow.
        for slot in &mut *self.slots {
            if slot.record.is_some() && slot.rank > rank {
                slot.rank -= 1;
            }
        }
    }
}

fn validate_capacity(capacity: usize) -> Result<(), Error> {
    if capacity == 0 || capacity > MAX_RECORDS {
        return Err(Error::InvalidCapacity);
    }
    Ok(())
}

/// Store capacity, allocation, or field preparation failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Requested capacity is zero or exceeds the protocol record cap.
    #[error("record capacity must be between 1 and 256")]
    InvalidCapacity,
    /// Heap storage could not be reserved. Requires the `alloc` feature.
    #[cfg(feature = "alloc")]
    #[error("{0}")]
    Allocation(#[from] alloc::collections::TryReserveError),
    /// Copying or decoding a validated report field failed.
    #[error("{0}")]
    Parser(#[from] parser::Error),
}
