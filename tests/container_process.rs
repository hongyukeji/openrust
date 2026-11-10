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
struct Provider;
impl ServiceProvider for Provider {
    fn register(&mut self, app: &Application) -> ProviderResult {
        app.container()
            .bind::<Greeting, _>(|_| Ok(Arc::new(Greeting("from provider".into()))));
        app.container()
            .alias(ServiceKey::typed::<Greeting>(), "greeting")?;
        Ok(())
    }
}
fn routes() -> Result<(), ConsoleError> {
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
            assert_eq!(output.stdout, b"from provider");
            assert!(output.stderr.is_empty());
        } else {
            assert!(!output.stderr.is_empty());
        }
    }
    println!("3 container/provider Artisan process checks passed");
    ExitCode::SUCCESS
}
