//! Chunk composition and resumption after consumer failures.
use hegel::{Generator as _, PrintableGenerator, TestCase, generators as gs};
use osc7501_parser::{Decoder, Error, Event, Limit, MAX_SEQUENCE, stream::Error as StreamError};
use std::{
    convert::Infallible,
    error::Error as _,
    io,
    sync::mpsc::{TrySendError, sync_channel},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Observation = Result<String, Error>;

struct Paused;

fn observe(event: Result<Event<'_>, Error>) -> Observation {
    event.map(|event| format!("{event:?}"))
}

fn stream() -> impl PrintableGenerator<Vec<u8>> {
    let frame = hegel::one_of![
        gs::just(b"\x1b]7501;state=done:msg=T0s=\x1b\\".to_vec()),
        gs::just(b"\x1b]7501;state=blocked:id=a/b:kind=auth\x07".to_vec()),
        gs::just(b"\x1b]7501;?:states=done,future,working:kinds=\x07".to_vec()),
        gs::just(b"\x1b]7501;?\x1b\\".to_vec()),
        gs::just(b"\x1b]7501;state=future\x07".to_vec()),
        gs::just(b"\x1b]7501;state=done:msg=AA==\x07".to_vec()),
        gs::just(b"\x1b]0;other osc\x1b\\".to_vec()),
        gs::just(b"\x1b]7501;state=".to_vec()),
        gs::integers::<usize>()
            .min_value(4080)
            .max_value(4109)
            .map(|size| {
                format!("\x1b]7501;state=done:future={}\x1b\\", "x".repeat(size)).into_bytes()
            }),
        gs::binary().max_size(79),
    ];
    gs::vecs(frame).max_size(11).map(|parts| parts.concat())
}

fn trace(prefix: &[u8], bytes: &[u8], cuts: &[usize], stop: bool) -> Vec<Observation> {
    let mut decoder = Decoder::new();
    let mut events = Vec::new();
    decoder.feed(prefix, |event| events.push(observe(event)));
    let mut start = 0;
    for end in cuts.iter().copied().chain(core::iter::once(bytes.len())) {
        decoder.feed(&[], |event| events.push(observe(event)));
        let mut input = &bytes[start..end];
        loop {
            let result = decoder.try_feed(input, |event| {
                events.push(observe(event));
                if stop { Err(Paused) } else { Ok(()) }
            });
            match result {
                Ok(()) => break,
                Err(failure) => {
                    assert!((1..=input.len()).contains(&failure.consumed()));
                    input = &input[failure.consumed()..];
                }
            }
        }
        start = end;
    }
    // A common suffix makes retained partial-frame state observable.
    decoder.feed(b"done\x07\x1b]7501;state=error\x1b\\", |event| {
        events.push(observe(event));
    });
    events
}

#[hegel::test]
fn generated_streams_preserve_events_errors_and_continuation_across_partitions(tc: TestCase) {
    let prefix = tc.draw(stream());
    let bytes = tc.draw(stream());
    let cuts = tc.draw(gs::vecs(gs::integers::<u16>()).max_size(39));
    let mut cuts: Vec<_> = cuts
        .into_iter()
        .map(|cut| usize::from(cut) % (bytes.len() + 1))
        .collect();
    cuts.sort_unstable();
    let expected = trace(&prefix, &bytes, &[], false);
    assert_eq!(&trace(&prefix, &bytes, &cuts, false), &expected);
    assert_eq!(trace(&prefix, &bytes, &cuts, true), expected);
}

#[test]
fn callback_failure_consumes_only_the_delivered_event_across_every_split() -> TestResult {
    for first in [
        &b"\x1b]7501;?\x1b\\"[..],
        b"\x1b]7501;state=done\x07",
        b"\x1b]7501;state=done:msg=AA==\x1b\\",
    ] {
        let second = b"\x1b]7501;state=working\x07";
        let input = [first, second].concat();
        for split in 0..first.len() {
            let mut decoder = Decoder::new();
            let mut events = Vec::new();
            decoder.feed(&input[..split], |event| events.push(observe(event)));
            assert_eq!(events, [] as [Observation; 0]);
            let failure = decoder
                .try_feed(&input[split..], |event| {
                    events.push(observe(event));
                    Err(io::Error::other("consumer stopped"))
                })
                .expect_err("the first callback must stop feeding");
            assert_eq!(failure.consumed(), first.len() - split);
            assert_eq!(failure.get_ref().kind(), io::ErrorKind::Other);
            assert_eq!(
                failure.source().ok_or("missing source")?.to_string(),
                "consumer stopped"
            );
            assert_eq!(events.len(), 1);
            decoder.try_feed(&input[split + failure.consumed()..], |event| {
                events.push(observe(event));
                Ok::<(), Infallible>(())
            })?;
            assert_eq!(events, trace(&[], &input, &[], false)[..2]);
            assert_eq!(failure.into_inner().kind(), io::ErrorKind::Other);
        }
    }
    Ok(())
}

#[test]
fn callback_errors_can_borrow_caller_data() {
    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct Borrowed<'a>(&'a str);

    let message = String::from("consumer stopped");
    let failure = Decoder::new()
        .try_feed(b"\x1b]7501;?\x07", |_| Err(Borrowed(&message)))
        .expect_err("the callback must stop feeding");
    assert_eq!(failure.consumed(), 9);
    assert_eq!(failure.get_ref().0, message);
    assert_eq!(failure.into_inner().0, message);
}

#[test]
fn anyhow_callback_errors_propagate_without_a_new_wrapper() -> TestResult {
    let receive = || -> anyhow::Result<()> {
        Decoder::new()
            .try_feed(b"\x1b]7501;?\x07", |_| -> anyhow::Result<()> {
                Err(io::Error::other("consumer stopped").into())
            })
            .map_err(StreamError::into_inner)?;
        Ok(())
    };
    let failure = receive().expect_err("the callback error must propagate");
    assert_eq!(
        failure
            .downcast_ref::<io::Error>()
            .ok_or("lost I/O error")?
            .kind(),
        io::ErrorKind::Other
    );
    assert_eq!(failure.to_string(), "consumer stopped");
    Ok(())
}

#[test]
fn boxed_callback_errors_propagate_without_a_new_wrapper() -> TestResult {
    let receive = || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Decoder::new()
            .try_feed(
                b"\x1b]7501;?\x07",
                |_| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                    Err(io::Error::other("consumer stopped").into())
                },
            )
            .map_err(StreamError::into_inner)?;
        Ok(())
    };
    let failure = receive().expect_err("the callback error must propagate");
    assert_eq!(
        failure
            .downcast_ref::<io::Error>()
            .ok_or("lost I/O error")?
            .kind(),
        io::ErrorKind::Other
    );
    assert_eq!(failure.to_string(), "consumer stopped");
    Ok(())
}

#[test]
fn error_chain_preserves_context_without_repeating_the_source() {
    let receive = || -> anyhow::Result<()> {
        Decoder::new().try_feed(b"\x1b]7501;?\x07", |_| Err(io::Error::other("disk full")))?;
        Ok(())
    };
    let failure = receive().expect_err("the callback error must propagate");
    assert_eq!(
        format!("{failure:#}"),
        "event callback failed after 9 input bytes: disk full"
    );
    assert_eq!(failure.chain().count(), 2);
}

#[test]
fn full_queue_retains_the_delivered_event_for_retry_before_resuming() -> TestResult {
    let mut decoder = Decoder::new();
    let first = b"\x1b]7501;?\x07";
    let second = b"\x1b]7501;state=working\x07";
    let third = b"\x1b]7501;state=done\x07";
    let input = [first.as_slice(), second, third].concat();
    let expected = trace(&[], &input, &[], false);
    let (sender, receiver) = sync_channel(1);
    let enqueue = |event: Result<Event<'_>, Error>| sender.try_send(observe(event));

    let failure = decoder
        .try_feed(&input, enqueue)
        .expect_err("the second event must find the queue full");
    assert!(matches!(failure.get_ref(), TrySendError::Full(_)));
    let consumed = failure.consumed();
    assert_eq!(consumed, first.len() + second.len());
    let pending = match failure.into_inner() {
        TrySendError::Full(event) => event,
        TrySendError::Disconnected(_) => return Err("queue disconnected".into()),
    };

    assert_eq!(receiver.try_recv()?, expected[0]);
    sender.try_send(pending)?;
    assert_eq!(receiver.try_recv()?, expected[1]);
    decoder.try_feed(&input[consumed..], enqueue)?;
    assert_eq!(receiver.try_recv()?, expected[2]);
    assert!(receiver.try_recv().is_err());
    Ok(())
}

#[test]
fn stopping_at_overflow_preserves_draining_and_consumed_byte_count() -> TestResult {
    for terminator in ["\x07", "\x1b\\"] {
        for size in [MAX_SEQUENCE + 1, MAX_SEQUENCE + 2, MAX_SEQUENCE + 20] {
            let prefix = "\x1b]7501;state=done:future=";
            let frame = format!(
                "{prefix}{}{terminator}",
                "x".repeat(size - prefix.len() - terminator.len())
            );
            let input = [frame.as_bytes(), b"\x1b]7501;?\x07"].concat();
            for split in [0, 1, MAX_SEQUENCE - 1, MAX_SEQUENCE] {
                let mut decoder = Decoder::new();
                let mut events = Vec::new();
                decoder.feed(&input[..split], |event| events.push(observe(event)));
                assert_eq!(events, [] as [Observation; 0]);
                let failure = decoder
                    .try_feed(&input[split..], |event| {
                        assert_eq!(event, Err(Error::Limit(Limit::Sequence)));
                        Err(Paused)
                    })
                    .expect_err("overflow must reach the callback");
                assert_eq!(failure.consumed(), MAX_SEQUENCE + 1 - split);
                decoder.try_feed(&input[split + failure.consumed()..], |event| {
                    events.push(observe(event));
                    Ok::<(), Infallible>(())
                })?;
                assert_eq!(events, [Ok(format!("{:?}", Event::Query))]);
            }
        }
    }
    Ok(())
}

#[test]
fn empty_ignored_and_partial_input_do_not_call_the_consumer() -> TestResult {
    let mut decoder = Decoder::new();
    for input in [
        &b""[..],
        b"text\x1b]0;title\x07\x1b]7501;state=future\x07",
        b"\x1b]7501;state=",
    ] {
        decoder.try_feed(input, |_| Err(io::Error::other("unexpected callback")))?;
    }
    decoder.reset();
    decoder.try_feed(b"done\x07", |_| {
        Err(io::Error::other("unexpected callback"))
    })?;
    Ok(())
}

#[test]
fn stopping_and_resuming_do_not_allocate() -> TestResult {
    let mut decoder = Decoder::new();
    let input = b"\x1b]7501;?\x07\x1b]7501;state=done\x07";
    let mut result = Ok(());
    let mut events = 0;
    let measured = allocation_counter::measure(|| {
        if let Err(failure) = decoder.try_feed(input, |_| {
            events += 1;
            Err(Paused)
        }) {
            result = decoder.try_feed(&input[failure.consumed()..], |_| {
                events += 1;
                Ok::<(), Infallible>(())
            });
        }
    });
    result?;
    assert_eq!(events, 2);
    assert_eq!(measured.count_total, 0);
    Ok(())
}
