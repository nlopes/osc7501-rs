//! Maximum-field storage and allocation checks on a bounded thread stack.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use osc7501_store::{
    Change, Error, MAX_RECORDS, Record, Slot, Store,
    parser::{
        self, AppId, Decoder, Event, MAX_APP, MAX_ID, MAX_MESSAGE, MAX_SEQUENCE, MAX_TITLE,
        RecordId, Report, State, parse_sequence,
    },
};
use std::mem::size_of;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

// Windows needs extra room for overflow handling and unoptimized value copies.
const STACK_SIZE: usize = if cfg!(windows) { 128 * 1024 } else { 64 * 1024 };
const STATES: [State; 5] = [
    State::Idle,
    State::Working,
    State::Blocked,
    State::Done,
    State::Error,
];

struct Workload {
    frames: Vec<Vec<u8>>,
    inherited_frames: Vec<Vec<u8>>,
    ids: Vec<Option<String>>,
    app: String,
    title: String,
    message: String,
    clear_children: String,
    invalid_input: Vec<u8>,
    transient: usize,
}

impl Workload {
    fn new() -> Self {
        let app = "a".repeat(MAX_APP);
        let title = "é".repeat(MAX_TITLE / 2);
        let message = "🦀".repeat(MAX_MESSAGE / 4);
        let segment = "b".repeat(32);
        let prefix = format!("{segment}/{segment}/{segment}");
        let ids: Vec<_> = (0..MAX_RECORDS)
            .map(|index| (index != 0).then(|| format!("{prefix}/{index:029}")))
            .collect();
        let encoded_title = STANDARD.encode(&title);
        let encoded_message = STANDARD.encode(&message);
        let frames: Vec<Vec<u8>> = ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let id = id.as_ref().map_or_else(String::new, |id| format!(":id={id}"));
                format!(
                    "\x1b]7501;state={}{id}:app={app}:kind=permission:progress=100:title={encoded_title}:msg={encoded_message}\x1b\\",
                    STATES[index % STATES.len()].as_str()
                ).into_bytes()
            })
            .collect();
        let inherited_frames = frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                if index == 0 {
                    frame.clone()
                } else {
                    String::from_utf8_lossy(frame)
                        .replace(&format!(":app={app}"), "")
                        .into_bytes()
                }
            })
            .collect();
        let clear_children = format!("\x1b]7501;state=clear:id={segment}\x1b\\");
        let mut invalid_input = b"\x1b]7501;state=future\x07\x1b]7501;state=working:id=bad!\x07\x1b]7501;state=clear:msg=A\x07\x1b]7501;state=working:msg=AA==\x07\x1b]7501;state=done:id=extra:msg=/w==\x07".to_vec();
        invalid_input.extend_from_slice(b"\x1b]7501;state=done:id=extra:msg=");
        invalid_input.extend(std::iter::repeat_n(b'A', MAX_SEQUENCE));
        invalid_input.extend_from_slice(b"\x1b\\");
        let transient = (0..MAX_RECORDS)
            .filter(|index| {
                matches!(
                    STATES[index % STATES.len()],
                    State::Working | State::Blocked
                )
            })
            .count();
        Self {
            frames,
            inherited_frames,
            ids,
            app,
            title,
            message,
            clear_children,
            invalid_input,
            transient,
        }
    }
}

fn only_removals(mut receive: impl FnMut(Record<'_>)) -> impl FnMut(Change<'_>) {
    move |change| {
        if let Change::Remove(record) = change {
            receive(record);
        }
    }
}

fn report(frame: &[u8]) -> Result<Report<'_>, Error> {
    match parse_sequence(frame)? {
        Some(Event::Report(report)) => Ok(report),
        _ => Err(parser::Error::Framing.into()),
    }
}

fn fill(store: &mut Store<'_>, frames: &[Vec<u8>]) -> Result<(), Error> {
    for frame in frames {
        store.apply(report(frame)?)?;
    }
    Ok(())
}

fn observe(
    store: &mut Store<'_>,
    decoder: &mut Decoder,
    input: &[u8],
) -> Result<(usize, usize), Error> {
    let (mut accepted, mut rejected) = (0, 0);
    let mut failure = None;
    for chunk in input.chunks(31) {
        decoder.feed(chunk, |event| match event {
            Ok(Event::Report(report)) => {
                accepted += 1;
                if let Err(error) = store.apply(report) {
                    failure = Some(error);
                }
            }
            Err(_) => rejected += 1,
            _ => {}
        });
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok((accepted, rejected))
}

fn check_records(store: &Store<'_>, workload: &Workload) -> Result<(), Error> {
    for (index, id) in workload.ids.iter().enumerate() {
        let id = id.as_deref().map(RecordId::new).transpose()?;
        let record = store.get(id).ok_or(parser::Error::Framing)?;
        assert_eq!(record.id(), id);
        assert_eq!(record.id().map(|id| id.as_str().len()), id.map(|_| MAX_ID));
        assert_eq!(record.state(), STATES[index % STATES.len()]);
        assert_eq!(record.title(), Some(workload.title.as_str()));
        assert_eq!(record.message(), Some(workload.message.as_str()));
        assert_eq!(record.app().map(AppId::as_str), Some(workload.app.as_str()));
        assert_eq!(
            store.effective_app(id).map(AppId::as_str),
            Some(workload.app.as_str())
        );
    }
    assert_eq!(store.iter().count(), MAX_RECORDS);
    assert_eq!(
        store
            .iter()
            .filter_map(Record::message)
            .map(str::len)
            .sum::<usize>(),
        MAX_RECORDS * MAX_MESSAGE
    );
    Ok(())
}

fn exercise(store: &mut Store<'_>, workload: &Workload) -> Result<(), Error> {
    let mut decoder = Decoder::new();
    for frame in &workload.frames {
        assert_eq!(observe(store, &mut decoder, frame)?, (1, 0));
    }
    assert_eq!(store.len(), MAX_RECORDS);
    assert_eq!(
        observe(store, &mut decoder, &workload.invalid_input)?,
        (0, 4)
    );
    assert_eq!(store.len(), MAX_RECORDS);
    check_records(store, workload)?;

    let mut removals = 0;
    store.apply_with_changes(
        report(&workload.frames[0])?,
        only_removals(|_| removals += 1),
    )?;
    assert_eq!(removals, 0);
    store.apply_with_changes(
        report(b"\x1b]7501;state=done:id=extra\x1b\\")?,
        only_removals(|record| {
            assert_eq!(
                record.id().map(RecordId::as_str),
                workload.ids[1].as_deref()
            );
            assert_eq!(record.message(), Some(workload.message.as_str()));
            removals += 1;
        }),
    )?;
    assert_eq!(removals, 1);
    assert_eq!(store.len(), MAX_RECORDS);
    store.on_prompt_with_changes(only_removals(|record| {
        assert!(matches!(record.state(), State::Working | State::Blocked));
        removals += 1;
    }));
    assert_eq!(store.len(), MAX_RECORDS - workload.transient + 1);
    assert_eq!(removals, workload.transient);
    store.on_full_reset();
    assert!(store.is_empty());

    fill(store, &workload.frames)?;
    let mut exited = 0;
    store.on_process_exit_with_changes(only_removals(|record| {
        assert!(matches!(record.state(), State::Working | State::Blocked));
        exited += 1;
    }));
    assert_eq!(exited, workload.transient);
    assert_eq!(store.len(), MAX_RECORDS - workload.transient);
    store.on_full_reset();

    fill(store, &workload.frames)?;
    let mut cleared = 0;
    store.apply_with_changes(
        report(workload.clear_children.as_bytes())?,
        only_removals(|record| {
            assert!(record.id().is_some());
            assert_eq!(record.message(), Some(workload.message.as_str()));
            cleared += 1;
        }),
    )?;
    assert_eq!(cleared, MAX_RECORDS - 1);
    assert_eq!(store.len(), 1);
    store.apply_with_changes(
        report(b"\x1b]7501;state=clear\x1b\\")?,
        only_removals(|_| cleared += 1),
    )?;
    assert_eq!(cleared, MAX_RECORDS);
    assert!(store.is_empty());
    Ok(())
}

fn exercise_changes(store: &mut Store<'_>, workload: &Workload) -> Result<(), Error> {
    fill(store, &workload.frames)?;
    let mut changes = 0;
    for frame in &workload.frames {
        store.apply_with_changes(report(frame)?, |_| changes += 1)?;
    }
    assert_eq!(changes, 0);
    store.on_full_reset_with_changes(|change| {
        assert!(matches!(change, Change::Remove(_)));
        changes += 1;
    });
    assert_eq!(changes, MAX_RECORDS);

    fill(store, &workload.inherited_frames)?;
    changes = 0;
    store.apply_with_changes(report(b"\x1b]7501;state=working:app=next\x07")?, |change| {
        assert!(
            matches!(change, Change::Set { effective_app: Some(app), .. } if app.as_str() == "next")
        );
        changes += 1;
    })?;
    assert_eq!(changes, MAX_RECORDS);
    let mut removed = 0;
    let mut updated = 0;
    store.on_prompt_with_changes(|change| match change {
        Change::Remove(_) => removed += 1,
        Change::Set { effective_app, .. } => {
            assert_eq!(effective_app, None);
            updated += 1;
        }
        _ => {}
    });
    assert_eq!(removed, workload.transient + 1);
    assert_eq!(updated, MAX_RECORDS - removed);
    store.on_full_reset_with_changes(|_| {});
    fill(store, &workload.inherited_frames)?;
    removed = 0;
    store.on_process_exit_with_changes(|change| {
        assert!(matches!(change, Change::Remove(_)));
        removed += 1;
    });
    assert_eq!(removed, workload.transient);
    store.on_full_reset_with_changes(|_| {});
    assert!(store.is_empty());
    Ok(())
}

#[test]
fn maximum_workload_is_allocation_free_on_a_bounded_thread_stack() -> TestResult {
    let mut slots: Vec<_> = (0..MAX_RECORDS).map(|_| Slot::EMPTY).collect();
    let workload = Workload::new();
    std::thread::Builder::new()
        .name("store-resource-check".into())
        .stack_size(STACK_SIZE)
        .spawn(move || maximum_workload(&mut slots, &workload))?
        .join()
        .map_err(|_| "maximum workload thread failed")?
}

fn maximum_workload(slots: &mut [Slot], workload: &Workload) -> TestResult {
    let resident = size_of::<[Slot; MAX_RECORDS]>() + size_of::<Store<'_>>() + size_of::<Decoder>();
    let with_scratch = resident + size_of::<Slot>() + MAX_MESSAGE;
    assert!(with_scratch < 1_000_000);
    assert!(resident + STACK_SIZE < 1_000_000);
    eprintln!(
        "slot={} slots={} store={} decoder={} scratch_upper={} total_upper={} with_thread_stack={}",
        size_of::<Slot>(),
        size_of::<[Slot; MAX_RECORDS]>(),
        size_of::<Store<'_>>(),
        size_of::<Decoder>(),
        size_of::<Slot>() + MAX_MESSAGE,
        with_scratch,
        resident + STACK_SIZE
    );
    let mut result = Ok(());
    let info = allocation_counter::measure(|| {
        result = Store::from_slots(slots).and_then(|mut store| {
            exercise(&mut store, workload)?;
            exercise_changes(&mut store, workload)
        });
    });
    result?;
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
    Ok(())
}

#[cfg(feature = "alloc")]
#[test]
fn invalid_owned_capacity_does_not_allocate() {
    let info = allocation_counter::measure(|| {
        for capacity in [0, MAX_RECORDS + 1, usize::MAX] {
            assert!(matches!(Store::new(capacity), Err(Error::InvalidCapacity)));
        }
    });
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn owned_storage_allocates_once_on_a_bounded_thread_stack() -> TestResult {
    let workload = Workload::new();
    std::thread::Builder::new()
        .name("owned-store-resource-check".into())
        .stack_size(STACK_SIZE)
        .spawn(move || maximum_owned_workload(&workload))?
        .join()
        .map_err(|_| "owned maximum workload thread failed")?
}

#[cfg(feature = "alloc")]
fn maximum_owned_workload(workload: &Workload) -> TestResult {
    let mut result = Err(Error::InvalidCapacity);
    let creation = allocation_counter::measure(|| result = Store::new(MAX_RECORDS));
    let mut store = result?;
    assert_eq!(creation.count_total, 1);
    assert_eq!(creation.count_current, 1);
    let storage_bytes = usize::try_from(creation.bytes_current)?;
    assert!(storage_bytes >= size_of::<[Slot; MAX_RECORDS]>());
    assert!(storage_bytes + size_of::<Store<'_>>() + STACK_SIZE < 1_000_000);

    let mut result = Ok(());
    let updates = allocation_counter::measure(|| {
        result =
            exercise(&mut store, workload).and_then(|()| exercise_changes(&mut store, workload));
    });
    result?;
    assert_eq!(updates.count_total, 0);
    assert_eq!(updates.bytes_max, 0);
    assert_eq!(store.capacity(), MAX_RECORDS);

    let cleanup = allocation_counter::measure(|| drop(store));
    assert_eq!(cleanup.count_total, 0);
    assert_eq!(creation.count_current + cleanup.count_current, 0);
    assert_eq!(creation.bytes_current + cleanup.bytes_current, 0);
    eprintln!(
        "owned_storage_bytes={} construction_allocations={} operation_allocations={}",
        creation.bytes_total, creation.count_total, updates.count_total
    );
    Ok(())
}
