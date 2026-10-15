//! Maximum-field storage and allocation checks on a bounded thread stack.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use osc7501_store::{
    Error, MAX_RECORDS, Record, Slot, Store,
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
        let frames = ids
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

fn report(frame: &[u8]) -> Result<Report<'_>, Error> {
    match parse_sequence(frame)? {
        Some(Event::Report(report)) => Ok(report),
        _ => Err(parser::Error::Framing.into()),
    }
}

fn fill(store: &mut Store<'_>, frames: &[Vec<u8>]) -> Result<(), Error> {
    for frame in frames {
        store.apply(report(frame)?, |_| {})?;
    }
    Ok(())
}

fn observe(
    store: &mut Store<'_>,
    decoder: &mut Decoder,
    input: &[u8],
) -> Result<(usize, usize, usize), Error> {
    let (mut accepted, mut rejected, mut removed) = (0, 0, 0);
    let mut failure = None;
    for chunk in input.chunks(31) {
        decoder.feed(chunk, |event| match event {
            Ok(Event::Report(report)) => {
                accepted += 1;
                if let Err(error) = store.apply(report, |_| removed += 1) {
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
    Ok((accepted, rejected, removed))
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

fn exercise(slots: &mut [Slot], workload: &Workload) -> Result<(), Error> {
    let mut store = Store::new(slots)?;
    let mut decoder = Decoder::new();
    for frame in &workload.frames {
        assert_eq!(observe(&mut store, &mut decoder, frame)?, (1, 0, 0));
    }
    assert_eq!(store.len(), MAX_RECORDS);
    assert_eq!(
        observe(&mut store, &mut decoder, &workload.invalid_input)?,
        (0, 4, 0)
    );
    assert_eq!(store.len(), MAX_RECORDS);
    check_records(&store, workload)?;

    let mut removals = 0;
    store.apply(report(&workload.frames[0])?, |_| removals += 1)?;
    assert_eq!(removals, 0);
    store.apply(report(b"\x1b]7501;state=done:id=extra\x1b\\")?, |record| {
        assert_eq!(
            record.id().map(RecordId::as_str),
            workload.ids[1].as_deref()
        );
        assert_eq!(record.message(), Some(workload.message.as_str()));
        removals += 1;
    })?;
    assert_eq!(removals, 1);
    assert_eq!(store.len(), MAX_RECORDS);
    store.on_prompt(|record| {
        assert!(matches!(record.state(), State::Working | State::Blocked));
        removals += 1;
    });
    assert_eq!(store.len(), MAX_RECORDS - workload.transient + 1);
    assert_eq!(removals, workload.transient);
    store.on_full_reset(|_| {});
    assert!(store.is_empty());

    fill(&mut store, &workload.frames)?;
    let mut exited = 0;
    store.on_process_exit(|record| {
        assert!(matches!(record.state(), State::Working | State::Blocked));
        exited += 1;
    });
    assert_eq!(exited, workload.transient);
    assert_eq!(store.len(), MAX_RECORDS - workload.transient);
    store.on_full_reset(|_| {});

    fill(&mut store, &workload.frames)?;
    let mut cleared = 0;
    store.apply(report(workload.clear_children.as_bytes())?, |record| {
        assert!(record.id().is_some());
        assert_eq!(record.message(), Some(workload.message.as_str()));
        cleared += 1;
    })?;
    assert_eq!(cleared, MAX_RECORDS - 1);
    assert_eq!(store.len(), 1);
    store.apply(report(b"\x1b]7501;state=clear\x1b\\")?, |_| cleared += 1)?;
    assert_eq!(cleared, MAX_RECORDS);
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
    let info = allocation_counter::measure(|| result = exercise(slots, workload));
    result?;
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
    Ok(())
}
