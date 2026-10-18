//! Keep an owned view in sync across reports, prompts, process exit, and reset.

use std::collections::BTreeMap;

use osc7501_store::{
    Change, Error, MIN_HOST_CAPACITY, Store,
    parser::{Event, Report, State, parse_body},
};

#[derive(Clone)]
struct Task {
    state: State,
    app: Option<String>,
    title: Option<String>,
}

type View = BTreeMap<Option<String>, Task>;

#[derive(Clone, Copy)]
enum Input<'a> {
    Report(Report<'a>),
    Prompt,
    ProcessExit,
    FullReset,
}

fn handle(store: &mut Store<'_>, view: &mut View, input: Input<'_>) -> Result<(), Error> {
    let mut update = |change: Change<'_>| match change {
        Change::Set {
            record,
            effective_app,
        } => {
            let id = record.id().map(|id| id.as_str().to_owned());
            view.insert(
                id,
                Task {
                    state: record.state(),
                    app: effective_app.map(|app| app.as_str().to_owned()),
                    title: record.title().map(str::to_owned),
                },
            );
        }
        Change::Remove(record) => {
            let id = record.id().map(|id| id.as_str().to_owned());
            view.remove(&id);
        }
        _ => {}
    };
    match input {
        Input::Report(report) => store.apply_with_changes(report, &mut update)?,
        Input::Prompt => store.on_prompt_with_changes(&mut update),
        Input::ProcessExit => store.on_process_exit_with_changes(&mut update),
        Input::FullReset => store.on_full_reset_with_changes(&mut update),
    }
    Ok(())
}

fn report(store: &mut Store<'_>, view: &mut View, body: &[u8]) -> Result<(), Error> {
    // A terminal host supplies the OSC body after framing the input stream.
    if let Some(Event::Report(report)) = parse_body(body)? {
        handle(store, view, Input::Report(report))?;
    }
    Ok(())
}

fn show(label: &str, view: &View) {
    println!("{label}");
    if view.is_empty() {
        println!("  (empty)");
    }
    for (id, task) in view {
        println!(
            "  {}: {:?}, app={}, title={}",
            id.as_deref().unwrap_or("(root)"),
            task.state,
            task.app.as_deref().unwrap_or("(none)"),
            task.title.as_deref().unwrap_or("(none)"),
        );
    }
}

fn main() -> Result<(), Error> {
    let mut store = Store::new(MIN_HOST_CAPACITY)?;
    let mut view = View::new();
    report(
        &mut store,
        &mut view,
        b"state=done:id=build/test:title=VGVzdHM=",
    )?;
    report(&mut store, &mut view, b"state=working:app=cargo")?;
    show("Parent supplies the app:", &view);

    handle(&mut store, &mut view, Input::Prompt)?;
    show(
        "Prompt removes the working parent and inherited app:",
        &view,
    );

    report(&mut store, &mut view, b"state=blocked:app=deploy")?;
    handle(&mut store, &mut view, Input::ProcessExit)?;
    show("Process exit keeps the completed child:", &view);

    let saved = view.clone();
    handle(&mut store, &mut view, Input::FullReset)?;
    show("Full reset removes all current records:", &view);
    drop(store);
    show(
        "Saved data remains usable after the store is dropped:",
        &saved,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_updates_and_saved_views_own_their_data() -> Result<(), Error> {
        let mut store = Store::new(MIN_HOST_CAPACITY)?;
        let mut view = View::new();
        let child = Some("build/test".to_owned());
        report(
            &mut store,
            &mut view,
            b"state=done:id=build/test:title=VGVzdHM=",
        )?;
        for input in [Input::Prompt, Input::ProcessExit] {
            report(&mut store, &mut view, b"state=working:app=cargo")?;
            assert_eq!(
                view.get(&child).and_then(|task| task.app.as_deref()),
                Some("cargo")
            );
            handle(&mut store, &mut view, input)?;
            assert_eq!(view.len(), 1);
            assert_eq!(view.get(&child).and_then(|task| task.app.as_deref()), None);
        }
        handle(&mut store, &mut view, Input::FullReset)?;
        assert!(store.is_empty());
        assert!(view.is_empty());

        report(&mut store, &mut view, b"state=done:title=VGVzdHM=")?;
        drop(store);
        assert_eq!(view.get(&None).map(|task| task.state), Some(State::Done));
        assert_eq!(
            view.get(&None).and_then(|task| task.title.as_deref()),
            Some("Tests")
        );
        Ok(())
    }
}
