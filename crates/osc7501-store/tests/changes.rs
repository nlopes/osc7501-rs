//! Notifications include final inherited apps without changing update order.

use osc7501_store::{
    Change, Error, Slot, Store,
    parser::{self, Event, RecordId, Report, parse_body},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Seen {
    Remove(String),
    Set(String, Option<String>),
}

fn report(body: &[u8]) -> Result<Report<'_>, parser::Error> {
    match parse_body(body)? {
        Some(Event::Report(report)) => Ok(report),
        _ => Err(parser::Error::Framing),
    }
}

fn receive(output: &mut Vec<Seen>, change: Change<'_>) {
    match change {
        Change::Remove(record) => output.push(Seen::Remove(
            record.id().map_or("", RecordId::as_str).into(),
        )),
        Change::Set {
            record,
            effective_app,
        } => output.push(Seen::Set(
            record.id().map_or("", RecordId::as_str).into(),
            effective_app.map(|app| app.as_str().into()),
        )),
        _ => {}
    }
}

fn apply(store: &mut Store<'_>, body: &[u8]) -> Result<Vec<Seen>, Error> {
    let mut output = Vec::new();
    store.apply_with_changes(report(body)?, |change| receive(&mut output, change))?;
    output.sort();
    Ok(output)
}

fn set(id: &str, app: Option<&str>) -> Seen {
    Seen::Set(id.into(), app.map(str::to_owned))
}

#[test]
fn parent_changes_notify_only_descendants_whose_app_changes() -> TestResult {
    let mut slots: Vec<_> = (0..8).map(|_| Slot::EMPTY).collect();
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:app=root")?;
    apply(&mut store, b"state=done:id=a/missing/leaf")?;
    apply(&mut store, b"state=done:id=a/own:app=local")?;
    apply(&mut store, b"state=done:id=a/own/leaf")?;
    apply(&mut store, b"state=done:id=ab/leaf")?;
    assert_eq!(
        apply(&mut store, b"state=done:id=a:app=inner")?,
        vec![
            set("a", Some("inner")),
            set("a/missing/leaf", Some("inner")),
        ]
    );
    assert_eq!(
        apply(&mut store, b"state=done:id=a")?,
        vec![set("a", Some("root")), set("a/missing/leaf", Some("root")),]
    );
    assert_eq!(
        apply(&mut store, b"state=done:app=next")?,
        vec![
            set("", Some("next")),
            set("a", Some("next")),
            set("a/missing/leaf", Some("next")),
            set("ab/leaf", Some("next")),
        ]
    );
    Ok(())
}

#[test]
fn matching_app_values_do_not_notify_descendants_when_source_changes() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 3];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:app=same")?;
    apply(&mut store, b"state=done:id=a/leaf")?;
    assert_eq!(
        apply(&mut store, b"state=working:id=a:app=same")?,
        vec![set("a", Some("same"))]
    );
    let mut changes = Vec::new();
    store.on_prompt_with_changes(|change| receive(&mut changes, change));
    assert_eq!(changes, vec![Seen::Remove("a".into())]);
    Ok(())
}

#[test]
fn eviction_and_insertion_report_final_inheritance_once() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 2];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:app=old")?;
    apply(&mut store, b"state=done:id=a/leaf")?;
    let mut changes = Vec::new();
    store.apply_with_changes(report(b"state=done:id=a:app=new")?, |change| {
        receive(&mut changes, change);
    })?;
    assert_eq!(changes.first(), Some(&Seen::Remove(String::new())));
    changes.sort();
    assert_eq!(
        changes,
        vec![
            Seen::Remove(String::new()),
            set("a", Some("new")),
            set("a/leaf", Some("new"))
        ]
    );
    Ok(())
}

#[test]
fn lifecycle_cleanup_notifies_only_final_survivors() -> TestResult {
    for exit in [false, true] {
        let mut slots = [const { Slot::EMPTY }; 4];
        let mut store = Store::from_slots(&mut slots)?;
        apply(&mut store, b"state=working:app=root")?;
        apply(&mut store, b"state=blocked:id=a:app=inner")?;
        apply(&mut store, b"state=done:id=a/leaf")?;
        apply(&mut store, b"state=error:id=ab/leaf")?;
        let mut changes = Vec::new();
        if exit {
            store.on_process_exit_with_changes(|change| receive(&mut changes, change));
        } else {
            store.on_prompt_with_changes(|change| receive(&mut changes, change));
        }
        assert!(
            changes[..2]
                .iter()
                .all(|change| matches!(change, Seen::Remove(_)))
        );
        changes.sort();
        assert_eq!(
            changes,
            vec![
                Seen::Remove(String::new()),
                Seen::Remove("a".into()),
                set("a/leaf", None),
                set("ab/leaf", None)
            ]
        );
        changes.clear();
        store.on_full_reset_with_changes(|change| receive(&mut changes, change));
        changes.sort();
        assert_eq!(
            changes,
            vec![
                Seen::Remove("a/leaf".into()),
                Seen::Remove("ab/leaf".into())
            ]
        );
        changes.clear();
        store.on_full_reset_with_changes(|change| receive(&mut changes, change));
        assert_eq!(changes, []);
    }
    Ok(())
}

#[test]
fn semantic_duplicates_are_silent_but_still_refresh_update_order() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 2];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:id=a:msg=T0s=")?;
    apply(&mut store, b"state=done:id=b")?;
    assert_eq!(apply(&mut store, b"state=done:id=a:msg=T0s")?, []);
    assert_eq!(
        apply(&mut store, b"state=done:id=c")?,
        vec![Seen::Remove("b".into()), set("c", None)]
    );
    assert_eq!(
        apply(&mut store, b"state=done:id=a:msg=")?,
        vec![set("a", None)]
    );
    assert_eq!(apply(&mut store, b"state=done:id=a")?, vec![set("a", None)]);
    Ok(())
}

#[test]
fn every_explicit_field_can_trigger_a_set() -> TestResult {
    let mut slots = [Slot::EMPTY];
    let mut store = Store::from_slots(&mut slots)?;
    let base = b"state=blocked:id=a";
    for body in [
        b"state=working:id=a".as_slice(),
        b"state=blocked:id=a:app=cargo",
        b"state=blocked:id=a:title=VGVzdCDwn6aA",
        b"state=blocked:id=a:msg=",
        b"state=blocked:id=a:kind=auth",
        b"state=blocked:id=a:progress=42",
    ] {
        apply(&mut store, base)?;
        let updates = apply(&mut store, body)?;
        assert_eq!(updates.len(), 1, "{body:?}");
        assert!(matches!(&updates[0], Seen::Set(id, _) if id == "a"));
        assert_eq!(apply(&mut store, body)?, []);
        assert_eq!(apply(&mut store, base)?, vec![set("a", None)]);
    }
    Ok(())
}

#[test]
fn subtree_clear_preserves_slash_boundaries_and_absent_parent_semantics() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 3];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:app=root")?;
    apply(&mut store, b"state=done:id=a/missing/leaf")?;
    apply(&mut store, b"state=done:id=ab/leaf")?;
    assert_eq!(
        apply(&mut store, b"state=clear:id=a")?,
        vec![Seen::Remove("a/missing/leaf".into())]
    );
    assert_eq!(apply(&mut store, b"state=clear:id=a")?, []);
    assert_eq!(
        apply(&mut store, b"state=clear")?,
        vec![Seen::Remove(String::new()), Seen::Remove("ab/leaf".into())]
    );
    Ok(())
}
