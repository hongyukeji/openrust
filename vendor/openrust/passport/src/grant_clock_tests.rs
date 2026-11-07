use super::{GrantClock, SystemGrantClock, clock_unavailable};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn system_clock_returns_current_unix_seconds() -> openrust::Result<()> {
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(openrust::Error::internal)?
        .as_secs();
    let actual = SystemGrantClock.now()?;
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(openrust::Error::internal)?
        .as_secs();
    let actual = u64::try_from(actual).map_err(openrust::Error::internal)?;
    assert!(actual >= before && actual <= after);
    Ok(())
}

#[test]
fn clock_error_is_stable_and_redacted() {
    let error = clock_unavailable();
    assert_eq!(error.code(), "passport_clock_unavailable");
    assert!(!error.to_string().contains("SystemTime"));
    assert!(!error.to_string().contains("UNIX_EPOCH"));
}
