//! Runs the actual framework entry in child processes with a test provider.
use openrust::{
    console::{
        Command as FrameworkCommand, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output,
    },
    container::{ParameterKey, ParameterOverrides, ParameterValue, ServiceKey},
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
struct ReboundResults;
impl Facade for ReboundResults {
    type Service = u32;
    fn getFacadeAccessor() -> ServiceKey {
        "rebind-result".into()
    }
}
struct ExtendedProbe(u32);
struct ExtendedProbes;
impl Facade for ExtendedProbes {
    type Service = ExtendedProbe;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<ExtendedProbe>()
    }
}
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
struct AsyncProbeCommand(CommandDefinition);
impl FrameworkCommand for AsyncProbeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let value = app
                .get::<u32>("async-sum")
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            output.stdout.push_str(&value.to_string());
            Ok(0)
        })
    }
}
struct ParametersProbeCommand(CommandDefinition);
impl FrameworkCommand for ParametersProbeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let mut parameters = ParameterOverrides::new();
            parameters.insert(ParameterKey::name("number"), ParameterValue::new(12u32));
            let value = app
                .makeWithNamed::<u32>("parameter-number", parameters)
                .await
                .map_err(|e| ConsoleError::new(e.to_string()))?;
            let cached = app
                .get::<u32>("parameter-number")
                .await
                .map_err(|e| ConsoleError::new(e.to_string()))?;
            output.stdout.push_str(&format!("{value}:{cached}"));
            Ok(0)
        })
    }
}
struct Provider;
impl ServiceProvider for Provider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singletonAsyncNamed::<u32, _>("parameter-number", |c| {
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        let value = c
                            .getLastParameterOverride()
                            .get(&ParameterKey::name("number"))
                            .map(|v| v.downcast::<u32>())
                            .transpose()?
                            .map_or(0, |v| *v);
                        Ok(Arc::new(value))
                    })
                })
                .await?;
            app.container()
                .bindAsyncNamed::<u32, _>("async-number", |_| {
                    Box::pin(async {
                        tokio::task::yield_now().await;
                        Ok(Arc::new(3))
                    })
                })
                .await?;
            app.container()
                .bindAsyncNamed::<u32, _>("async-sum", |c| {
                    Box::pin(
                        async move { Ok(Arc::new(*c.getAsync::<u32>("async-number").await? + 4)) },
                    )
                })
                .await?;
            app.container()
                .instanceNamed("rebind-probe", Arc::new(1_u32))?;
            app.container()
                .rebinding::<u32, _>("rebind-probe", |container, value| {
                    container.instanceNamed("rebind-result", value)?;
                    Ok(())
                })?;
            app.container()
                .instanceNamed("rebind-probe", Arc::new(2_u32))?;
            app.container()
                .bind::<ExtendedProbe, _>(|_| Ok(Arc::new(ExtendedProbe(1))))
                .unwrap();
            app.container()
                .extend::<ExtendedProbe, _>(ServiceKey::typed::<ExtendedProbe>(), |value, _| {
                    Ok(Arc::new(ExtendedProbe(value.0 + 10)))
                })?;
            app.container()
                .scoped::<ScopedProbe, _>(|_| Ok(Arc::new(ScopedProbe)))
                .unwrap();
            app.container().afterResolving(
                Some(ServiceKey::typed::<ScopedProbe>()),
                |event, c| {
                    assert!(Arc::ptr_eq(
                        &event.value::<ScopedProbe>().unwrap(),
                        &c.make::<ScopedProbe>()?
                    ));
                    c.instanceNamed("hook-result", Arc::new(String::from("hooked")))
                        .unwrap();
                    Ok(())
                },
            )?;
            app.container()
                .bindIf::<Greeting, _>(|_| Ok(Arc::new(Greeting("from provider".into()))))
                .unwrap();
            let key = ServiceKey::typed::<Greeting>();
            assert!(!app.container().isShared(key.clone()));
            assert!(!app.container().resolved(key.clone())?);
            app.container()
                .instance(Arc::new(Greeting("temporary override".into())))
                .unwrap();
            assert_eq!(app.container().make::<Greeting>()?.0, "temporary override");
            app.container().forgetInstance(key.clone());
            assert_eq!(app.container().make::<Greeting>()?.0, "from provider");
            assert!(app.container().resolved(key)?);
            app.container()
                .alias(ServiceKey::typed::<Greeting>(), "greeting")?;
            app.container()
                .bind::<ContextProbe, _>(|c| Ok(Arc::new(ContextProbe(c.make()?))))
                .unwrap();
            app.container()
                .when::<ContextProbe>()?
                .needs::<Greeting>()
                .give(|_| Ok(Arc::new(Greeting("contextual".into()))))?;
            Ok(())
        })
    }
}
fn routes() -> Result<(), ConsoleError> {
    Artisan::command("context:probe", |output| {
        let contextual = ContextProbes::getFacadeRoot().unwrap();
        output.stdout.push_str(&contextual.0.0);
        assert_eq!(Greetings::getFacadeRoot().unwrap().0, "from provider");
    })?;
    Artisan::command("rebind:probe", |output| {
        output
            .stdout
            .push_str(&ReboundResults::getFacadeRoot().unwrap().to_string());
    })?;
    Artisan::command("extend:probe", |output| {
        output
            .stdout
            .push_str(&ExtendedProbes::getFacadeRoot().unwrap().0.to_string());
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
        .withCommands(|artisan| {
            artisan.add_command(Box::new(ParametersProbeCommand(CommandDefinition::new(
                clap::Command::new("parameters:probe"),
            ))))?;
            artisan.add_command(Box::new(AsyncProbeCommand(CommandDefinition::new(
                clap::Command::new("async:probe"),
            ))))
        })
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
        (vec!["artisan", "extend:probe"], 0),
        (vec!["artisan", "rebind:probe"], 0),
        (vec!["artisan", "async:probe"], 0),
        (vec!["artisan", "parameters:probe"], 0),
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
                } else if args.last() == Some(&"extend:probe") {
                    b"11".as_slice()
                } else if args.last() == Some(&"rebind:probe") {
                    b"2".as_slice()
                } else if args.last() == Some(&"parameters:probe") {
                    b"12:0".as_slice()
                } else if args.last() == Some(&"async:probe") {
                    b"7".as_slice()
                } else {
                    b"from provider".as_slice()
                }
            );
            assert!(output.stderr.is_empty());
        } else {
            assert!(!output.stderr.is_empty());
        }
    }
    println!("9 container/provider Artisan process checks passed");
    ExitCode::SUCCESS
}
