use openrust::support::{Retry, Sleep, Str, blank, filled};
use serde_json::json;
use std::{num::NonZeroUsize, time::Duration};

#[test]
fn blank_matches_json_scalar_and_container_boundaries() {
    for item in [json!(null), json!(" \t\0"), json!([]), json!({})] {
        assert!(blank(&item));
        assert!(!filled(&item));
    }
    for item in [
        json!(false),
        json!(0),
        json!("0"),
        json!(" "),
        json!([null]),
    ] {
        assert!(filled(&item));
    }
    assert_eq!(
        Str::of("abc").hash::<md5::Md5>().to_string(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
}

#[tokio::test]
async fn retry_uses_one_based_attempts_stops_on_success_and_returns_terminal_error() {
    let fake = Sleep::fake();
    let delays = [Duration::from_millis(2), Duration::from_millis(7)];
    let value = Retry::backoff(delays)
        .using(&fake)
        .run(|attempt| async move {
            if attempt == 3 {
                Ok(attempt)
            } else {
                Err(attempt)
            }
        })
        .await
        .unwrap();
    assert_eq!(value, 3);
    fake.assert_sequence(&delays.map(Some));

    let fake = Sleep::fake();
    let result = Retry::times(NonZeroUsize::new(3).unwrap(), delays[0])
        .using(&fake)
        .run_when(
            |attempt| async move { Err::<(), _>(attempt) },
            |attempt| *attempt < 2,
        )
        .await;
    assert_eq!(result, Err(2));
    fake.assert_sequence(&[Some(delays[0])]);

    let result = Retry::backoff([])
        .run(|attempt| async move { Err::<(), _>(attempt) })
        .await;
    assert_eq!(result, Err(1));
}
