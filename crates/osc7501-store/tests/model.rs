//! Store observations against an independent model ordered by last update.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hegel::{TestCase, generators as gs};
use osc7501_store::{
    Change, Error, Record, Slot, Store,
    parser::{self, Action, AppId, Event, Kind, Progress, RecordId, Report, State, parse_body},
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

#[hegel::test_helper]
fn commands(tc: &TestCase) -> Vec<Command<'static>> {
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
    let operations = tc.draw(gs::vecs(gs::integers::<u8>().max_value(12)).max_size(79));
    operations
        .into_iter()
        .map(|operation| match operation {
            8 => Command::Prompt,
            9 => Command::Exit,
            10 => Command::Reset,
            11 | 12 => Command::Read,
            _ => Command::Report(tc.draw(gs::sampled_from(&bodies))),
        })
        .collect()
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

fn check(store: &Store<'_>, model: &Model) -> TestResult {
    assert_eq!(store.len(), model.oldest_first.len());
    assert_eq!(
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
        assert_eq!(actual, model.effective_app(id));
    }
    Ok(())
}

#[hegel::test]
fn plain_operations_match_update_order_model(tc: TestCase) {
    let capacity = tc.draw(gs::sampled_from(&[1, 2, 3, 8, 64, 256]));
    let commands = commands(&tc);
    let result = check_plain_operations(capacity, &commands);
    assert!(result.is_ok(), "{result:?}");
}

fn check_plain_operations(capacity: usize, commands: &[Command<'_>]) -> TestResult {
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
        .iter()
        .copied()
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

#[hegel::test]
fn notifications_match_independent_visible_state_differences(tc: TestCase) {
    let capacity = tc.draw(gs::sampled_from(&[1, 2, 3, 8, 64]));
    let commands = commands(&tc);
    let result = check_notifications(capacity, &commands);
    assert!(result.is_ok(), "{result:?}");
}

fn check_notifications(capacity: usize, commands: &[Command<'_>]) -> TestResult {
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
        .iter()
        .copied()
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
        assert_eq!(&changes(&mut borrowed, command)?, &expected);
        check(&borrowed, &model)?;
        #[cfg(feature = "alloc")]
        {
            assert_eq!(&changes(&mut owned, command)?, &expected);
            check(&owned, &model)?;
        }
    }
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

#[hegel::test]
fn generated_fields_and_lifecycle_histories_match_the_model(tc: TestCase) {
    let capacity = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    let history = tc.draw(
        gs::vecs(gs::tuples!(
            gs::integers::<u8>().max_value(11),
            gs::sampled_from(&["idle", "working", "blocked", "done", "error", "clear"]),
            gs::optional(gs::sampled_from(&["a", "a/b", "a/b/c", "ab", "b"])),
            gs::optional(
                gs::text()
                    .alphabet("abcABC012._+-")
                    .min_size(1)
                    .max_size(32)
            ),
            gs::optional(gs::text().alphabet(" a:;=é€🦀").max_size(32)),
            gs::optional(gs::integers::<u8>().max_value(100)),
            gs::optional(gs::sampled_from(&["auth", "permission", "question"])),
        ))
        .max_size(60),
    );
    let mut commands = Vec::new();
    for (operation, state, id, app, text, progress, kind) in history {
        let mut fields = vec![format!("state={state}")];
        if let Some(id) = id {
            fields.push(format!("id={id}"));
        }
        if let Some(app) = app {
            fields.push(format!("app={app}"));
        }
        if let Some(text) = text {
            fields.push(format!(
                "title={}:msg={}",
                STANDARD.encode(&text),
                STANDARD.encode(&text)
            ));
        }
        if let Some(progress) = progress {
            fields.push(format!("progress={progress}"));
        }
        if let Some(kind) = kind {
            fields.push(format!("kind={kind}"));
        }
        commands.push((operation, fields.join(":")));
    }
    commands.extend((0..=capacity).map(|id| (0, format!("state=done:id=probe/{id}"))));
    let result = check_generated_history(capacity, &commands);
    assert!(result.is_ok(), "{result:?}");
}

fn check_generated_history(capacity: usize, commands: &[(u8, String)]) -> TestResult {
    let mut slots: Vec<_> = (0..capacity).map(|_| Slot::EMPTY).collect();
    let mut plain = Store::from_slots(&mut slots)?;
    let mut observed_slots: Vec<_> = (0..capacity).map(|_| Slot::EMPTY).collect();
    let mut observed = Store::from_slots(&mut observed_slots)?;
    #[cfg(feature = "alloc")]
    let mut owned = Store::new(capacity)?;
    let mut model = Model {
        capacity,
        oldest_first: Vec::new(),
    };
    for (operation, body) in commands {
        let command = match operation {
            8 => Command::Prompt,
            9 => Command::Exit,
            10 => Command::Reset,
            11 => Command::Read,
            _ => Command::Report(body.as_bytes()),
        };
        let before = visible(&model);
        let removed = model.apply(command)?;
        let sets: Vec<_> = visible(&model)
            .into_iter()
            .filter(|(id, value)| before.get(id) != Some(value))
            .map(|(_, value)| value)
            .collect();
        let expected = (removed, sets);
        step(&mut plain, command)?;
        check(&plain, &model)?;
        assert_eq!(changes(&mut observed, command)?, expected);
        check(&observed, &model)?;
        #[cfg(feature = "alloc")]
        {
            assert_eq!(changes(&mut owned, command)?, expected);
            check(&owned, &model)?;
        }
    }
    Ok(())
}
