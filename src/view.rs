#![doc = include_str!("view/README.md")]

use std::path::{Component, Path};
use std::sync::RwLock;

use axum::response::Html;
use minijinja::{AutoEscape, Environment, ErrorKind, UndefinedBehavior, Value};
use serde::Serialize;

use crate::config::ViewConfig;
use crate::core::{AppBuilder, Error, Plugin, Result};

/// 应用服务端视图插件。
///
/// 从 `[view]` 配置读取模板目录，默认是应用工作目录中的 `views/`。
pub struct ViewPlugin;

/// 加载应用文件系统模板的共享 renderer。
pub struct Views {
    environment: RwLock<Environment<'static>>,
    auto_reload: bool,
}

impl Plugin for ViewPlugin {
    fn name(&self) -> &'static str {
        "view"
    }

    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let config = app.section::<ViewConfig>("view").map_err(Error::from)?;
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
        app.insert(Views {
            environment: RwLock::new(environment),
            auto_reload: config.auto_reload,
        });
        Ok(())
    }
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
