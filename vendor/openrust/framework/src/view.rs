#![doc = include_str!("view/README.md")]

mod errors;
#[cfg(feature = "session")]
pub(crate) use errors::PreparedPersistence;
pub(crate) use errors::RequestViewErrors;

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::response::Html;
use minijinja::{AutoEscape, Environment, ErrorKind, UndefinedBehavior, Value, value::ValueKind};
use serde::Serialize;

use crate::config::ViewConfig;
use crate::core::{Error, Result};

/// 加载应用文件系统模板的共享 renderer。
pub struct Views {
    environment: RwLock<Environment<'static>>,
    namespaces: Arc<RwLock<HashMap<String, ViewNamespace>>>,
    auto_reload: bool,
}

#[derive(Clone)]
struct ViewNamespace {
    source: PathBuf,
    published: PathBuf,
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

impl crate::support::ServiceProvider for OptionalViewServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.source_has_section("view")? {
                crate::support::ServiceProvider::register(&ViewServiceProvider, app).await?;
            }
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.is_bound::<Views>() {
                let _ = app.resolve::<Views>().await?;
            }
            Ok(())
        })
    }
}

impl crate::support::ServiceProvider for ViewServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
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
            })
            .await?;
            app.require::<Views>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
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
    let namespaces = Arc::new(RwLock::new(HashMap::<String, ViewNamespace>::new()));
    let loader_namespaces = Arc::clone(&namespaces);
    environment.set_loader(move |name| {
        if let Some((namespace, template)) = name.split_once("::") {
            let source = loader_namespaces
                .read()
                .map_err(|_| {
                    minijinja::Error::new(ErrorKind::InvalidOperation, "view namespace unavailable")
                })?
                .get(namespace)
                .cloned();
            if let Some(source) = source {
                return load_template(&source.published, template)?.map_or_else(
                    || load_template(&source.source, template),
                    |content| Ok(Some(content)),
                );
            }
            return Ok(None);
        }
        load_template(&template_root, name)
    });
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
        namespaces,
        auto_reload: config.auto_reload,
    })
}

impl Views {
    /// 登记模块视图命名空间；已发布路径优先，缺失时回退模块源模板。
    ///
    /// 应在模块 Provider 的 `boot` 调用，禁用模块不会登记；重复名称在 Ready 前失败。
    pub fn add_namespace(
        &self,
        name: &str,
        source_dir: impl AsRef<Path>,
        published_dir: impl AsRef<Path>,
    ) -> Result<()> {
        if !valid_namespace(name) {
            return Err(Error::custom("view_namespace_invalid", "视图命名空间无效"));
        }
        let source = source_dir.as_ref().canonicalize().map_err(|_| {
            Error::custom("view_namespace_source_invalid", "模块视图源目录无法读取")
        })?;
        if !source.is_dir() {
            return Err(Error::custom(
                "view_namespace_source_invalid",
                "模块视图源路径不是目录",
            ));
        }
        let published = published_dir.as_ref().to_path_buf();
        if published.exists() && !published.is_dir() {
            return Err(Error::custom(
                "view_namespace_published_invalid",
                "已发布视图路径不是目录",
            ));
        }
        let mut environment = self
            .environment
            .write()
            .map_err(|_| Error::custom("view_renderer_unavailable", "视图渲染器不可用"))?;
        let mut namespaces = self
            .namespaces
            .write()
            .map_err(|_| Error::custom("view_namespace_unavailable", "视图命名空间不可用"))?;
        if namespaces.contains_key(name) {
            return Err(Error::custom("view_namespace_conflict", "视图命名空间重复"));
        }
        namespaces.insert(name.to_owned(), ViewNamespace { source, published });
        environment.clear_templates();
        Ok(())
    }

    /// 渲染指定模板并返回 `text/html` 响应。
    ///
    /// 模板名相对于 `[view].directory`，例如 `pages/home.html`；路径不得包含绝对路径、
    /// `.` 或 `..` 组件。HTML 文件自动转义，模板语法和渲染错误会转换为框架内部错误。
    pub fn render<T: Serialize>(&self, name: &str, context: &T) -> Result<Html<String>> {
        self.render_value(name, Value::from_serialize(context))
    }

    /// 使用本请求在 Session 启动后取得的错误包渲染模板。
    ///
    /// `context` 必须是 map 或 struct；显式 `errors` 键优先于请求共享值。
    /// Session 未启用时共享值为空包。模板方法会调用原消息包契约，保留自定义实现。
    pub fn render_for_request<T: Serialize>(
        &self,
        name: &str,
        request: &crate::http::RequestInfo,
        context: &T,
    ) -> Result<Html<String>> {
        let explicit = Value::from_serialize(context);
        if explicit.kind() != ValueKind::Map {
            return Err(Error::custom(
                "view_context_invalid",
                "视图上下文必须是映射或结构体",
            ));
        }
        let mut merged = BTreeMap::from([(
            "errors".to_owned(),
            Value::from_object(errors::ViewErrors::new(request.view_errors())),
        )]);
        for key in explicit.try_iter().map_err(Error::internal)? {
            let Some(key_name) = key.as_str() else {
                return Err(Error::custom(
                    "view_context_invalid",
                    "视图上下文键必须是字符串",
                ));
            };
            let value = explicit.get_item(&key).map_err(Error::internal)?;
            merged.insert(key_name.to_owned(), value);
        }
        self.render_value(name, Value::from_iter(merged))
    }

    fn render_value(&self, name: &str, value: Value) -> Result<Html<String>> {
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
    let path_name = if let Some((namespace, template)) = name.split_once("::") {
        if !valid_namespace(namespace) {
            return Err(Error::custom(
                "invalid_view_name",
                "view namespace is invalid",
            ));
        }
        template
    } else {
        name
    };
    let path = Path::new(path_name);
    if path_name.is_empty()
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

fn valid_namespace(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn load_template(
    directory: &Path,
    name: &str,
) -> std::result::Result<Option<String>, minijinja::Error> {
    if validate_template_name(name).is_err() || name.contains("::") {
        return Ok(None);
    }
    let directory = match directory.canonicalize() {
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
    if !canonical.starts_with(&directory) {
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
    use crate::foundation::BootContext;
    use crate::foundation::ProviderBuilder;
    use crate::support::{BoxFuture, ServiceProvider};
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

    #[test]
    fn module_namespace_prefers_published_template_and_falls_back_to_source() -> Result<()> {
        let temporary = tempfile::tempdir().map_err(Error::internal)?;
        let root = temporary.path();
        let application = root.join("application");
        let source = root.join("module");
        let published = root.join("published");
        std::fs::create_dir(&application).map_err(Error::internal)?;
        std::fs::create_dir(&source).map_err(Error::internal)?;
        std::fs::create_dir(&published).map_err(Error::internal)?;
        std::fs::write(source.join("card.html"), "source {{ name }}").map_err(Error::internal)?;
        let views = configured_views(ViewConfig {
            directory: application,
            auto_reload: true,
        })?;
        views.add_namespace("blog", &source, &published)?;
        assert_eq!(
            views.render("blog::card.html", &json!({"name":"<x>"}))?.0,
            "source &lt;x&gt;"
        );
        std::fs::write(published.join("card.html"), "published {{ name }}")
            .map_err(Error::internal)?;
        assert_eq!(
            views.render("blog::card.html", &json!({"name":"<x>"}))?.0,
            "published &lt;x&gt;"
        );
        assert_eq!(
            views
                .add_namespace("blog", &source, &published)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("view_namespace_conflict")
        );
        assert!(views.render("other::card.html", &json!({})).is_err());
        assert!(views.render("blog::../card.html", &json!({})).is_err());
        Ok(())
    }
}
