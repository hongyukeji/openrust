use std::{error::Error, sync::Arc};

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Facade, Route},
    testing::{FacadeSpy, TestApp},
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

trait Lookup: Send + Sync {
    fn find(&self, id: &str) -> String;
}

struct Catalog;

impl Lookup for Catalog {
    fn find(&self, id: &str) -> String {
        format!("catalog:{id}")
    }
}

struct CatalogFacade;

impl Facade for CatalogFacade {
    type Service = dyn Lookup;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<dyn Lookup>()
    }
}

#[derive(Debug, PartialEq)]
struct Find(String);

struct RecordingCatalog {
    calls: FacadeSpy<Find>,
    original: Arc<dyn Lookup>,
}

impl Lookup for RecordingCatalog {
    fn find(&self, id: &str) -> String {
        self.calls.record("find", Find(id.to_owned()));
        self.original.find(id)
    }
}

fn catalog() -> Result<Arc<dyn Lookup>, HttpError> {
    CatalogFacade::getFacadeRoot().map_err(|source| HttpError::Internal {
        source: Box::new(source),
    })
}

fn lookup(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Response::json(StatusCode::OK, catalog()?.find("web")) })
}

fn install_inside_scope(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let original = catalog()?;
        original.find("before");
        let spy = CatalogFacade::spy(|calls| Arc::new(RecordingCatalog { calls, original }))
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        let value = catalog()?.find("inside");
        spy.shouldHaveReceived("find")
            .with(Find("inside".into()))
            .once();
        spy.shouldNotHaveReceivedWhere("find", |call| call.0 == "before");
        Response::json(StatusCode::OK, value)
    })
}

struct LookupCommand(CommandDefinition);

impl Command for LookupCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let service = CatalogFacade::getFacadeRootAsync()
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            output
                .stdout
                .push_str(&format!("{}\n", service.find("cli")));
            Ok(0)
        })
    }
}

fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(LookupCommand(CommandDefinition::new(
        clap::Command::new("lookup:probe"),
    ))))
}

async fn fixture() -> TestResult<TestApp> {
    let mut app = TestApp::create(|builder| {
        builder
            .withCommands(commands)
            .withRouting(Routing::default().web(|| {
                Route::get("/lookup", lookup)?;
                Route::get("/install-spy", install_inside_scope)
            }))
    })?;
    app.instance::<dyn Lookup>(Arc::new(Catalog)).await?;
    Ok(app)
}

#[tokio::test]
async fn spy_records_http_artisan_and_container_calls_in_one_app() -> TestResult {
    let mut first = fixture().await?;
    let mut second = fixture().await?;
    let original = first.app().make::<dyn Lookup>().await?;
    let spy = first
        .spy::<CatalogFacade, _>(|calls| Arc::new(RecordingCatalog { calls, original }))
        .await?;
    assert_eq!(spy.received("find").len(), 0);
    let (left, right) = tokio::join!(first.get("/lookup"), second.get("/lookup"));
    left?.assertOk().assertContent(r#""catalog:web""#);
    right?.assertOk().assertContent(r#""catalog:web""#);
    first
        .artisan(["lookup:probe"])
        .await
        .assertSuccessful()
        .expectsOutput("catalog:cli");
    second
        .artisan(["lookup:probe"])
        .await
        .assertSuccessful()
        .expectsOutput("catalog:cli");
    first.app().make::<dyn Lookup>().await?.find("direct");
    spy.shouldHaveReceived("find").times(3);
    spy.shouldHaveReceived("find")
        .with(Find("web".into()))
        .once();
    spy.shouldHaveReceived("find")
        .withArgs(|call| {
            spy.shouldNotHaveReceived("other");
            call.0 != "web"
        })
        .times(2);
    spy.shouldNotHaveReceivedWhere("find", |call| call.0 == "missing");
    let calls = spy.received("find");
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        ["web", "cli", "direct"]
    );
    Ok(())
}

#[tokio::test]
async fn facade_spy_replaces_an_already_resolved_root() -> TestResult {
    let result = CatalogFacade::spy::<Find>(|_| panic!("replacement requires an application"));
    assert!(matches!(result, Err(BindingResolutionError::FacadeNotSet)));
    let mut app = fixture().await?;
    app.get("/install-spy")
        .await?
        .assertOk()
        .assertContent(r#""catalog:inside""#);
    app.artisan(["lookup:probe"])
        .await
        .assertSuccessful()
        .expectsOutput("catalog:cli");
    Ok(())
}
