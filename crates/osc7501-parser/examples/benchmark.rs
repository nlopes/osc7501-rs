//! Reproducible local throughput and allocation measurements, without a harness.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use osc7501_parser::{Decoder, MAX_MESSAGE, Report, parse_body, parse_sequence};
use std::{hint::black_box, mem::size_of, time::Instant};

fn main() {
    let short = "state=working:app=cargo:progress=42".to_owned();
    let text = format!(
        "state=blocked:id=build/test:kind=permission:app=cargo:msg={}",
        STANDARD.encode("Approve the deployment?")
    );
    let maximum = format!(
        "state=blocked:id=build/test:kind=auth:title={}:msg={}",
        STANDARD.encode("t".repeat(192)),
        STANDARD.encode("m".repeat(2048))
    );
    for (name, body) in [
        ("short", short.as_str()),
        ("text", text.as_str()),
        ("maximum-text", maximum.as_str()),
    ] {
        let frame = format!("\x1b]7501;{body}\x1b\\");
        measure(name, "body", body.len(), || {
            black_box(parse_body(black_box(body.as_bytes())).unwrap());
        });
        measure(name, "frame", frame.len(), || {
            black_box(parse_sequence(black_box(frame.as_bytes())).unwrap());
        });
        let mut decoder = Decoder::new();
        measure(name, "stream-64", frame.len(), || {
            for chunk in black_box(frame.as_bytes()).chunks(64) {
                decoder.feed(chunk, |event| {
                    black_box(event.unwrap());
                });
            }
        });
    }
    let frame = format!("\x1b]7501;{maximum}\x1b\\");
    let mut decoder = Decoder::new();
    let mut output = [0; MAX_MESSAGE];
    let allocations = allocation_counter::measure(|| {
        for _ in 0..1000 {
            let Some(osc7501_parser::Event::Report(report)) =
                parse_body(black_box(maximum.as_bytes())).unwrap()
            else {
                panic!("expected report");
            };
            black_box(report.message.unwrap().decode_into(&mut output).unwrap());
            decoder.feed(black_box(frame.as_bytes()), |event| {
                black_box(event.unwrap());
            });
        }
    });
    println!(
        "heap allocations={}, peak heap bytes={}, decoder bytes={}, report bytes={}",
        allocations.count_total,
        allocations.bytes_max,
        size_of::<Decoder>(),
        size_of::<Report<'_>>()
    );
}

fn measure(name: &str, path: &str, bytes: usize, mut run: impl FnMut()) {
    const ITERATIONS: u32 = 100_000;
    for _ in 0..1000 {
        run();
    }
    let mut seconds = [0.0_f64; 5];
    for sample in &mut seconds {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            run();
        }
        *sample = start.elapsed().as_secs_f64();
    }
    seconds.sort_by(f64::total_cmp);
    let elapsed = seconds[2];
    // Counts are small and exactly representable in f64 on supported targets.
    let bytes = u32::try_from(bytes).expect("protocol frame fits u32");
    println!(
        "{name:12} {path:9} median={:.1} ns/report, {:.1} MiB/s",
        elapsed * 1e9 / f64::from(ITERATIONS),
        f64::from(bytes) * f64::from(ITERATIONS) / elapsed / (1024.0 * 1024.0)
    );
}
