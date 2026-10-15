//! Current-record semantics through the parser and store public APIs.
use osc7501_store::{
    Error, MAX_RECORDS, MIN_HOST_CAPACITY, Record, Slot, Store,
    parser::{self, AppId, Decoder, Event, Kind, RecordId, Report, State, parse_body},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn report(body: &[u8]) -> Result<Report<'_>, parser::Error> {
    match parse_body(body)? {
        Some(Event::Report(report)) => Ok(report),
        _ => Err(parser::Error::Framing),
    }
}

fn apply(store: &mut Store<'_>, body: &[u8]) -> Result<(), Error> {
    store.apply(report(body)?, |_| {})
}

#[test]
fn capacity_limits_include_small_test_stores() -> TestResult {
    assert_eq!(MIN_HOST_CAPACITY, 64);
    assert_eq!(MAX_RECORDS, 256);
    for capacity in [0, 1, 63, 64, 256, 257] {
        let mut slots: Vec<_> = (0..capacity).map(|_| Slot::EMPTY).collect();
        let result = Store::from_slots(&mut slots);
        if matches!(capacity, 0 | 257) {
            assert!(matches!(result, Err(Error::InvalidCapacity)));
        } else {
            let store = result?;
            assert_eq!(store.capacity(), capacity);
            assert_eq!(store.len(), 0);
            assert!(store.is_empty());
            assert_eq!(store.iter().count(), 0);
        }
    }
    Ok(())
}

#[cfg(feature = "alloc")]
#[test]
fn owned_capacity_limits_include_small_test_stores() -> TestResult {
    for capacity in [0, 1, 63, 64, 256, 257, usize::MAX] {
        let result = Store::new(capacity);
        if (1..=MAX_RECORDS).contains(&capacity) {
            let store = result?;
            assert_eq!(store.capacity(), capacity);
            assert!(store.is_empty());
            assert_eq!(store.iter().count(), 0);
        } else {
            assert!(matches!(result, Err(Error::InvalidCapacity)));
        }
    }
    Ok(())
}

#[test]
fn replacement_removes_every_omitted_field() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 1];
    let mut store = Store::from_slots(&mut slots)?;
    let id = Some(RecordId::new("build/test")?);
    apply(
        &mut store,
        b"state=blocked:id=build/test:app=cargo:kind=permission:progress=42:title=VGVzdCDwn6aA:msg=RXNwYcOxYQ==",
    )?;
    let record = store.get(id).ok_or("missing child")?;
    assert_eq!(record.state(), State::Blocked);
    assert_eq!(record.id(), id);
    assert_eq!(record.app(), Some(AppId::new("cargo")?));
    assert_eq!(record.kind(), Some(Kind::Permission));
    assert_eq!(record.progress().map(parser::Progress::get), Some(42));
    assert_eq!(record.title(), Some("Test 🦀"));
    assert_eq!(record.message(), Some("España"));

    let mut removals = 0;
    store.apply(report(b"state=working:id=build/test")?, |_| removals += 1)?;
    let record = store.get(id).ok_or("missing replacement")?;
    assert_eq!(record.state(), State::Working);
    assert_eq!(record.app(), None);
    assert_eq!(record.kind(), None);
    assert_eq!(record.progress(), None);
    assert_eq!(record.title(), None);
    assert_eq!(record.message(), None);
    assert_eq!(removals, 0);
    assert_eq!(store.len(), 1);
    Ok(())
}

#[test]
fn explicitly_empty_text_is_present_until_omitted() -> TestResult {
    let mut slots = [Slot::EMPTY];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:title=:msg=")?;
    let record = store.get(None).ok_or("missing root")?;
    assert_eq!(record.title(), Some(""));
    assert_eq!(record.message(), Some(""));
    apply(&mut store, b"state=done")?;
    let record = store.get(None).ok_or("missing root")?;
    assert_eq!(record.title(), None);
    assert_eq!(record.message(), None);
    Ok(())
}

#[test]
fn root_counts_toward_capacity_and_can_be_evicted() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 2];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:app=cargo")?;
    apply(&mut store, b"state=done:id=build")?;
    assert_eq!(store.len(), 2);
    assert_eq!(
        store.iter().filter(|record| record.id().is_none()).count(),
        1
    );
    let mut removed = Vec::new();
    store.apply(report(b"state=working:id=test")?, |record| {
        removed.push((record.id().map(|id| id.as_str().to_owned()), record.state()));
    })?;
    assert_eq!(removed, [(None, State::Done)]);
    assert!(store.get(None).is_none());
    assert!(store.get(Some(RecordId::new("build")?)).is_some());
    assert!(store.get(Some(RecordId::new("test")?)).is_some());
    assert_eq!(store.len(), 2);
    Ok(())
}

#[test]
fn capacity_boundaries_replace_existing_and_evict_only_for_new_ids() -> TestResult {
    for capacity in [1, MIN_HOST_CAPACITY, MAX_RECORDS] {
        let mut slots: Vec<_> = (0..capacity).map(|_| Slot::EMPTY).collect();
        let mut store = Store::from_slots(&mut slots)?;
        apply(&mut store, b"state=done")?;
        for index in 1..capacity {
            apply(
                &mut store,
                format!("state=done:id=record{index}").as_bytes(),
            )?;
        }
        assert_eq!(store.len(), capacity);
        let mut removals = 0;
        store.apply(report(b"state=working")?, |_| removals += 1)?;
        assert_eq!(removals, 0);
        let mut oldest = None;
        store.apply(report(b"state=done:id=new")?, |record| {
            removals += 1;
            oldest = record.id().map(|id| id.as_str().to_owned());
        })?;
        assert_eq!(removals, 1);
        assert_eq!(store.len(), capacity);
        assert_eq!(oldest.as_deref(), (capacity > 1).then_some("record1"));
        assert_eq!(store.get(None).is_some(), capacity > 1);
    }
    Ok(())
}

#[test]
fn app_inheritance_tracks_changes_and_missing_ancestors() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 4];
    let mut store = Store::from_slots(&mut slots)?;
    let child = Some(RecordId::new("a/b/c")?);
    apply(&mut store, b"state=done:id=a/b/c")?;
    assert_eq!(store.effective_app(child), None);
    apply(&mut store, b"state=done:app=root")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("root")?));
    apply(&mut store, b"state=done:id=a:app=grandparent")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("grandparent")?));
    apply(&mut store, b"state=working:id=a/b:app=parent")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("parent")?));
    apply(&mut store, b"state=working:id=a/b")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("grandparent")?));
    apply(&mut store, b"state=done:id=a:app=changed")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("changed")?));
    apply(&mut store, b"state=done:id=a/b/c:app=child")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("child")?));
    apply(&mut store, b"state=done:id=a/b/c")?;
    store.on_prompt(|_| {});
    assert_eq!(store.effective_app(child), Some(AppId::new("changed")?));
    assert_eq!(store.get(child).and_then(Record::app), None);
    apply(&mut store, b"state=working:id=a:app=temporary")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("temporary")?));
    store.on_process_exit(|_| {});
    assert_eq!(store.effective_app(child), Some(AppId::new("root")?));
    apply(&mut store, b"state=done")?;
    assert_eq!(store.effective_app(child), None);
    apply(&mut store, b"state=done:app=fresh")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("fresh")?));
    assert_eq!(store.effective_app(None), Some(AppId::new("fresh")?));
    assert_eq!(store.effective_app(Some(RecordId::new("absent")?)), None);
    Ok(())
}

#[test]
fn evicting_an_ancestor_changes_app_without_rewriting_children() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 3];
    let mut store = Store::from_slots(&mut slots)?;
    let child = Some(RecordId::new("build/test")?);
    apply(&mut store, b"state=done:app=root")?;
    apply(&mut store, b"state=done:id=build:app=cargo")?;
    apply(&mut store, b"state=done:id=build/test:msg=T0s=")?;
    apply(&mut store, b"state=done:app=new-root")?;
    assert_eq!(store.effective_app(child), Some(AppId::new("cargo")?));
    let mut removed = None;
    store.apply(report(b"state=done:id=other")?, |record| {
        removed = record.id().map(|id| id.as_str().to_owned());
    })?;
    assert_eq!(removed.as_deref(), Some("build"));
    assert_eq!(store.effective_app(child), Some(AppId::new("new-root")?));
    assert_eq!(store.get(child).and_then(Record::app), None);
    assert_eq!(store.get(child).and_then(Record::message), Some("OK"));
    Ok(())
}

#[test]
fn clear_obeys_slash_boundaries_and_absent_parents() -> TestResult {
    let mut slots: Vec<_> = (0..8).map(|_| Slot::EMPTY).collect();
    let mut store = Store::from_slots(&mut slots)?;
    for body in [
        &b"state=done"[..],
        b"state=working:id=build",
        b"state=error:id=build/test",
        b"state=done:id=builder",
        b"state=done:id=build.test",
        b"state=done:id=build2/test",
    ] {
        apply(&mut store, body)?;
    }
    let mut removed = Vec::new();
    store.apply(report(b"state=clear:id=build")?, |record| {
        removed.push((record.id().map(|id| id.as_str().to_owned()), record.state()));
    })?;
    removed.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        removed,
        [
            (Some("build".to_owned()), State::Working),
            (Some("build/test".to_owned()), State::Error),
        ]
    );
    assert_eq!(store.len(), 4);
    for id in ["builder", "build.test", "build2/test"] {
        assert!(store.get(Some(RecordId::new(id)?)).is_some());
    }
    apply(&mut store, b"state=done:id=build/test/deep")?;
    let mut count = 0;
    store.apply(report(b"state=clear:id=build")?, |_| count += 1)?;
    assert_eq!(count, 1);
    assert_eq!(store.len(), 4);
    store.apply(report(b"state=clear:id=missing")?, |_| count += 1)?;
    assert_eq!(count, 1);
    store.apply(report(b"state=clear")?, |_| count += 1)?;
    assert_eq!(count, 5);
    assert!(store.is_empty());
    assert_eq!(store.iter().count(), 0);
    Ok(())
}

#[test]
fn retries_and_reused_ids_have_no_workflow_restriction() -> TestResult {
    let mut slots = [Slot::EMPTY];
    let mut store = Store::from_slots(&mut slots)?;
    let id = Some(RecordId::new("build")?);
    apply(&mut store, b"state=error:id=build:msg=RmFpbGVk")?;
    apply(&mut store, b"state=working:id=build:progress=0")?;
    let record = store.get(id).ok_or("missing retry")?;
    assert_eq!(record.state(), State::Working);
    assert_eq!(record.progress().map(parser::Progress::get), Some(0));
    assert_eq!(record.message(), None);
    apply(&mut store, b"state=clear:id=build")?;
    apply(&mut store, b"state=done:id=build:msg=T0s=")?;
    assert_eq!(store.get(id).map(Record::state), Some(State::Done));
    assert_eq!(store.get(id).and_then(Record::message), Some("OK"));
    assert_eq!(store.len(), 1);
    Ok(())
}

#[test]
fn reads_replacements_and_clears_preserve_eviction_order() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 3];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:app=root")?;
    apply(&mut store, b"state=done:id=a:msg=T0s=")?;
    apply(&mut store, b"state=error:id=b")?;
    assert!(store.get(None).is_some());
    assert_eq!(store.effective_app(None), Some(AppId::new("root")?));
    assert_eq!(store.iter().count(), 3);
    let mut removed = Vec::new();
    store.apply(report(b"state=working:id=c")?, |record| {
        removed.push(record.id().map(|id| id.as_str().to_owned()));
    })?;
    assert_eq!(removed, [None]);
    store.apply(report(b"state=working:id=b")?, |_| {
        removed.push(Some("unexpected replacement removal".to_owned()));
    })?;
    assert_eq!(removed, [None]);
    store.apply(report(b"state=working:id=d")?, |record| {
        assert_eq!(record.message(), Some("OK"));
        removed.push(record.id().map(|id| id.as_str().to_owned()));
    })?;
    assert_eq!(removed, [None, Some("a".to_owned())]);
    apply(&mut store, b"state=clear:id=c")?;
    apply(&mut store, b"state=done:id=e")?;
    store.apply(report(b"state=done:id=f")?, |record| {
        removed.push(record.id().map(|id| id.as_str().to_owned()));
    })?;
    assert_eq!(removed, [None, Some("a".to_owned()), Some("b".to_owned())]);
    assert_eq!(store.len(), 3);
    Ok(())
}

#[test]
fn repeated_updates_cannot_roll_over_eviction_order() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 3];
    let mut store = Store::from_slots(&mut slots)?;
    for id in ["a", "b", "c"] {
        apply(&mut store, format!("state=done:id={id}").as_bytes())?;
    }
    let a = report(b"state=working:id=a")?;
    let b = report(b"state=working:id=b")?;
    let mut removals = 0;
    for _ in 0..70_000 {
        store.apply(a, |_| removals += 1)?;
        store.apply(b, |_| removals += 1)?;
    }
    assert_eq!(removals, 0);
    let mut oldest = None;
    store.apply(report(b"state=done:id=d")?, |record| {
        oldest = record.id().map(|id| id.as_str().to_owned());
    })?;
    assert_eq!(oldest.as_deref(), Some("c"));
    assert_eq!(store.len(), 3);
    Ok(())
}

fn all_states(store: &mut Store<'_>) -> Result<(), Error> {
    for body in [
        &b"state=idle:app=root"[..],
        b"state=working:id=working",
        b"state=blocked:id=blocked:kind=auth",
        b"state=done:id=done",
        b"state=error:id=error",
        b"state=done:id=working/result",
    ] {
        apply(store, body)?;
    }
    Ok(())
}

#[test]
fn prompt_and_exit_remove_only_working_and_blocked() -> TestResult {
    for process_exit in [false, true] {
        let mut slots = [const { Slot::EMPTY }; 6];
        let mut store = Store::from_slots(&mut slots)?;
        all_states(&mut store)?;
        let mut removed = Vec::new();
        let on_remove = |record: osc7501_store::Record<'_>| removed.push(record.state());
        if process_exit {
            store.on_process_exit(on_remove);
        } else {
            store.on_prompt(on_remove);
        }
        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&State::Working));
        assert!(removed.contains(&State::Blocked));
        assert_eq!(store.len(), 4);
        assert_eq!(store.get(None).map(Record::state), Some(State::Idle));
        for id in ["done", "error", "working/result"] {
            assert!(store.get(Some(RecordId::new(id)?)).is_some());
        }
        assert_eq!(
            store.effective_app(Some(RecordId::new("working/result")?)),
            Some(AppId::new("root")?)
        );
    }
    Ok(())
}

#[test]
fn full_reset_clears_all_states_and_calls_removal_callback() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 6];
    let mut store = Store::from_slots(&mut slots)?;
    all_states(&mut store)?;
    assert_eq!(store.len(), 6);
    let states: Vec<_> = store.iter().map(Record::state).collect();
    for state in [
        State::Idle,
        State::Working,
        State::Blocked,
        State::Done,
        State::Error,
    ] {
        assert!(states.contains(&state));
    }
    let mut removed = Vec::new();
    store.on_full_reset(|record| removed.push(record.state()));
    assert_eq!(removed, states);
    assert!(store.is_empty());
    all_states(&mut store)?;
    assert_eq!(store.len(), 6);
    Ok(())
}

#[test]
fn ignored_and_rejected_frames_do_not_mutate_or_age_full_store() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 2];
    let mut store = Store::from_slots(&mut slots)?;
    apply(&mut store, b"state=done:id=first:msg=T0s=")?;
    apply(&mut store, b"state=error:id=second")?;
    let input = b"ordinary output\x1b]7501;state=working:id=bad!\x07\x1b]7501;state=future:id=first\x07\x1b]7501;id=first\x07\x1b]7501;state=clear:id=first:msg=A\x07\x1b]7501;state=working:id=first:msg=AA==\x07\x1b]7501;state=done:id=third:msg=/w==\x07\x1b]7501;?\x07";
    let mut decoder = Decoder::new();
    let mut accepted = 0;
    let mut rejected = 0;
    let mut removals = 0;
    let mut result = Ok(());
    for chunk in input.chunks(3) {
        decoder.feed(chunk, |event| match event {
            Ok(Event::Report(report)) => {
                accepted += 1;
                if let Err(error) = store.apply(report, |_| removals += 1) {
                    result = Err(error);
                }
            }
            Err(_) => rejected += 1,
            _ => {}
        });
    }
    result?;
    assert_eq!(accepted, 0);
    assert_eq!(rejected, 3);
    assert_eq!(removals, 0);
    assert_eq!(store.len(), 2);
    let first = store
        .get(Some(RecordId::new("first")?))
        .ok_or("missing first")?;
    assert_eq!(first.state(), State::Done);
    assert_eq!(first.message(), Some("OK"));
    assert_eq!(
        store.get(Some(RecordId::new("second")?)).map(Record::state),
        Some(State::Error)
    );
    let mut oldest = None;
    store.apply(report(b"state=done:id=third")?, |record| {
        oldest = record.id().map(|id| id.as_str().to_owned());
    })?;
    assert_eq!(oldest.as_deref(), Some("first"));
    Ok(())
}

#[test]
fn reusing_caller_storage_starts_empty() -> TestResult {
    let mut slots = [const { Slot::EMPTY }; 2];
    {
        let mut store = Store::from_slots(&mut slots)?;
        apply(&mut store, b"state=done")?;
        apply(&mut store, b"state=done:id=build")?;
    }
    let mut store = Store::from_slots(&mut slots)?;
    assert!(store.is_empty());
    apply(&mut store, b"state=error:id=test")?;
    assert_eq!(store.len(), 1);
    assert!(store.get(None).is_none());
    Ok(())
}
