use openrust::{
    support::{
        Date as DateValue, DateTimezone,
        facades::{App, Config, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::json;
use std::sync::Arc;
fn value(text: &str, zone: &str) -> DateValue {
    DateValue::from_utc(text.parse().unwrap(), DateValue::timezone(zone).unwrap())
}
async fn app(zone: &str) -> TestApp {
    let mut app = TestApp::create(|builder| {
        builder.with_config(|root, _| {
            Ok([(
                "app".into(),
                serde_json::from_slice(&std::fs::read(root.join("app.json")).unwrap()).unwrap(),
            )]
            .into())
        })
    })
    .unwrap();
    std::fs::write(
        app.root().join("app.json"),
        json!({"timezone":zone}).to_string(),
    )
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn default_facade_works_without_application_and_helpers_share_its_clock() {
    let time = value("2024-04-12T03:04:05.123456Z", "Asia/Shanghai");
    Date::with_test_now(Some(time.clone()), async {
        assert!(Date::has_test_now().unwrap());
        assert_eq!(Date::get_test_now().unwrap().unwrap(), time);
        assert_eq!(
            Date::now(None).unwrap().as_date_time().timestamp(),
            time.as_date_time().timestamp()
        );
        assert_eq!(
            openrust::support::now(Some(DateTimezone::UTC))
                .unwrap()
                .as_date_time()
                .timestamp(),
            time.as_date_time().timestamp()
        );
        assert_eq!(Date::instance(&time).unwrap(), time);
    })
    .await
    .unwrap();
    assert!(!Date::has_test_now().unwrap());
}
#[tokio::test]
async fn facade_factory_and_test_app_share_one_clock_with_the_original_timezone() {
    let app = app("Asia/Shanghai").await;
    let time = value("2024-03-10T06:30:00Z", "America/New_York");
    app.travel_to(time.clone());
    App::set_facade_application(Some(app.app()), async {
        let root = Date::get_facade_root().unwrap();
        assert!(Arc::ptr_eq(&root, &Date::get_facade_root().unwrap()));
        assert_eq!(Date::get_test_now().unwrap(), Some(time.clone()));
        assert_eq!(Date::now(None).unwrap().as_date_time().hour(), 14);
        Date::set_test_now(Some(value("2024-01-01T00:00:00Z", "UTC"))).unwrap();
        assert_eq!(
            openrust::support::now(None).unwrap().as_date_time().year(),
            2024
        );
        Config::set("app.timezone", "Europe/Paris".into()).unwrap();
        assert_eq!(Date::now(None).unwrap().as_date_time().hour(), 1);
        Date::set_test_now(None).unwrap();
        assert!(!Date::has_test_now().unwrap());
        Config::set("app.timezone", false.into()).unwrap();
        assert_eq!(
            Date::now(None),
            Err(openrust::support::DateError::InvalidTimezone)
        );
        assert!(Date::now(Some(DateTimezone::UTC)).is_ok());
    })
    .await;
}
#[tokio::test]
async fn civil_midnights_follow_dst_and_explicit_zones_do_not_mutate_application_defaults() {
    let app = app("America/New_York").await;
    App::set_facade_application(Some(app.app()), async {
        Date::with_test_now(Some(value("2024-03-10T12:00:00Z", "UTC")), async {
            let today = Date::today(None).unwrap();
            let tomorrow = Date::tomorrow(None).unwrap();
            let yesterday = Date::yesterday(None).unwrap();
            assert_eq!(today.as_date_time().hour(), 0);
            assert_eq!(
                tomorrow
                    .as_date_time()
                    .timestamp()
                    .duration_since(today.as_date_time().timestamp())
                    .as_hours(),
                23
            );
            assert_eq!(yesterday.as_date_time().day(), 9);
            let shanghai = DateValue::timezone("Asia/Shanghai").unwrap();
            assert_eq!(Date::now(Some(shanghai)).unwrap().as_date_time().hour(), 20);
            assert_eq!(Date::now(None).unwrap().as_date_time().hour(), 8);
            assert_eq!(
                Date::now(Some(DateTimezone::UTC))
                    .unwrap()
                    .as_date_time()
                    .hour(),
                12
            );
        })
        .await
        .unwrap();
    })
    .await;
}
#[tokio::test]
async fn nested_freezes_clear_on_exit_after_errors_panics_and_future_cancellation() {
    use futures_util::FutureExt;
    let app = app("UTC").await;
    App::set_facade_application(Some(app.app()), async {
        let outer = value("2024-01-01T00:00:00Z", "UTC");
        Date::set_test_now(Some(outer.clone())).unwrap();
        let result = Date::with_test_now(None, async {
            assert!(!Date::has_test_now().unwrap());
            Err::<(), _>("business error")
        })
        .await
        .unwrap();
        assert_eq!(result, Err("business error"));
        assert_eq!(Date::get_test_now().unwrap(), None);
        Date::set_test_now(Some(outer.clone())).unwrap();
        let panic = std::panic::AssertUnwindSafe(Date::with_test_now(
            Some(value("2025-01-01T00:00:00Z", "UTC")),
            async { panic!("expected panic") },
        ))
        .catch_unwind()
        .await;
        assert!(panic.is_err());
        assert_eq!(Date::get_test_now().unwrap(), None);
        Date::set_test_now(Some(outer.clone())).unwrap();
        let (entered, seen) = tokio::sync::oneshot::channel();
        let mut future = Box::pin(Date::with_test_now(None, async {
            entered.send(()).unwrap();
            std::future::pending::<()>().await
        }));
        tokio::select! {r=&mut future=>panic!("unexpected {r:?}"),_ =seen=>{}}
        drop(future);
        assert_eq!(Date::get_test_now().unwrap(), None);
    })
    .await;
}
#[tokio::test]
async fn concurrent_freezes_and_applications_are_isolated() {
    let a = app("UTC").await;
    let b = app("Asia/Shanghai").await;
    let one = value("2024-01-01T00:00:00Z", "UTC");
    let two = value("2025-01-01T00:00:00Z", "UTC");
    App::set_facade_application(Some(a.app()), async {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let run = |time: DateValue| {
            let barrier = barrier.clone();
            async move {
                Date::with_test_now(Some(time.clone()), async {
                    barrier.wait().await;
                    tokio::task::yield_now().await;
                    assert_eq!(Date::get_test_now().unwrap(), Some(time));
                })
                .await
                .unwrap();
            }
        };
        tokio::join!(run(one.clone()), run(two));
        assert!(!Date::has_test_now().unwrap());
        Date::set_test_now(Some(one.clone())).unwrap();
        App::set_facade_application(Some(b.app()), async {
            assert!(!Date::has_test_now().unwrap());
        })
        .await;
        assert_eq!(Date::get_test_now().unwrap(), Some(one));
    })
    .await;
}

#[tokio::test]
async fn original_php_clock_fixture_matches_the_public_facade() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/date/clock-contract.json")).unwrap();
    let app = app("America/New_York").await;
    App::set_facade_application(Some(app.app()), async {
        Date::set_test_now(Some(value("2024-03-10T12:00:00Z", "America/New_York"))).unwrap();
        let format = |date: DateValue| {
            date.as_date_time()
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string()
        };
        assert_eq!(format(Date::today(None).unwrap()), expected["today"]);
        assert_eq!(format(Date::tomorrow(None).unwrap()), expected["tomorrow"]);
        assert_eq!(
            format(Date::yesterday(None).unwrap()),
            expected["yesterday"]
        );
        assert_eq!(
            Date::tomorrow(None)
                .unwrap()
                .as_date_time()
                .timestamp()
                .duration_since(Date::today(None).unwrap().as_date_time().timestamp())
                .as_hours(),
            expected["dst_hours"].as_i64().unwrap()
        );
        Date::with_test_now(Some(value("2025-01-01T00:00:00Z", "UTC")), async {
            Date::with_test_now(Some(value("2026-01-01T00:00:00Z", "UTC")), async {})
                .await
                .unwrap();
            assert_eq!(Date::has_test_now().unwrap(), expected["nested"]);
        })
        .await
        .unwrap();
        assert_eq!(Date::has_test_now().unwrap(), expected["after"]);
    })
    .await;
}
