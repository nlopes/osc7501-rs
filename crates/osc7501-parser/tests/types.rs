//! Producer value validation, conversions, borrowing, and allocation guarantees.
use osc7501_parser::{AppId, Error, Progress, RecordId};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn progress_construction_and_conversion_validate_every_percentage() -> TestResult {
    const MAXIMUM: Result<Progress, Error> = Progress::new(100);
    const TOO_LARGE: Result<Progress, Error> = Progress::new(101);
    assert_eq!(MAXIMUM?.get(), 100);
    assert_eq!(TOO_LARGE, Err(Error::InvalidProgress));
    for percent in 0..=u8::MAX {
        let result = Progress::new(percent);
        assert_eq!(Progress::try_from(percent), result);
        if percent <= 100 {
            assert_eq!(result?.get(), percent);
        } else {
            assert_eq!(result, Err(Error::InvalidProgress));
        }
    }
    Ok(())
}

#[test]
fn app_construction_and_conversion_validate_borrowed_identifiers() -> TestResult {
    let maximum = "a".repeat(32);
    for input in ["cargo", "a.b_c+d-e", "ABC012", &maximum] {
        let app = AppId::new(input)?;
        let converted = AppId::try_from(input)?;
        assert_eq!(converted, app);
        assert_eq!(app.as_str(), input);
        assert_eq!(app.as_str().as_ptr(), input.as_ptr());
        assert_eq!(converted.as_str().as_ptr(), input.as_ptr());
    }
    let too_long = "a".repeat(33);
    for input in [
        "", "a/b", "a b", "a=b", "a,b", "a!", "🦀", "a\0b", &too_long,
    ] {
        assert_eq!(AppId::new(input), Err(Error::InvalidAppId), "{input:?}");
        assert_eq!(
            AppId::try_from(input),
            Err(Error::InvalidAppId),
            "{input:?}"
        );
    }
    Ok(())
}

#[test]
fn record_construction_and_conversion_validate_borrowed_identifiers() -> TestResult {
    let segment = "a".repeat(32);
    let maximum = format!("{segment}/{segment}/{segment}/{}", "a".repeat(29));
    assert_eq!(maximum.len(), 128);
    for input in [
        "build/test",
        "a.b_c+d-e/ABC012",
        "a/a/a/a/a/a/a/a",
        &segment,
        &maximum,
    ] {
        let id = RecordId::new(input)?;
        let converted = RecordId::try_from(input)?;
        assert_eq!(converted, id);
        assert_eq!(id.as_str(), input);
        assert_eq!(id.as_str().as_ptr(), input.as_ptr());
        assert_eq!(converted.as_str().as_ptr(), input.as_ptr());
    }
    let long_segment = "a".repeat(33);
    let too_long = format!("{maximum}a");
    for input in [
        "",
        "/",
        "/build",
        "build/",
        "build//test",
        "build/test!",
        "a=b",
        "a,b",
        "has space",
        "🦀",
        "a\0b",
        "a/a/a/a/a/a/a/a/a",
        &long_segment,
        &too_long,
    ] {
        assert_eq!(
            RecordId::new(input),
            Err(Error::InvalidRecordId),
            "{input:?}"
        );
        assert_eq!(
            RecordId::try_from(input),
            Err(Error::InvalidRecordId),
            "{input:?}"
        );
    }
    Ok(())
}

#[test]
fn value_construction_and_conversion_do_not_allocate() {
    let info = allocation_counter::measure(|| {
        assert!(AppId::new("cargo").is_ok());
        assert!(AppId::try_from("cargo").is_ok());
        assert_eq!(AppId::new(""), Err(Error::InvalidAppId));
        assert_eq!(AppId::try_from(""), Err(Error::InvalidAppId));
        assert!(RecordId::new("build/test").is_ok());
        assert!(RecordId::try_from("build/test").is_ok());
        assert_eq!(RecordId::new(""), Err(Error::InvalidRecordId));
        assert_eq!(RecordId::try_from(""), Err(Error::InvalidRecordId));
        assert!(Progress::new(100).is_ok());
        assert!(Progress::try_from(100).is_ok());
        assert_eq!(Progress::new(101), Err(Error::InvalidProgress));
        assert_eq!(Progress::try_from(101), Err(Error::InvalidProgress));
    });
    assert_eq!(info.count_total, 0);
    assert_eq!(info.bytes_max, 0);
}
