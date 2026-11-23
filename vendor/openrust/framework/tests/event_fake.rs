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
    database::events::{
        TransactionBeginning, TransactionCommitted, TransactionCommitting, TransactionRolledBack,
    },
    events::{
        Delivery, DispatchResult, Dispatcher, EventContext, EventError, EventFuture, EventResult,
        ListenerResponse, NamedEvent,
    },
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Db, Event, Facade, Route},
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

    fn get_facade_accessor() -> ServiceKey {
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
        let greeting = GreetingFacade::get_facade_root()?;
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
        fake.assert_dispatched("greeting.done");
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

#[tokio::test]
async fn test_app_replaces_a_service_and_fakes_events_in_one_application() -> TestResult {
    let fixture = || {
        TestApp::create(|builder| {
            builder.with_routing(Routing::default().web(|| {
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
        .assert_ok()
        .assert_content(r#""original""#);
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

    let fake = first.fake_events(&["greeting.sent"]).await?;
    fake.assert_nothing_dispatched();
    let (first_response, second_response) =
        tokio::join!(first.get("/greeting"), second.get("/greeting"));
    first_response?.assert_ok().assert_content(r#""fake""#);
    second_response?.assert_ok().assert_content(r#""original""#);

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
    fake.assert_dispatched("greeting.sent")
        .assert_dispatched_times("greeting.sent", 1)
        .assert_not_dispatched("greeting.done");
    assert_eq!(
        fake.dispatched("greeting.sent")[0]
            .downcast_ref::<NamedEvent<u64>>()
            .map(NamedEvent::payload),
        Some(&42)
    );

    let all = first.fake_events(&[]).await?;
    all.except(&["greeting.sent"]);
    first.get("/greeting").await?.assert_ok();
    all.assert_dispatched_times("greeting.done", 1)
        .assert_not_dispatched("greeting.sent");
    fake.assert_dispatched_times("greeting.sent", 1);
    assert_eq!(first_count.load(Ordering::Relaxed), 2);
    first
        .get("/fake")
        .await?
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(first_count.load(Ordering::Relaxed), 2);
    Ok(())
}

fn fake_for_request(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let global = Event::fake_except(&["scope.selected", "scope.replaced"])?;
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, resumed) = tokio::sync::oneshot::channel();
        let scoped = Event::fake_for(
            async {
                entered.send(()).unwrap();
                resumed.await.unwrap();
                Event::dispatch(NamedEvent::new("scope.selected", ())).await?;
                Event::dispatch(NamedEvent::new("scope.base", ())).await?;

                let nested = Event::fake_except_for(
                    async {
                        Event::dispatch(NamedEvent::new("scope.selected", ())).await?;
                        Event::dispatch(NamedEvent::new("scope.base", ())).await?;
                        Err::<(), _>(EventError::InvalidPayload)
                    },
                    &["scope.selected"],
                )
                .await;
                assert!(matches!(nested, Err(EventError::InvalidPayload)));
                Event::dispatch(NamedEvent::new("scope.selected", ())).await?;

                let local = Event::fake(&["scope.replaced"])?;
                Event::dispatch(NamedEvent::new("scope.replaced", ())).await?;
                local.assert_dispatched_times("scope.replaced", 1);
                Ok(42)
            },
            &["scope.selected"],
        );
        let sibling = async {
            ready.await.unwrap();
            let dispatched = Event::dispatch(NamedEvent::new("scope.selected", ())).await;
            release.send(()).unwrap();
            dispatched
        };
        let (value, dispatched) = tokio::join!(scoped, sibling);
        dispatched?;
        assert_eq!(value?, 42);

        let timeout = tokio::time::timeout(
            std::time::Duration::ZERO,
            Event::fake_for(std::future::pending::<EventResult<()>>(), &[]),
        )
        .await;
        assert!(timeout.is_err());
        Event::dispatch(NamedEvent::new("scope.selected", ())).await?;
        Event::dispatch(NamedEvent::new("scope.replaced", ())).await?;
        Event::dispatch(NamedEvent::new("scope.base", ())).await?;
        global
            .assert_dispatched_times("scope.base", 1)
            .assert_not_dispatched("scope.selected")
            .assert_not_dispatched("scope.replaced");
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

#[tokio::test]
async fn scoped_event_fakes_restore_after_errors_and_cancellation() -> TestResult {
    let mut app = TestApp::create(|builder| {
        builder.with_routing(Routing::default().web(|| Route::get("/scoped", fake_for_request)))
    })?;
    app.boot().await?;
    let dispatcher = app.app().make::<Dispatcher>().await?;
    let selected = Arc::new(AtomicUsize::new(0));
    let base = Arc::new(AtomicUsize::new(0));
    let replaced = Arc::new(AtomicUsize::new(0));
    for (name, count) in [
        ("scope.selected", &selected),
        ("scope.base", &base),
        ("scope.replaced", &replaced),
    ] {
        dispatcher.listen(name, Arc::new(Count(count.clone())), Delivery::Sync)?;
    }
    app.get("/scoped")
        .await?
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(selected.load(Ordering::Relaxed), 3);
    assert_eq!(base.load(Ordering::Relaxed), 1);
    assert_eq!(replaced.load(Ordering::Relaxed), 1);
    Ok(())
}

fn numeric_payload(event: &dyn Payload) -> Option<u64> {
    event
        .downcast_ref::<NamedEvent<u64>>()
        .map(|event| *event.payload())
}

fn fake_matching_events(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let fake = Event::fake_when(&["filter.exact", "filter.excluded"], |event| {
            match event.name() {
                "filter.exact" | "filter.excluded" => {
                    panic!("exact and excluded names are resolved before the predicate")
                }
                "filter.numeric" => numeric_payload(event).is_some_and(|value| value >= 2),
                "greeting.saved" => true,
                name => name == TransactionBeginning::NAME,
            }
        })?;
        fake.except(&["filter.excluded"]).except_when(|event| {
            event.name() == "filter.numeric" && numeric_payload(event) == Some(3)
        });

        for value in 1_u64..=4 {
            Event::dispatch(NamedEvent::new("filter.numeric", value)).await?;
        }
        Event::dispatch(NamedEvent::new("filter.numeric", "not a number")).await?;
        Event::dispatch(NamedEvent::new("filter.exact", ())).await?;
        Event::dispatch(NamedEvent::new("filter.excluded", ())).await?;

        let mut transaction = Db::begin_transaction()
            .await
            .map_err(EventError::Transaction)?;
        let dispatch = Event::dispatch_after_commit(Saved, &mut transaction).await?;
        assert!(matches!(dispatch, DispatchResult::Deferred));
        transaction
            .roll_back(None)
            .await
            .map_err(EventError::Transaction)?;
        fake.assert_dispatched_once(TransactionBeginning::NAME)
            .assert_not_dispatched(TransactionRolledBack::NAME)
            .assert_not_dispatched("greeting.saved")
            .assert_dispatched_once("filter.exact")
            .assert_not_dispatched("filter.excluded");
        let mut transaction = Db::begin_transaction()
            .await
            .map_err(EventError::Transaction)?;
        Event::dispatch_after_commit(Saved, &mut transaction).await?;
        fake.assert_not_dispatched("greeting.saved");
        transaction
            .commit()
            .await
            .map_err(EventError::Transaction)?;
        fake.assert_dispatched_once("greeting.saved");

        Event::fake_for_when(
            async {
                Event::dispatch(NamedEvent::new("filter.numeric", 1_u64)).await?;
                Event::dispatch(NamedEvent::new("filter.numeric", 2_u64)).await?;
                Ok(())
            },
            &[],
            |event| numeric_payload(event) == Some(1),
        )
        .await?;
        Event::dispatch(NamedEvent::new("filter.numeric", 2_u64)).await?;

        fake.assert_dispatched_where("filter.numeric", |event| {
            fake.assert_dispatched_times("filter.numeric", 3);
            numeric_payload(event) == Some(4)
        })
        .assert_not_dispatched_where("filter.numeric", |event| {
            matches!(numeric_payload(event), Some(1 | 3))
        });
        let recorded: Vec<_> = fake
            .dispatched_where("filter.numeric", |event| numeric_payload(event).is_some())
            .iter()
            .filter_map(|event| numeric_payload(event.as_ref()))
            .collect();
        assert_eq!(recorded, [2, 4, 2]);
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

#[tokio::test]
async fn predicates_select_payloads_without_hiding_other_events() -> TestResult {
    let mut app = TestApp::sqlite(|builder| {
        builder.with_routing(Routing::default().web(|| Route::get("/filter", fake_matching_events)))
    })?;
    app.boot().await?;
    let dispatcher = app.app().make::<Dispatcher>().await?;
    let observed = Arc::new(AtomicUsize::new(0));
    dispatcher.listen(
        "filter.*",
        Arc::new(Count(observed.clone())),
        Delivery::Sync,
    )?;
    app.get("/filter")
        .await?
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(observed.load(Ordering::Relaxed), 5);
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
        builder.with_config(|_, env| {
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
    dispatcher.mark_after_commit::<Saved>();
    let count = Arc::new(AtomicUsize::new(0));
    dispatcher.listen(
        "greeting.saved",
        Arc::new(Count(count.clone())),
        Delivery::Sync,
    )?;
    let fake = app.fake_events(&[]).await?;

    let mut tx = connection.begin_transaction().await?;
    let result = dispatcher
        .dispatch(Arc::new(Saved), &mut EventContext::in_transaction(&mut tx))
        .await?;
    assert!(matches!(result, DispatchResult::Deferred));
    fake.assert_dispatched_times(TransactionBeginning::NAME, 1)
        .assert_not_dispatched("greeting.saved");

    let replacement = app.fake_events(&[]).await?;
    tx.commit().await?;
    fake.assert_dispatched_times("greeting.saved", 1);
    replacement
        .assert_dispatched_times(TransactionCommitting::NAME, 1)
        .assert_dispatched_times(TransactionCommitted::NAME, 1)
        .assert_not_dispatched("greeting.saved");

    let mut tx = connection.begin_transaction().await?;
    assert!(matches!(
        dispatcher
            .dispatch_after_commit(Saved, &mut EventContext::in_transaction(&mut tx))
            .await?,
        DispatchResult::Deferred
    ));
    tx.roll_back(None).await?;
    replacement
        .assert_dispatched_times(TransactionBeginning::NAME, 1)
        .assert_dispatched_times(TransactionRolledBack::NAME, 1)
        .assert_not_dispatched("greeting.saved");
    fake.assert_dispatched_times("greeting.saved", 1);
    assert_eq!(count.load(Ordering::Relaxed), 0);
    Ok(())
}
