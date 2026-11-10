//! Runs the actual framework entry in child processes with a test provider.
use openrust::{
    console::{ConsoleError, Output},
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::Routing},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Facade},
    },
};
use std::{
    collections::BTreeMap,
    error::Error,
    process::{Command, ExitCode},
    sync::Arc,
};

struct Greeting(String);
struct Greetings;
impl Facade for Greetings {
    type Service = Greeting;
    fn getFacadeAccessor() -> ServiceKey {
        "greeting".into()
    }
}
struct ScopedProbe;
struct ContextProbe(Arc<Greeting>);
struct ContextProbes;
impl Facade for ContextProbes {
    type Service = ContextProbe;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<ContextProbe>()
    }
}
struct ScopedProbes;
struct HookResults;
impl Facade for HookResults {
    type Service = String;
    fn getFacadeAccessor() -> ServiceKey {
        "hook-result".into()
    }
}
impl Facade for ScopedProbes {
    type Service = ScopedProbe;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<ScopedProbe>()
    }
}
struct Provider;
impl ServiceProvider for Provider {
    fn register(&mut self, app: &Application) -> ProviderResult {
        app.container()
            .scoped::<ScopedProbe, _>(|_| Ok(Arc::new(ScopedProbe)));
        app.container()
            .afterResolving(Some(ServiceKey::typed::<ScopedProbe>()), |event, c| {
                assert!(Arc::ptr_eq(
                    &event.value::<ScopedProbe>().unwrap(),
                    &c.make::<ScopedProbe>()?
                ));
                c.instanceNamed("hook-result", Arc::new(String::from("hooked")));
                Ok(())
            })?;
        app.container()
            .bindIf::<Greeting, _>(|_| Ok(Arc::new(Greeting("from provider".into()))));
        let key = ServiceKey::typed::<Greeting>();
        assert!(!app.container().isShared(key.clone()));
        assert!(!app.container().resolved(key.clone())?);
        app.container()
            .instance(Arc::new(Greeting("temporary override".into())));
        assert_eq!(app.container().make::<Greeting>()?.0, "temporary override");
        app.container().forgetInstance(key.clone());
        assert_eq!(app.container().make::<Greeting>()?.0, "from provider");
        assert!(app.container().resolved(key)?);
        app.container()
            .alias(ServiceKey::typed::<Greeting>(), "greeting")?;
        app.container()
            .bind::<ContextProbe, _>(|c| Ok(Arc::new(ContextProbe(c.make()?))));
        app.container()
            .when::<ContextProbe>()?
            .needs::<Greeting>()
            .give(|_| Ok(Arc::new(Greeting("contextual".into()))))?;
        Ok(())
    }
}
fn routes() -> Result<(), ConsoleError> {
    Artisan::command("context:probe", |output| {
        let contextual = ContextProbes::getFacadeRoot().unwrap();
        output.stdout.push_str(&contextual.0.0);
        assert_eq!(Greetings::getFacadeRoot().unwrap().0, "from provider");
    })?;
    Artisan::command("hooks:probe", |output| {
        ScopedProbes::getFacadeRoot().unwrap();
        output
            .stdout
            .push_str(&HookResults::getFacadeRoot().unwrap());
    })?;
    Artisan::command("scope:probe", |output: &mut Output| {
        let first = ScopedProbes::getFacadeRoot().unwrap();
        let second = ScopedProbes::getFacadeRoot().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        output.stdout.push_str("scoped");
    })?;
    Artisan::command("probe", |output: &mut Output| {
        let first = Greetings::getFacadeRoot().unwrap();
        let second = Greetings::getFacadeRoot().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        output.stdout.push_str(&first.0);
    })?;
    Ok(())
}
fn create() -> Result<Application, Box<dyn Error + Send + Sync>> {
    Ok(Application::configure(env!("CARGO_MANIFEST_DIR").into())
        .withConfig(|_, _| Ok(BTreeMap::new()))
        .withProviders(vec![Box::new(Provider)])
        .withRouting(Routing::default().commands(routes))
        .create()?)
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::var_os("TEST_CONTAINER_CHILD").is_some() {
        return openrust::foundation::runApplication(create).await;
    }
    let executable = std::env::current_exe().unwrap();
    for (args, expected) in [
        (vec!["artisan", "probe"], 0),
        (vec!["artisan", "scope:probe"], 0),
        (vec!["artisan", "context:probe"], 0),
        (vec!["artisan", "hooks:probe"], 0),
        (vec!["artisan", "unknown-command"], 1),
        (vec!["wrong-mode"], 2),
    ] {
        let output = Command::new(&executable)
            .args(&args)
            .env("TEST_CONTAINER_CHILD", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if expected == 0 {
            assert_eq!(
                output.stdout,
                if args.last() == Some(&"scope:probe") {
                    b"scoped".as_slice()
                } else if args.last() == Some(&"context:probe") {
                    b"contextual".as_slice()
                } else if args.last() == Some(&"hooks:probe") {
                    b"hooked".as_slice()
                } else {
                    b"from provider".as_slice()
                }
            );
            assert!(output.stderr.is_empty());
        } else {
            assert!(!output.stderr.is_empty());
        }
    }
    println!("6 container/provider Artisan process checks passed");
    ExitCode::SUCCESS
}
