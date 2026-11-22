use openrust::http::HttpError;
use std::error::Error as _;

#[test]
fn public_result_question_mark_keeps_extension_error() {
    fn propagate(result: openrust_pennant::Result<u32>) -> Result<u32, HttpError> {
        Ok(result?)
    }
    assert_eq!(propagate(Ok(7)).unwrap(), 7);
    let error = propagate(Err(openrust_pennant::Error::InvalidValue)).unwrap_err();
    assert!(matches!(error, HttpError::Internal { .. }));
    assert!(error.source().unwrap().is::<openrust_pennant::Error>());
}

#[test]
fn database_source_stays_in_extension_domain() {
    let error = HttpError::from(openrust_pennant::Error::Database(
        openrust::database::DatabaseError::InvalidQuery("private-query"),
    ));
    assert!(error.source().unwrap().is::<openrust_pennant::Error>());
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<openrust::database::DatabaseError>()
    );
}

fn unavailable(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, HttpError>> {
    Box::pin(async {
        openrust_pennant::Feature::manager()?;
        Ok(openrust::http::Response::from("resolved"))
    })
}

#[tokio::test]
async fn public_consumer_reports_domain_and_returns_redacted_server_error() {
    use openrust::{foundation::configuration::Routing, support::facades::Route, testing::TestApp};
    let reported = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let counter = reported.clone();
    let mut app = TestApp::create(move |builder| {
        builder
            .withConfig(|_, _| Ok([("app".into(), serde_json::json!({"debug":false}))].into()))
            .withExceptions(move |exceptions| {
                exceptions.report::<openrust_pennant::Error>(move |_| {
                    counter.store(true, std::sync::atomic::Ordering::SeqCst);
                    openrust::foundation::configuration::ReportDecision::Stop
                });
            })
            .withRouting(Routing::default().web(|| Route::get("/failure", unavailable)))
    })
    .unwrap();
    let response = app.get("/failure").await.unwrap();
    response
        .assertStatus(openrust::http::StatusCode::INTERNAL_SERVER_ERROR)
        .assertDontSee("unavailable", false);
    assert!(reported.load(std::sync::atomic::Ordering::SeqCst));
}
