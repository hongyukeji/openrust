//! 在真实生成应用中验证持久启停与已加载资源快照；仅追加一个已编译模块。

#[path = "../app/mod.rs"]
mod app;
#[path = "../bootstrap/app.rs"]
mod bootstrap;
#[path = "../config/mod.rs"]
mod config;
#[path = "../modules/mod.rs"]
mod modules;
#[path = "../routes/mod.rs"]
mod routes;

use bootstrap::__GENERATED_APPLICATION__ as Original;
use openrust::config::{ConfigLoader, ConfigManifest, ConfigSources};
use openrust::core::{
    Application, BootContext, BoxFuture, DefaultProviders, Module, ModuleFile, RegisterContext,
    ModuleManifest, ModuleRepository, ServiceProvider,
};
use openrust::http::{
    RouteGroup, Routes,
    axum::{
        body::Body,
        http::{Request, StatusCode},
    },
};
use openrust::testing::TestApp;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const MODULE_NAME: &str = "ActivationFixture";
const MODULE_ROUTE: &str = "/api/module-fixture";

struct ActivationApplication;

impl Application for ActivationApplication {
    type Config = <Original as Application>::Config;

    fn config() -> ConfigManifest {
        Original::config()
    }

    fn config_namespace() -> &'static str {
        Original::config_namespace()
    }

    fn default_providers() -> DefaultProviders {
        Original::default_providers()
    }

    fn package_providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        Original::package_providers(config)
    }

    fn additional_providers() -> Vec<Box<dyn ServiceProvider>> {
        Original::additional_providers()
    }

    fn with_bootstrap_providers() -> bool {
        Original::with_bootstrap_providers()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        Original::providers(config)
    }

    fn modules(config: &Self::Config) -> ModuleManifest {
        Original::modules(config)
            .compiled_provider("ActivationProvider", |_| Ok(ActivationProvider))
            .compiled_module_file(MODULE_NAME, "module_init.rs", ActivationFile(Arc::new(AtomicUsize::new(0))))
    }
}

struct ActivationProvider;

impl ServiceProvider for ActivationProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Routes| {
                routes.group(
                    RouteGroup::new().prefix("/api").middleware_group("api"),
                    |routes| routes.get("/module-fixture", module_response),
                )
            })?;
            Ok(())
        })
    }
}

struct ActivationFile(Arc<AtomicUsize>);
struct FileMarker(Arc<AtomicUsize>);
impl ModuleFile for ActivationFile {
    fn initialize<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            app.instance(Arc::new(FileMarker(Arc::clone(&self.0)))).await?;
            let marker = app.resolve::<FileMarker>().await?;
            if !Arc::ptr_eq(&marker.0, &self.0) {
                return Err(openrust::Error::custom("module_initializer_container_changed", "文件初始化没有使用同一容器"));
            }
            Ok(())
        })
    }
}

async fn module_response(openrust::http::Inject(marker): openrust::http::Inject<FileMarker>) -> openrust::Result<&'static str> {
    if marker.0.load(Ordering::SeqCst) != 1 {
        return Err(openrust::Error::custom("module_initializer_duplicate", "模块编译文件没有遵守 include_once"));
    }
    Ok("compiled activation fixture")
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

fn sources(root: &Path) -> TestResult<ConfigSources> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(ConfigLoader::application_code_sources_with_overrides(
        root,
        "MODULE_ACTIVATION_FIXTURE_",
        None,
        &Original::config(),
        [
            (
                "app",
                json!({"env":"testing", "name":"module-activation-host", "url":"http://module-activation.test"}),
            ),
            ("application_root", json!(root)),
            (
                "modules",
                json!({"activator":"file", "activators":{"file":{"statuses-file":"state/modules.json"}}}),
            ),
            (
                "database",
                json!({"url":null, "instances":{}, "read_replicas":[]}),
            ),
            (
                "cache",
                json!({"default":"fixture", "stores":{"fixture":{"driver":"memory", "namespace":"module-activation"}}}),
            ),
            (
                "view",
                json!({"directory":project.join("resources/views"), "auto_reload":false}),
            ),
        ],
    )?)
}

async fn route_status(
    app: &TestApp<ActivationApplication>,
    path: &str,
    status: StatusCode,
) -> TestResult {
    app.request(Request::get(path).body(Body::empty())?)
        .await?
        .assert_status(status)?;
    Ok(())
}

fn state(root: &Path) -> TestResult<Value> {
    Ok(serde_json::from_slice(&fs::read(
        root.join("state/modules.json"),
    )?)?)
}

#[tokio::test]
async fn generated_application_persists_module_activation_without_hot_resource_changes()
-> TestResult {
    let mut root = OwnedTempRoot::new()?;
    fs::create_dir_all(root.path.join("modules/activation_fixture"))?;
    fs::write(root.path.join("modules/activation_fixture/module.json"), br#"{"name":"ActivationFixture","priority":0,"providers":[["ActivationProvider"],"ActivationProvider"],"files":["module_init.rs","./module_init.rs"]}"#)?;
    fs::write(root.path.join("modules/activation_fixture/module_init.rs"), "// 已编译初始化的运行路径凭证\n")?;
    fs::create_dir(root.path.join("state"))?;
    fs::write(
        root.path.join("state/modules.json"),
        br#"{"ActivationFixture":true,"unrelated":{"keep":17}}"#,
    )?;
    let mut apps = Vec::<TestApp<ActivationApplication>>::new();
    let outcome: TestResult = async {
        apps.push(
            TestApp::<ActivationApplication>::builder(sources(&root.path)?)
                .build()
                .await?,
        );
        route_status(&apps[0], MODULE_ROUTE, StatusCode::OK).await?;
        let repository = apps[0].app()?.state().resolve::<ModuleRepository>().await?;
        let module: Module = repository.find_or_fail(MODULE_NAME)?;
        ensure(
            repository.find("activation-fixture").is_some(),
            "Studly 名称无法找到已编译模块",
        )?;
        ensure(
            module.is_loaded() && module.is_enabled()?,
            "首个 App 未加载已启用模块",
        )?;
        ensure(
            repository.statuses_file_path() == Some(root.path.join("state/modules.json").as_path()),
            "模块仓储没有使用独立状态路径",
        )?;

        let before = fs::read(root.path.join("state/modules.json"))?;
        let invalid = apps[0]
            .app()?
            .run_command(
                "module:disable",
                vec![
                    MODULE_NAME.into(),
                    "MissingSecondModule".into(),
                    "--no-interaction".into(),
                ],
            )
            .await;
        let error = invalid
            .err()
            .ok_or_else(|| std::io::Error::other("未知第二名称没有导致命令失败"))?;
        ensure(
            error.code() == "module_not_found",
            "未知第二名称返回了错误的命令失败类别",
        )?;
        ensure(
            fs::read(root.path.join("state/modules.json"))? == before,
            "全部名称验证之前发生了持久写入",
        )?;

        apps[0]
            .app()?
            .run_command(
                "module:enable",
                vec![
                    "MissingIgnoredByAll".into(),
                    "--all".into(),
                    "--no-interaction".into(),
                ],
            )
            .await?;
        ensure(
            state(&root.path)?.get("MissingIgnoredByAll").is_none(),
            "--all 没有覆盖显式错误名称",
        )?;
        apps[0]
            .app()?
            .run_command(
                "module:disable",
                vec!["activation-fixture".into(), "--no-interaction".into()],
            )
            .await?;
        ensure(
            module.is_loaded() && !module.is_enabled()?,
            "持久禁用改变了当前 loaded 快照或未写状态",
        )?;
        route_status(&apps[0], MODULE_ROUTE, StatusCode::OK).await?;
        let disabled = state(&root.path)?;
        ensure(
            disabled[MODULE_NAME] == false && disabled.get("activation-fixture").is_none(),
            "Studly 参数没有写 canonical 状态键",
        )?;

        apps.push(
            TestApp::<ActivationApplication>::builder(sources(&root.path)?)
                .build()
                .await?,
        );
        route_status(&apps[1], MODULE_ROUTE, StatusCode::NOT_FOUND).await?;
        let host = apps[1]
            .request(Request::get("/api").body(Body::empty())?)
            .await?;
        host.assert_status(StatusCode::OK)?;
        ensure(
            serde_json::from_slice::<Value>(host.body())?["data"]["name"]
                == "module-activation-host",
            "禁用模块后原应用 Provider/API 未保留",
        )?;
        let second_repository = apps[1].app()?.state().resolve::<ModuleRepository>().await?;
        let second_module: Module = second_repository.find_or_fail(MODULE_NAME)?;
        ensure(
            !second_module.is_loaded() && !second_module.is_enabled()?,
            "第二 App 仍加载了禁用模块",
        )?;

        apps[0]
            .app()?
            .run_command(
                "module:enable",
                vec!["ACTIVATIONFIXTURE".into(), "--no-interaction".into()],
            )
            .await?;
        ensure(
            !second_module.is_loaded() && second_module.is_enabled()?,
            "持久启用错误地热挂载了第二 App",
        )?;
        route_status(&apps[1], MODULE_ROUTE, StatusCode::NOT_FOUND).await?;
        let enabled = state(&root.path)?;
        ensure(
            enabled[MODULE_NAME] == true && enabled.get("ACTIVATIONFIXTURE").is_none(),
            "大小写查找没有写 canonical 状态键",
        )?;
        ensure(
            enabled["unrelated"] == json!({"keep":17}),
            "持久管理丢失未知状态值",
        )?;

        apps.push(
            TestApp::<ActivationApplication>::builder(sources(&root.path)?)
                .build()
                .await?,
        );
        route_status(&apps[2], MODULE_ROUTE, StatusCode::OK).await?;
        let third_repository = apps[2].app()?.state().resolve::<ModuleRepository>().await?;
        ensure(
            third_repository.find_or_fail(MODULE_NAME)?.is_loaded(),
            "第三 App 没有重新加载启用模块",
        )?;
        Ok(())
    }
    .await;

    // 返回任何错误前均尝试关闭全部已建立的真实应用；不以 Drop 代替 async shutdown。
    let mut shutdown_error = None;
    for app in apps.iter_mut().rev() {
        if let Err(error) = app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await
        {
            if shutdown_error.is_none() {
                shutdown_error = Some(error);
            }
        }
    }
    let cleanup = root.cleanup();
    outcome?;
    if let Some(error) = shutdown_error {
        return Err(error.into());
    }
    cleanup
}

struct OwnedTempRoot {
    path: PathBuf,
    marker: String,
    cleaned: bool,
}

impl OwnedTempRoot {
    fn new() -> TestResult<Self> {
        let base = std::env::temp_dir().canonicalize()?;
        let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        for attempt in 0..32 {
            let marker = format!("{}-{time}-{attempt}", std::process::id());
            let path = base.join(format!("openrust-module-activation-{marker}"));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    if let Err(error) = fs::write(path.join(".module-activation-owner"), &marker) {
                        let _ = fs::remove_dir(&path);
                        return Err(error.into());
                    }
                    return Ok(Self {
                        path,
                        marker,
                        cleaned: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(std::io::Error::other("无法建立唯一的模块启停测试临时目录").into())
    }

    fn cleanup(&mut self) -> TestResult {
        if self.cleaned {
            return Ok(());
        }
        let metadata = fs::symlink_metadata(&self.path)?;
        ensure(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "临时目录归属检查失败",
        )?;
        let marker_path = self.path.join(".module-activation-owner");
        ensure(
            !fs::symlink_metadata(&marker_path)?.file_type().is_symlink(),
            "临时目录标记不能是符号链接",
        )?;
        ensure(
            fs::read_to_string(marker_path)? == self.marker,
            "临时目录标记不匹配，拒绝清理",
        )?;
        fs::remove_dir_all(&self.path)?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for OwnedTempRoot {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
