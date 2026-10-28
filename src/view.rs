#![doc = include_str!("view/README.md")]

use std::path::{Component, Path};
use std::sync::{Arc, RwLock};

use axum::response::Html;
use minijinja::{AutoEscape, Environment, ErrorKind, UndefinedBehavior, Value};
use serde::Serialize;

use crate::config::ViewConfig;
use crate::core::{Error, Result};

/// 加载应用文件系统模板的共享 renderer。
pub struct Views {
    environment: RwLock<Environment<'static>>,
    auto_reload: bool,
}

/// 按当前配置向统一容器注册文件模板服务。
///
/// 显式安装 Provider 会在启动时校验目录；生产入口尚未完成切换。
#[derive(Debug, Default, Clone, Copy)]
pub struct ViewServiceProvider;

/// 仅在原始配置来源声明 `[view]` 时安装视图服务。
///
/// 默认应用使用此 Provider 保持视图的可选性；显式安装 [`ViewServiceProvider`]
/// 仍可使用默认 `views/` 目录。
#[derive(Debug, Default, Clone, Copy)]
pub struct OptionalViewServiceProvider;

impl crate::core::ServiceProvider for OptionalViewServiceProvider {
    fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
        if app.source_has_section("view")? {
            crate::core::ServiceProvider::register(&ViewServiceProvider, app)?;
        }
        Ok(())
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::core::BootContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.is_bound::<Views>() {
                let _ = app.resolve::<Views>().await?;
            }
            Ok(())
        })
    }
}

impl crate::core::ServiceProvider for ViewServiceProvider {
    fn register(&self, app: &mut crate::core::RegisterContext<'_>) -> Result<()> {
        use crate::config::repository::SectionSchema;
        app.config_section(
            "view",
            ViewConfig::default(),
            SectionSchema::object([
                ("directory", SectionSchema::String),
                ("auto_reload", SectionSchema::Boolean),
            ]),
            |_| Ok(()),
        )?;
        app.singleton::<Views, _>(|resolver| {
            Box::pin(async move {
                let config = resolver.section::<ViewConfig>("view")?;
                Ok(Arc::new(configured_views(config)?))
            })
        });
        app.require::<Views>();
        Ok(())
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::core::BootContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _ = app.resolve::<Views>().await?;
            Ok(())
        })
    }
}

fn configured_views(config: ViewConfig) -> Result<Views> {
    let directory = if config.directory.is_absolute() {
        config.directory
    } else {
        std::env::current_dir()
                .map_err(|_| {
                    crate::config::ConfigError::Invalid(
                        "view.directory cannot be resolved because the application working directory is unavailable".into(),
                    )
                })?
                .join(config.directory)
    };
    if !directory.is_dir() {
        return Err(crate::config::ConfigError::Invalid(
            "view.directory must name an existing directory".into(),
        )
        .into());
    }
    let directory = directory.canonicalize().map_err(|_| {
        crate::config::ConfigError::Invalid(
            "view.directory cannot be resolved; check its path and permissions".into(),
        )
    })?;
    let mut environment = Environment::new();
    let template_root = directory.clone();
    environment.set_loader(move |name| load_template(&template_root, name));
    environment.set_auto_escape_callback(|name| {
        if Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
        {
            AutoEscape::Html
        } else {
            AutoEscape::None
        }
    });
    environment.set_fuel(Some(100_000));
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    Ok(Views {
        environment: RwLock::new(environment),
        auto_reload: config.auto_reload,
    })
}

impl Views {
    /// 渲染指定模板并返回 `text/html` 响应。
    ///
    /// 模板名相对于 `[view].directory`，例如 `pages/home.html`；路径不得包含绝对路径、
    /// `.` 或 `..` 组件。HTML 文件自动转义，模板语法和渲染错误会转换为框架内部错误。
    pub fn render<T: Serialize>(&self, name: &str, context: &T) -> Result<Html<String>> {
        validate_template_name(name)?;
        if self.auto_reload {
            let mut environment = self.environment.write().map_err(|_| {
                Error::custom("view_renderer_unavailable", "view renderer unavailable")
            })?;
            environment.clear_templates();
        }
        let environment = self
            .environment
            .read()
            .map_err(|_| Error::custom("view_renderer_unavailable", "view renderer unavailable"))?;
        let template = environment.get_template(name).map_err(Error::internal)?;
        let value = Value::from_serialize(context);
        let rendered = template.render(value).map_err(Error::internal)?;
        Ok(Html(rendered))
    }

    pub(crate) fn validate_template(&self, name: &str) -> Result<()> {
        validate_template_name(name)?;
        self.environment
            .read()
            .map_err(|_| Error::custom("view_renderer_unavailable", "view renderer unavailable"))?
            .get_template(name)
            .map(|_| ())
            .map_err(Error::internal)
    }
}

fn validate_template_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    if name.is_empty()
        || path.is_absolute()
        || !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().contains('\\')
        })
    {
        return Err(Error::custom(
            "invalid_view_name",
            "view name must be a relative template path",
        ));
    }
    Ok(())
}

fn load_template(
    directory: &Path,
    name: &str,
) -> std::result::Result<Option<String>, minijinja::Error> {
    if validate_template_name(name).is_err() {
        return Ok(None);
    }
    let path = directory.join(name);
    let canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(minijinja::Error::new(
                ErrorKind::InvalidOperation,
                "could not read template",
            )
            .with_source(error));
        }
    };
    if !canonical.starts_with(directory) {
        return Ok(None);
    }
    match std::fs::read_to_string(canonical) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(minijinja::Error::new(
            ErrorKind::InvalidOperation,
            "could not read template",
        )
        .with_source(error)),
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::core::{BootContext, BoxFuture, ProviderBuilder, ServiceProvider};
    use figment::{Figment, providers::Serialized};
    use serde_json::json;

    struct Overlay(std::path::PathBuf);
    impl ServiceProvider for Overlay {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { app.overlay("view", json!({"directory": self.0})) })
        }
    }

    #[tokio::test]
    async fn provider_reads_overlay_directory_and_renders_escaped_template() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        std::fs::write(directory.path().join("index.html"), "Hello {{ name }}")
            .map_err(Error::internal)?;
        let mut providers = ProviderBuilder::new();
        providers
            .provider(Overlay(directory.path().to_path_buf()))
            .provider(ViewServiceProvider);
        let runtime = providers.start(Figment::new()).await?;
        let renderer = runtime.container().resolve::<Views>().await?;
        assert_eq!(
            renderer.render("index.html", &json!({"name":"<user>"}))?.0,
            "Hello &lt;user&gt;"
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    #[tokio::test]
    async fn provider_rejects_explicit_invalid_directory() {
        let mut providers = ProviderBuilder::new();
        providers.provider(ViewServiceProvider);
        let sources = Figment::from(Serialized::defaults(
            json!({"view":{"directory":"/nonexistent-openrust-view-test"}}),
        ));
        assert!(providers.start(sources).await.is_err());
    }

    #[tokio::test]
    async fn optional_provider_skips_missing_source_but_rejects_bad_explicit_directory()
    -> Result<()> {
        let mut providers = ProviderBuilder::new();
        providers.provider(OptionalViewServiceProvider);
        let runtime = providers.start(Figment::new()).await?;
        assert!(!runtime.container().contains::<Views>());
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await?;

        let mut providers = ProviderBuilder::new();
        providers.provider(OptionalViewServiceProvider);
        let source = Figment::from(Serialized::defaults(json!({
            "view": {"directory": "/nonexistent-openrust-view-test"}
        })));
        assert!(providers.start(source).await.is_err());
        Ok(())
    }
}
