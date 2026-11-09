//! 真实 Application/裸 `ProviderBuilder` 的模块命令顺序与业务覆盖合同。

use figment::{Figment, providers::Serialized};
use openrust::Application;
use openrust::Result;
use openrust::config::NoConfig;
use openrust::console::Command;
use openrust::container::ServiceKey;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::foundation::ProviderBuilder;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, DefaultProviders, ServiceProvider};
use openrust_modules::{
    ModuleDescriptor, ModuleManifest, ModuleProviderBuilderExt, ModuleRepository,
    ModuleTestAppBuilderExt, ModulesConfig,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Deserialize)]
struct Settings {
    module_root: PathBuf,
}
struct WithModules<const OVERRIDE: bool>;
impl<const OVERRIDE: bool> ApplicationDefinition for WithModules<OVERRIDE> {
    type Config = Settings;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
            .merge([Box::new(openrust::events::EventServiceProvider) as Box<dyn ServiceProvider>])
    }
    fn providers(_: &Settings) -> Vec<Box<dyn ServiceProvider>> {
        if OVERRIDE {
            vec![Box::new(BusinessCommands)]
        } else {
            Vec::new()
        }
    }
    fn configure_providers(config: &Settings, builder: &mut ProviderBuilder) {
        builder.module_manifest(manifest(&config.module_root));
    }
}
fn manifest(root: &std::path::Path) -> ModuleManifest {
    ModuleManifest::from_file(root.join("statuses.json"))
        .with_application_root(root)
        .module(ModuleDescriptor::new("Blog", root.join("blog"), 0).provider(ModuleMarker))
}
fn fixture() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir().map_err(openrust::Error::internal)?;
    std::fs::create_dir(root.path().join("blog")).map_err(openrust::Error::internal)?;
    std::fs::write(root.path().join("statuses.json"), br#"{"Blog":true}"#)
        .map_err(openrust::Error::internal)?;
    Ok(root)
}
fn source(root: &std::path::Path) -> Figment {
    Figment::from(Serialized::defaults(json!({"module_root":root})))
}
struct Calls(AtomicUsize);
struct BusinessCommands;
impl ServiceProvider for BusinessCommands {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.instance(Arc::new(Calls(AtomicUsize::new(0)))).await })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(BusinessList);
            Ok(())
        })
    }
}
struct BusinessList;
impl Command for BusinessList {
    fn name(&self) -> &'static str {
        "module:list"
    }
    fn about(&self) -> &'static str {
        "业务覆盖列表"
    }
    fn run<'a>(&'a self, app: &'a Application, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.state()
                .resolve::<Calls>()
                .await?
                .0
                .fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}
struct ModuleMarker;
struct Installed;
impl ServiceProvider for ModuleMarker {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            assert!(app.is_bound_key(&ServiceKey::named("modules")));
            app.instance(Arc::new(Installed)).await
        })
    }
}

#[tokio::test]
async fn application_core_commands_precede_modules_and_business_boot_overrides_only_its_name()
-> Result<()> {
    let root = fixture()?;
    let settings = Settings {
        module_root: root.path().to_owned(),
    };
    let providers = ProviderBuilder::for_application::<WithModules<true>>(&settings);
    let app = Application::from_provider_runtime(providers.start(source(root.path())).await?)?;
    let outcome: Result<()> = async {
        let names = app
            .commands()
            .map(|command| command.name())
            .collect::<Vec<_>>();
        let position = |name| {
            names
                .iter()
                .position(|candidate| *candidate == name)
                .ok_or_else(|| openrust::Error::custom("test_command_missing", "未安装预期命令"))
        };
        assert!(position("config:clear")? < position("module:enable")?);
        assert!(position("module:enable")? < position("module:disable")?);
        assert!(position("module:disable")? < position("module:list")?);
        let listing = app
            .commands()
            .find(|command| command.name() == "module:list")
            .ok_or_else(|| openrust::Error::custom("test_command_missing", "缺模块列表命令"))?;
        assert_eq!(listing.about(), "业务覆盖列表");
        app.run_command("module:list", Vec::new()).await?;
        assert_eq!(
            app.state()
                .resolve::<Calls>()
                .await?
                .0
                .load(Ordering::SeqCst),
            1
        );
        let container = app.state().container();
        let key = ServiceKey::named("modules");
        assert!(container.is_alias(&key));
        assert!(!container.resolved_key(&key)?);
        let repository = container.resolve_key::<ModuleRepository>(key).await?;
        assert!(Arc::ptr_eq(
            &repository,
            &app.state().resolve::<ModuleRepository>().await?
        ));
        app.run_command("module:disable", vec!["Blog".into()])
            .await?;
        assert!(repository.find_or_fail("Blog")?.is_disabled()?);
        app.run_command("module:enable", vec!["Blog".into()])
            .await?;
        assert!(repository.find_or_fail("Blog")?.is_enabled()?);
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

#[tokio::test]
async fn bare_builder_gets_module_commands_only_when_extension_is_selected() -> Result<()> {
    for selected in [false, true] {
        let mut builder = ProviderBuilder::new();
        builder.remove_module("Blog");
        if selected {
            builder.module_manifest(ModuleManifest::new());
        }
        let app = Application::from_provider_runtime(builder.start(Figment::new()).await?)?;
        let outcome: Result<()> = async {
            let names = app
                .commands()
                .map(|command| command.name())
                .collect::<Vec<_>>();
            for name in ["module:enable", "module:disable", "module:list"] {
                assert_eq!(names.contains(&name), selected);
            }
            assert!(!names.contains(&"config:clear"));
            let container = app.state().container();
            let key = ServiceKey::named("modules");
            assert_eq!(container.is_alias(&key), selected);
            if selected {
                assert!(container.contains::<ModuleRepository>());
                app.run_command("module:list", Vec::new()).await?;
                assert!(container.resolved_key(&key)?);
            } else {
                let missing = container.resolve_key::<ModuleRepository>(key).await.err();
                assert_eq!(
                    missing.as_ref().map(openrust::Error::code),
                    Some("service_not_bound")
                );
            }
            Ok(())
        }
        .await;
        let shutdown = app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await;
        outcome?;
        shutdown?;
    }
    Ok(())
}

#[tokio::test]
async fn module_configuration_defaults_only_fill_missing_fields() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.module_manifest(ModuleManifest::new());
    let runtime = builder.start(Figment::from(Serialized::defaults(json!({
        "modules":{"activator":"registered-custom", "paths":{"modules":"custom-modules"},
            "scan":{"enabled":true}, "activators":{"file":{"statuses-file":"custom-state.json"}}}
    })))).await?;
    let outcome: Result<()> = async {
        let config: ModulesConfig = runtime.config().section("modules")?;
        assert_eq!(config.activator, "registered-custom");
        assert_eq!(config.paths.modules, PathBuf::from("custom-modules"));
        assert!(config.scan.enabled);
        assert_eq!(config.scan.paths, vec![PathBuf::from("vendor/*/*")]);
        assert_eq!(
            config.activators.file.statuses_file,
            PathBuf::from("custom-state.json")
        );
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

struct HttpModules;
impl ApplicationDefinition for HttpModules {
    type Config = Settings;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &Settings) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(openrust::http::HttpServiceProvider)]
    }
    fn configure_providers(config: &Settings, builder: &mut ProviderBuilder) {
        builder.module_manifest(manifest(&config.module_root));
    }
}

struct BareHttp;
impl ApplicationDefinition for BareHttp {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(openrust::http::HttpServiceProvider)]
    }
}

#[tokio::test]
async fn test_app_remove_module_without_extension_stays_without_module_services_after_reset()
-> Result<()> {
    let mut blueprint = openrust::testing::TestApp::<BareHttp>::builder(Figment::new());
    blueprint.remove_module("Blog");
    let mut fixture = blueprint.build().await?;
    let outcome: Result<()> = async {
        for reset in [false, true] {
            if reset {
                fixture
                    .reset(tokio::time::Instant::now() + Duration::from_secs(5))
                    .await?;
            }
            let app = fixture.app()?;
            assert!(!app.state().container().contains::<ModuleRepository>());
            assert!(
                !app.commands()
                    .any(|command| command.name().starts_with("module:"))
            );
        }
        Ok(())
    }
    .await;
    let shutdown = fixture
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

#[tokio::test]
async fn test_app_remove_module_is_replayed_on_reset_without_changing_file_status() -> Result<()> {
    let root = fixture()?;
    let mut blueprint = openrust::testing::TestApp::<HttpModules>::builder(source(root.path()));
    blueprint.remove_module("Blog");
    let mut fixture = blueprint.build().await?;
    let outcome: Result<()> = async {
        for reset in [false, true] {
            if reset {
                fixture
                    .reset(tokio::time::Instant::now() + Duration::from_secs(5))
                    .await?;
            }
            assert!(!fixture.app()?.state().container().contains::<Installed>());
            let repository = fixture.app()?.state().resolve::<ModuleRepository>().await?;
            assert!(!repository.find_or_fail("Blog")?.is_loaded());
            let statuses: serde_json::Value = serde_json::from_slice(
                &std::fs::read(root.path().join("statuses.json"))
                    .map_err(openrust::Error::internal)?,
            )
            .map_err(openrust::Error::internal)?;
            assert_eq!(statuses["Blog"], json!(true));
        }
        Ok(())
    }
    .await;
    let shutdown = fixture
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}
