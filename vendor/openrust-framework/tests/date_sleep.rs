use openrust::{
    support::{
        DateError, DateFactory,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
#[tokio::test]
async fn frozen_sleep_matches_original_rounding_dst_and_keeps_original_value() {
    let rows: Value =
        serde_json::from_str(include_str!("fixtures/date/sleep-contract.json")).unwrap();
    let factory = DateFactory::default();
    for row in rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("seconds").is_some())
    {
        let original = factory
            .parse(
                "2024-03-10 01:59:59.123456",
                Some(openrust::support::Date::timezone("America/New_York").unwrap()),
            )
            .unwrap();
        factory.set_test_now(Some(original.clone()));
        let result = factory.sleep(row["seconds"].as_f64().unwrap()).await;
        if row["result"].get("error").is_some() {
            assert!(result.is_err(), "{row}");
            continue;
        }
        result.unwrap();
        let actual = factory.get_test_now().unwrap();
        assert_eq!(
            actual.format("Y-m-d H:i:s.uP").unwrap(),
            row["result"]["date"],
            "{row}"
        );
        assert_eq!(actual.timezone_name(), row["result"]["zone"]);
        assert_eq!(
            original.format("Y-m-d H:i:s.uP").unwrap(),
            row["result"]["original"]
        );
    }
}
#[tokio::test]
async fn facade_sleep_shares_application_clock_and_keeps_scopes_and_apps_isolated() {
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"UTC"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    let original = DateFactory::default()
        .parse("2024-01-01 00:00:00", None)
        .unwrap();
    app.travel_to(original.clone());
    App::set_facade_application(Some(app.app()), async {
        Date::sleep(1.0).await.unwrap();
        assert_eq!(
            Date::now(None).unwrap().format("H:i:s").unwrap(),
            "00:00:01"
        );
        Date::with_test_now(Some(original.clone()), async {
            let (left, right) = tokio::join!(
                Date::with_test_now(Some(original.clone()), async {
                    Date::sleep(2.0).await.unwrap();
                    Date::now(None).unwrap()
                }),
                Date::with_test_now(Some(original.clone()), async {
                    Date::sleep(3.0).await.unwrap();
                    Date::now(None).unwrap()
                })
            );
            assert_eq!(left.unwrap().format("H:i:s").unwrap(), "00:00:02");
            assert_eq!(right.unwrap().format("H:i:s").unwrap(), "00:00:03");
        })
        .await
        .unwrap();
        assert!(!Date::has_test_now().unwrap());
        assert_eq!(Date::sleep(f64::NAN).await, Err(DateError::OutOfRange));
    })
    .await;
    assert!(!DateFactory::default().has_test_now());
}
#[tokio::test]
async fn live_sleep_waits_asynchronously_only_for_nonnegative_microsecond_precision() {
    let factory = DateFactory::default();
    for seconds in [-1.0, -0.1, 0.0, 0.0000009] {
        factory.sleep(seconds).await.unwrap();
    }
    let start = tokio::time::Instant::now();
    factory.sleep(0.002).await.unwrap();
    assert!(start.elapsed() >= std::time::Duration::from_millis(1));
    assert!(!factory.has_test_now());
    assert_eq!(
        factory.sleep(f64::INFINITY).await,
        Err(DateError::OutOfRange)
    );
}
