//! Store observations against an independent model ordered by last update.
use osc7501_store::{
    Change, Error, Record, Slot, Store,
    parser::{self, Action, AppId, Event, Kind, Progress, RecordId, Report, State, parse_body},
};
use proptest::{
    collection::vec,
    prelude::*,
    test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
};
use std::collections::BTreeMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Value {
    id: Option<String>,
    state: State,
    app: Option<String>,
    title: Option<String>,
    message: Option<String>,
    kind: Option<Kind>,
    progress: Option<Progress>,
}

impl Value {
    fn from_record(record: Record<'_>) -> Self {
        Self {
            id: record.id().map(|id| id.as_str().to_owned()),
            state: record.state(),
            app: record.app().map(|app| app.as_str().to_owned()),
            title: record.title().map(str::to_owned),
            message: record.message().map(str::to_owned),
            kind: record.kind(),
            progress: record.progress(),
        }
    }

    fn from_report(report: Report<'_>, state: State) -> Result<Self, parser::Error> {
        let decode = |text: parser::EncodedText<'_>| {
            let mut bytes = vec![0; text.decoded_len()];
            text.decode_into(&mut bytes).map(str::to_owned)
        };
        Ok(Self {
            id: report.id().map(|id| id.as_str().to_owned()),
            state,
            app: report.app().map(|app| app.as_str().to_owned()),
            title: report.title().map(decode).transpose()?,
            message: report.message().map(decode).transpose()?,
            kind: report.kind(),
            progress: report.progress(),
        })
    }
}

fn sorted(mut values: Vec<Value>) -> Vec<Value> {
    values.sort_by(|left, right| left.id.cmp(&right.id));
    values
}

fn report(body: &[u8]) -> Result<Report<'_>, parser::Error> {
    match parse_body(body)? {
        Some(Event::Report(report)) => Ok(report),
        _ => Err(parser::Error::Framing),
    }
}

#[derive(Clone, Copy, Debug)]
enum Command<'a> {
    Report(&'a [u8]),
    Prompt,
    Exit,
    Reset,
    Read,
}

fn commands() -> impl Strategy<Value = Vec<Command<'static>>> {
    let bodies: Vec<&'static [u8]> = vec![
        b"state=idle:app=root",
        b"state=done:app=other",
        b"state=done",
        b"state=done:id=a:app=root",
        b"state=done:id=a:app=inner",
        b"state=error:id=a:msg=T0s=",
        b"state=error:id=a:msg=T0s",
        b"state=working:id=a:progress=42",
        b"state=blocked:id=a:kind=auth:app=inner",
        b"state=done:id=a/b:title=:msg=",
        b"state=done:id=a/b/c:msg=8J+mgA==",
        b"state=working:id=ab",
        b"state=error:id=b:app=other",
        b"state=clear:id=a",
        b"state=clear:id=a/b",
        b"state=clear",
    ];
    vec(
        prop_oneof![
            8 => proptest::sample::select(bodies).prop_map(Command::Report),
            1 => Just(Command::Prompt),
            1 => Just(Command::Exit),
            1 => Just(Command::Reset),
            2 => Just(Command::Read),
        ],
        0..80,
    )
}

struct Model {
    capacity: usize,
    oldest_first: Vec<Value>,
}

impl Model {
    fn apply(&mut self, command: Command<'_>) -> Result<Vec<Value>, parser::Error> {
        let mut removed = Vec::new();
        match command {
            Command::Report(body) => {
                let report = report(body)?;
                let id = report.id().map(RecordId::as_str);
                match report.action() {
                    Action::Set(state) => {
                        let value = Value::from_report(report, state)?;
                        if let Some(index) =
                            self.oldest_first.iter().position(|r| r.id.as_deref() == id)
                        {
                            self.oldest_first.remove(index);
                        } else if self.oldest_first.len() == self.capacity {
                            removed.push(self.oldest_first.remove(0));
                        }
                        self.oldest_first.push(value);
                    }
                    Action::Clear => self.oldest_first.retain(|value| {
                        let within = id.is_none_or(|parent| {
                            value.id.as_deref().is_some_and(|child| {
                                child == parent || child.starts_with(&format!("{parent}/"))
                            })
                        });
                        if within {
                            removed.push(value.clone());
                        }
                        !within
                    }),
                }
            }
            Command::Prompt | Command::Exit | Command::Reset => {
                self.oldest_first.retain(|value| {
                    let remove = matches!(command, Command::Reset)
                        || matches!(value.state, State::Working | State::Blocked);
                    if remove {
                        removed.push(value.clone());
                    }
                    !remove
                });
            }
            Command::Read => {}
        }
        Ok(sorted(removed))
    }

    fn effective_app(&self, id: Option<&str>) -> Option<&str> {
        self.oldest_first.iter().find(|r| r.id.as_deref() == id)?;
        self.oldest_first
            .iter()
            .filter(|r| {
                r.app.is_some()
                    && r.id.as_deref().is_none_or(|parent| {
                        id.is_some_and(|child| {
                            child == parent || child.starts_with(&format!("{parent}/"))
                        })
                    })
            })
            .max_by_key(|r| r.id.as_ref().map_or(0, String::len))
            .and_then(|r| r.app.as_deref())
    }
}

fn step(store: &mut Store<'_>, command: Command<'_>) -> Result<(), Error> {
    match command {
        Command::Report(body) => store.apply(report(body)?)?,
        Command::Prompt => store.on_prompt(),
        Command::Exit => store.on_process_exit(),
        Command::Reset => store.on_full_reset(),
        Command::Read => {
            for record in store.iter() {
                assert!(store.get(record.id()).is_some());
                let _ = store.effective_app(record.id());
            }
        }
    }
    Ok(())
}

fn check(store: &Store<'_>, model: &Model) -> Result<(), proptest::test_runner::TestCaseError> {
    prop_assert_eq!(store.len(), model.oldest_first.len());
    prop_assert_eq!(
        sorted(store.iter().map(Value::from_record).collect()),
        sorted(model.oldest_first.clone())
    );
    for id in [
        None,
        Some("a"),
        Some("a/b"),
        Some("a/b/c"),
        Some("ab"),
        Some("b"),
    ] {
        let actual = store
            .effective_app(id.map(RecordId::new).transpose()?)
            .map(AppId::as_str);
        prop_assert_eq!(actual, model.effective_app(id));
    }
    Ok(())
}

#[test]
fn plain_operations_match_update_order_model() -> TestResult {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 128,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    runner.run(
        &(
            proptest::sample::select(vec![1, 2, 3, 8, 64, 256]),
            commands(),
        ),
        |(capacity, commands)| {
            let mut slots: Vec<_> = (0..capacity).map(|_| Slot::EMPTY).collect();
            let mut borrowed = Store::from_slots(&mut slots)?;
            #[cfg(feature = "alloc")]
            let mut owned = Store::new(capacity)?;
            let mut model = Model {
                capacity,
                oldest_first: Vec::new(),
            };
            // New ids expose any update-order difference hidden by the current records.
            let probes: Vec<_> = (0..=capacity)
                .map(|id| format!("state=done:id=probe/{id}"))
                .collect();
            for command in commands
                .into_iter()
                .chain(probes.iter().map(|body| Command::Report(body.as_bytes())))
            {
                model.apply(command)?;
                step(&mut borrowed, command)?;
                check(&borrowed, &model)?;
                #[cfg(feature = "alloc")]
                {
                    step(&mut owned, command)?;
                    check(&owned, &model)?;
                }
            }
            Ok(())
        },
    )?;
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct View {
    record: Value,
    effective_app: Option<String>,
}

fn visible(model: &Model) -> BTreeMap<Option<String>, View> {
    model
        .oldest_first
        .iter()
        .map(|record| {
            (
                record.id.clone(),
                View {
                    record: record.clone(),
                    effective_app: model.effective_app(record.id.as_deref()).map(str::to_owned),
                },
            )
        })
        .collect()
}

fn changes(store: &mut Store<'_>, command: Command<'_>) -> Result<(Vec<Value>, Vec<View>), Error> {
    let mut removed = Vec::new();
    let mut sets = Vec::new();
    let receive = |change: Change<'_>| match change {
        Change::Remove(record) => {
            assert_eq!(sets.len(), 0, "all removals must precede sets");
            removed.push(Value::from_record(record));
        }
        Change::Set {
            record,
            effective_app,
        } => sets.push(View {
            record: Value::from_record(record),
            effective_app: effective_app.map(|app| app.as_str().to_owned()),
        }),
        _ => {}
    };
    match command {
        Command::Report(body) => store.apply_with_changes(report(body)?, receive)?,
        Command::Prompt => store.on_prompt_with_changes(receive),
        Command::Exit => store.on_process_exit_with_changes(receive),
        Command::Reset => store.on_full_reset_with_changes(receive),
        Command::Read => {}
    }
    sets.sort_by(|left, right| left.record.id.cmp(&right.record.id));
    Ok((sorted(removed), sets))
}

#[test]
fn notifications_match_independent_visible_state_differences() -> TestResult {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 128,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    );
    runner.run(
        &(proptest::sample::select(vec![1, 2, 3, 8, 64]), commands()),
        |(capacity, commands)| {
            let mut slots: Vec<_> = (0..capacity).map(|_| Slot::EMPTY).collect();
            let mut borrowed = Store::from_slots(&mut slots)?;
            #[cfg(feature = "alloc")]
            let mut owned = Store::new(capacity)?;
            let mut model = Model {
                capacity,
                oldest_first: Vec::new(),
            };
            let probes: Vec<_> = (0..=capacity)
                .map(|id| format!("state=done:id=probe/{id}"))
                .collect();
            for command in commands
                .into_iter()
                .chain(probes.iter().map(|body| Command::Report(body.as_bytes())))
            {
                let before = visible(&model);
                let removed = model.apply(command)?;
                let sets: Vec<_> = visible(&model)
                    .into_iter()
                    .filter(|(id, value)| before.get(id) != Some(value))
                    .map(|(_, value)| value)
                    .collect();
                let expected = (removed, sets);
                prop_assert_eq!(&changes(&mut borrowed, command)?, &expected);
                check(&borrowed, &model)?;
                #[cfg(feature = "alloc")]
                {
                    prop_assert_eq!(&changes(&mut owned, command)?, &expected);
                    check(&owned, &model)?;
                }
            }
            Ok(())
        },
    )?;
    Ok(())
}

#[test]
fn unchanged_updates_and_reordered_ids_change_future_eviction() -> TestResult {
    for history in [vec!["a", "b"], vec!["a", "b", "a"], vec!["b", "a"]] {
        let mut slots = [const { Slot::EMPTY }; 2];
        let mut store = Store::from_slots(&mut slots)?;
        for id in &history {
            step(
                &mut store,
                Command::Report(format!("state=done:id={id}").as_bytes()),
            )?;
        }
        let current = sorted(store.iter().map(Value::from_record).collect());
        assert_eq!(
            current
                .iter()
                .filter_map(|r| r.id.as_deref())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        let (removed, _) = changes(&mut store, Command::Report(b"state=done:id=c"))?;
        assert_eq!(removed.len(), 1);
        assert_eq!(
            removed[0].id.as_deref(),
            if history == ["a", "b"] {
                Some("a")
            } else {
                Some("b")
            }
        );
    }
    Ok(())
}
