#[path = "../config_app.rs"]
mod app;
#[path = "../../../config/mod.rs"]
mod config;
#[cfg(feature = "pay")]
mod pay;
mod sms;
mod storage;

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    filesystem::{FilesystemAdapter, FilesystemManager},
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Route, Sms, Storage},
    },
};
use serde_json::json;
use std::{
    error::Error,
    process::ExitCode,
    sync::{Arc, atomic::AtomicUsize},
};

struct StorageProvider;
impl ServiceProvider for StorageProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<FilesystemManager>()?
                .extend("example", |manager, profile| {
                    Ok(Arc::new(FilesystemAdapter::custom(
                        Arc::new(storage::External {
                            disk: manager.disk(Some("backing"))?,
                            writes: Arc::new(AtomicUsize::new(0)),
                        }),
                        true,
                        false,
                        profile["url"].as_str().map(str::to_owned),
                    )))
                });
            Ok(())
        })
    }
}
async fn probe() -> Result<serde_json::Value, Box<dyn Error + Send + Sync>> {
    Storage::extend("ondemand_example", |manager, _| {
        manager.disk(Some("external"))
    })?;
    let disk = Storage::build_from_config(json!({"driver":"ondemand_example"}))?;
    assert!(Arc::ptr_eq(&disk, &Storage::disk(Some("external"))?));
    let count = disk
        .get("counter")
        .await?
        .map(|b| String::from_utf8(b).unwrap().parse::<u32>().unwrap())
        .unwrap_or(0)
        + 1;
    disk.put("counter", count.to_string()).await?;
    let receipt = Sms::driver("example_primary")?
        .send(openrust::notifications::sms::SmsMessage::new(
            "+12025550123",
            "probe",
        )?)
        .await?;
    Ok(json!({"counter":count,"url":disk.url("counter")?,"sms":receipt}))
}
fn http(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let value = probe()
            .await
            .map_err(|source| HttpError::Internal { source })?;
        Response::json(openrust::http::StatusCode::OK, &value)
    })
}
struct Probe(CommandDefinition);
impl Command for Probe {
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
            let value = probe()
                .await
                .map_err(|e| ConsoleError::new(e.to_string()))?;
            output
                .write_raw(&format!("{value}\n"))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}
fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(clap::Command::new(
        "drivers:probe",
    )))))?;
    #[cfg(feature = "pay")]
    pay::register_command(artisan)?;
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_none() && std::env::var_os("DRIVER_HTTP").is_none() {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root = Application::infer_base_path(Some(env!("CARGO_MANIFEST_DIR")))?;
        let builder = Application::configure(root)
            .with_config(|path, env| {
                let mut config = config::load(path, env)?;
                config.get_mut("filesystems").unwrap()["disks"]["backing"] =
                    json!({"driver":"local","root":"storage/probe","throw":false});
                config.get_mut("filesystems").unwrap()["disks"]["external"] =
                    json!({"driver":"example","url":"https://cdn.example/assets"});
                config.get_mut("sms").unwrap()["drivers"]["example_primary"] =
                    json!({"driver":"example","endpoint":env.get_utf8("SMS_EXAMPLE_ENDPOINT")?});
                #[cfg(feature = "pay")]
                config.insert("pay".into(), pay::configuration(env)?);
                Ok(config)
            })
            .with_providers(vec![
                Box::new(StorageProvider),
                Box::new(sms::ExampleSmsProvider),
                #[cfg(feature = "pay")]
                Box::new(openrust_pay::PayServiceProvider),
                #[cfg(feature = "pay")]
                Box::new(pay::ExternalPayProvider),
            ])
            .with_commands(commands)
            .with_routing(
                Routing::default()
                    .web(|| Route::get("/drivers", http))
                    .api(|| {
                        #[cfg(feature = "pay")]
                        Route::post("/drivers/pay", pay::callback)?;
                        Ok(())
                    })
                    .health("/up"),
            );
        #[cfg(feature = "pay")]
        let builder = builder.with_migrations(pay::MIGRATIONS);
        builder.create().map_err(Into::into)
    })
    .await
}
