use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use openrust::{
    config::ConfigError,
    container::ServiceKey,
    contracts::{
        container::Container as _,
        events::{Dispatcher as _, Event as Payload, Listener, ShouldDispatchAfterCommit},
        foundation::Application as _,
    },
    events::{
        Delivery, DispatchResult, Dispatcher, EventContext, EventFuture, EventResult,
        ListenerResponse, NamedEvent,
    },
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Event, Facade, Route},
    testing::TestApp,
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

trait Greeting: Send + Sync {
    fn message(&self) -> &'static str;
}

struct Message(&'static str);

impl Greeting for Message {
    fn message(&self) -> &'static str {
        self.0
    }
}

struct GreetingFacade;

impl Facade for GreetingFacade {
    type Service = dyn Greeting;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<dyn Greeting>()
    }
}

struct Count(Arc<AtomicUsize>);

impl Listener for Count {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Payload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        })
    }
}

fn greet(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let greeting = GreetingFacade::getFacadeRoot().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Event::dispatch(NamedEvent::new("greeting.sent", 42_u64)).await?;
        Event::dispatch(NamedEvent::new("greeting.done", ())).await?;
        Response::json(StatusCode::OK, greeting.message())
    })
}

fn fake_inside_http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let fake = Event::fake(&["greeting.done"])?;
        assert!(
            Event::until(NamedEvent::new("greeting.done", ()))
                .await?
                .is_none()
        );
        fake.assertDispatched("greeting.done");
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

#[tokio::test]
async fn test_app_replaces_a_service_and_fakes_events_in_one_application() -> TestResult {
    let fixture = || {
        TestApp::create(|builder| {
            builder.withRouting(Routing::default().web(|| {
                Route::get("/greeting", greet)?;
                Route::get("/fake", fake_inside_http)
            }))
        })
    };
    let mut first = fixture()?;
    let mut second = fixture()?;
    first
        .instance::<dyn Greeting>(Arc::new(Message("original")))
        .await?;
    first
        .get("/greeting")
        .await?
        .assertOk()
        .assertContent(r#""original""#);
    first
        .instance::<dyn Greeting>(Arc::new(Message("fake")))
        .await?;
    second
        .instance::<dyn Greeting>(Arc::new(Message("original")))
        .await?;

    let first_count = Arc::new(AtomicUsize::new(0));
    let second_count = Arc::new(AtomicUsize::new(0));
    for (app, count) in [(&first, &first_count), (&second, &second_count)] {
        let dispatcher = app.app().make::<Dispatcher>().await?;
        dispatcher.listen("greeting.*", Arc::new(Count(count.clone())), Delivery::Sync)?;
    }

    let fake = first.fakeEvents(&["greeting.sent"]).await?;
    fake.assertNothingDispatched();
    let (first_response, second_response) =
        tokio::join!(first.get("/greeting"), second.get("/greeting"));
    first_response?.assertOk().assertContent(r#""fake""#);
    second_response?.assertOk().assertContent(r#""original""#);

    assert_eq!(
        first.app().container().make::<dyn Greeting>()?.message(),
        "fake"
    );
    assert_eq!(
        second.app().container().make::<dyn Greeting>()?.message(),
        "original"
    );
    assert_eq!(first_count.load(Ordering::Relaxed), 1);
    assert_eq!(second_count.load(Ordering::Relaxed), 2);
    fake.assertDispatched("greeting.sent")
        .assertDispatchedTimes("greeting.sent", 1)
        .assertNotDispatched("greeting.done");
    assert_eq!(
        fake.dispatched("greeting.sent")[0]
            .downcast_ref::<NamedEvent<u64>>()
            .map(NamedEvent::payload),
        Some(&42)
    );

    let all = first.fakeEvents(&[]).await?;
    all.except(&["greeting.sent"]);
    first.get("/greeting").await?.assertOk();
    all.assertDispatchedTimes("greeting.done", 1)
        .assertNotDispatched("greeting.sent");
    fake.assertDispatchedTimes("greeting.sent", 1);
    assert_eq!(first_count.load(Ordering::Relaxed), 2);
    first
        .get("/fake")
        .await?
        .assertStatus(StatusCode::NO_CONTENT);
    assert_eq!(first_count.load(Ordering::Relaxed), 2);
    Ok(())
}

struct Saved;
impl Payload for Saved {
    fn name(&self) -> &str {
        "greeting.saved"
    }
}
impl ShouldDispatchAfterCommit for Saved {}

#[tokio::test]
async fn event_fake_records_committed_dispatches_and_discards_rolled_back_dispatches() -> TestResult
{
    verify_commit(TestApp::sqlite(|builder| builder)?).await
}

#[tokio::test]
async fn event_fake_respects_mysql_commit_and_rollback() -> TestResult {
    if std::env::var_os("OPENRUST_TEST_ELOQUENT_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
        return Ok(());
    }
    let app = TestApp::create(|builder| {
        builder.withConfig(|_, env| {
            let url = env
                .get_utf8("OPENRUST_TEST_ELOQUENT_MYSQL_URL")?
                .ok_or_else(|| ConfigError::InvalidType {
                    key: "OPENRUST_TEST_ELOQUENT_MYSQL_URL".into(),
                    expected: "a MySQL URL",
                })?;
            Ok([(
                "database".into(),
                serde_json::json!({
                    "default": "mysql",
                    "connections": { "mysql": { "url": url } }
                }),
            )]
            .into())
        })
    })?;
    verify_commit(app).await
}

async fn verify_commit(mut app: TestApp) -> TestResult {
    let connection = app.database().await?;
    let dispatcher = app.app().make::<Dispatcher>().await?;
    dispatcher.markAfterCommit::<Saved>();
    let count = Arc::new(AtomicUsize::new(0));
    dispatcher.listen(
        "greeting.saved",
        Arc::new(Count(count.clone())),
        Delivery::Sync,
    )?;
    let fake = app.fakeEvents(&[]).await?;

    let mut tx = connection.beginTransaction().await?;
    let result = dispatcher
        .dispatch(Arc::new(Saved), &mut EventContext::in_transaction(&mut tx))
        .await?;
    assert!(matches!(result, DispatchResult::Deferred));
    fake.assertNothingDispatched();

    let replacement = app.fakeEvents(&[]).await?;
    tx.commit().await?;
    fake.assertDispatchedTimes("greeting.saved", 1);
    replacement.assertNothingDispatched();

    let mut tx = connection.beginTransaction().await?;
    assert!(matches!(
        dispatcher
            .dispatchAfterCommit(Saved, &mut EventContext::in_transaction(&mut tx))
            .await?,
        DispatchResult::Deferred
    ));
    tx.rollBack(None).await?;
    replacement.assertNothingDispatched();
    assert_eq!(count.load(Ordering::Relaxed), 0);
    Ok(())
}
