//! 已编译 Rust 配置函数的显式清单与同次环境快照。

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use super::sources::EnvironmentSnapshot;
use super::{ConfigError, ConfigSources};

/// 代码配置函数读取的同次启动环境；调试输出不包含变量或值。
#[derive(Clone)]
pub struct ConfigContext {
    environment: EnvironmentSnapshot,
}

impl fmt::Debug for ConfigContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ConfigContext { .. }")
    }
}

impl ConfigContext {
    pub(crate) fn new(environment: EnvironmentSnapshot) -> Self {
        Self { environment }
    }

    /// 读取进程优先、已选 dotenv 补充的变量；缺失或非 UTF-8 返回 `None`。
    pub fn env(&self, key: &str) -> Option<String> {
        self.environment.get(key)
    }

    /// 仅变量缺失时使用默认值，显式空字符串予以保留。
    pub fn env_or(&self, key: &str, default: &str) -> String {
        self.env(key).unwrap_or_else(|| default.to_owned())
    }

    /// 严格读取布尔部署值；无效输入只报告变量名，不显示值。
    pub fn bool_or(&self, key: &str, default: bool) -> Result<bool, ConfigError> {
        let Some(value) = self.env(key) else {
            return Ok(default);
        };
        parse_bool(key, &value)
    }
}

pub(crate) fn parse_bool(key: &str, value: &str) -> Result<bool, ConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Ok(true),
        "false" | "0" | "off" | "no" => Ok(false),
        _ => Err(ConfigError::Invalid(format!(
            "{key}: 布尔配置值无效（源值未公开）"
        ))),
    }
}

pub(crate) type TypedValue = Arc<dyn Any + Send + Sync>;

type Evaluator<T> = Box<dyn Fn(&ConfigContext) -> Result<T, ConfigError> + Send + Sync>;

enum Entry {
    Section {
        name: String,
        evaluate: Evaluator<Value>,
    },
    Typed {
        key: String,
        evaluate: Evaluator<TypedValue>,
    },
}

pub(crate) enum EvaluatedEntry {
    Section(String, Value),
    Typed(String, TypedValue),
}

/// 应用和模块显式声明的已编译配置函数，按登记顺序求值。
#[derive(Default)]
pub struct ConfigManifest {
    entries: Vec<Entry>,
    include_toml: bool,
}

impl ConfigManifest {
    /// 建立空清单；无需代码配置的应用可使用此默认值。
    pub fn new() -> Self {
        Self::default()
    }

    /// 显式启用现有 TOML 目录来源，作为代码区段之后、环境覆盖之前的可选层。
    #[must_use]
    pub fn with_toml(mut self) -> Self {
        self.include_toml = true;
        self
    }

    pub(crate) fn includes_toml(&self) -> bool {
        self.include_toml
    }

    /// 登记一个可序列化配置区段；环境文件加载后、Provider 注册前调用函数。
    #[must_use]
    pub fn section<T, F>(mut self, name: impl Into<String>, evaluate: F) -> Self
    where
        T: Serialize,
        F: Fn(&ConfigContext) -> Result<T, ConfigError> + Send + Sync + 'static,
    {
        self.entries.push(Entry::Section {
            name: name.into(),
            evaluate: Box::new(move |context| {
                let value = evaluate(context).map_err(|_| {
                    ConfigError::Invalid("代码配置求值失败（源值未公开）".to_owned())
                })?;
                serde_json::to_value(value).map_err(|_| {
                    ConfigError::Invalid("代码配置无法序列化（源值未公开）".to_owned())
                })
            }),
        });
        self
    }

    /// 登记运行期 Rust 类型或工厂闭包；此类清单拒绝配置缓存。
    #[must_use]
    pub fn typed<T, F>(mut self, key: impl Into<String>, evaluate: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ConfigContext) -> Result<T, ConfigError> + Send + Sync + 'static,
    {
        self.entries.push(Entry::Typed {
            key: key.into(),
            evaluate: Box::new(move |context| {
                evaluate(context)
                    .map(|value| Arc::new(value) as TypedValue)
                    .map_err(|_| ConfigError::Invalid("代码配置求值失败（源值未公开）".to_owned()))
            }),
        });
        self
    }

    /// 是否包含无法证明能由配置缓存恢复的 Rust 值。
    pub fn has_typed(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry, Entry::Typed { .. }))
    }

    pub(crate) fn apply(
        &self,
        sources: &mut ConfigSources,
        context: &ConfigContext,
    ) -> Result<(), ConfigError> {
        for entry in self.evaluate(context)? {
            match entry {
                EvaluatedEntry::Section(name, value) => sources.merge_code_section(&name, value)?,
                EvaluatedEntry::Typed(key, value) => sources.insert_typed(&key, value)?,
            }
        }
        Ok(())
    }

    pub(crate) fn evaluate(
        &self,
        context: &ConfigContext,
    ) -> Result<Vec<EvaluatedEntry>, ConfigError> {
        let mut names = Vec::<&str>::new();
        let mut evaluated = Vec::new();
        for entry in &self.entries {
            let name = match entry {
                Entry::Section { name, .. } => name,
                Entry::Typed { key, .. } => key,
            };
            if name.is_empty()
                || name.split('.').any(str::is_empty)
                || names.contains(&name.as_str())
            {
                return Err(ConfigError::Invalid(
                    "代码配置路径重复或无效（源值未公开）".to_owned(),
                ));
            }
            names.push(name);
            match entry {
                Entry::Section { name, evaluate } => {
                    evaluated.push(EvaluatedEntry::Section(name.clone(), evaluate(context)?));
                }
                Entry::Typed { key, evaluate } => {
                    evaluated.push(EvaluatedEntry::Typed(key.clone(), evaluate(context)?));
                }
            }
        }
        Ok(evaluated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::repository::ConfigRepository;
    use crate::config::{ConfigLoader, Environment};

    #[test]
    fn code_section_reads_selected_dotenv_and_typed_value_shares_repository() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(
            #[allow(
                clippy::result_large_err,
                reason = "Figment Jail 回调要求原生 figment::Error"
            )]
            |jail| {
                jail.clear_env();
                jail.create_file(".env", "GREETING_TOKEN=from-dotenv\nAPP_DEBUG=on\n")?;
                jail.create_dir("config")?;
                jail.create_file("config/app.toml", "name = 'legacy-toml'\n")?;
                let environment = EnvironmentSnapshot::capture()
                    .with_dotenv_at(std::path::Path::new(".env"))
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let loader =
                    ConfigLoader::from_snapshot(Environment::Production, "APP_", environment)
                        .map_err(|error| figment::Error::from(error.to_string()))?;
                let manifest = ConfigManifest::new()
                    .section("app", |ctx| {
                        Ok(serde_json::json!({
                            "name": ctx.env_or("GREETING_TOKEN", "missing"),
                            "debug": ctx.bool_or("APP_DEBUG", false)?
                        }))
                    })
                    .typed("factory", |_| Ok(42_u32));
                let sources = loader
                    .code_sources(&manifest)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let repository = ConfigRepository::new(sources);
                assert_eq!(
                    repository
                        .string("app.name")
                        .map_err(|error| figment::Error::from(error.to_string()))?,
                    "from-dotenv"
                );
                assert_eq!(
                    repository
                        .typed::<u32>("factory")
                        .map_err(|error| figment::Error::from(error.to_string()))?
                        .as_deref(),
                    Some(&42)
                );
                assert!(
                    repository
                        .all()
                        .map_err(|error| figment::Error::from(error.to_string()))?
                        .get("factory")
                        .is_none()
                );
                assert!(
                    repository
                        .set("factory.child", serde_json::json!(1))
                        .is_err()
                );
                assert_eq!(
                    repository
                        .typed::<u32>("factory")
                        .map_err(|error| figment::Error::from(error.to_string()))?
                        .as_deref(),
                    Some(&42)
                );
                let sibling_manifest = ConfigManifest::new()
                    .section("example", |_| Ok(serde_json::json!({"greeting": "hello"})))
                    .typed("example.runtime_policy", |_| Ok(17_u32));
                let sibling_sources = loader
                    .code_sources(&sibling_manifest)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                let siblings = ConfigRepository::new(sibling_sources);
                siblings
                    .set("example", serde_json::json!({"greeting": "updated"}))
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    siblings
                        .typed::<u32>("example.runtime_policy")
                        .map_err(|error| figment::Error::from(error.to_string()))?
                        .as_deref(),
                    Some(&17)
                );
                assert!(siblings.set("example", serde_json::json!(null)).is_err());
                assert!(
                    siblings
                        .set("example.runtime_policy", serde_json::json!(42))
                        .is_err()
                );
                let opt_in = ConfigManifest::new()
                    .section("app", |_| Ok(serde_json::json!({"name": "from-code"})))
                    .with_toml();
                let toml_sources = loader
                    .code_sources(&opt_in)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    toml_sources.figment().extract_inner::<String>("app.name")?,
                    "legacy-toml"
                );
                Ok(())
            },
        );
    }

    #[test]
    fn typed_manifest_refuses_existing_serialized_cache_without_loading_dotenv() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(
            #[allow(
                clippy::result_large_err,
                reason = "Figment Jail 回调要求原生 figment::Error"
            )]
            |jail| {
                jail.clear_env();
                jail.create_dir("bootstrap")?;
                jail.create_dir("bootstrap/cache")?;
                jail.create_file(".env", "broken dotenv line")?;
                let cached = serde_json::to_string(&crate::config::FrameworkConfig::default())
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                jail.create_file("bootstrap/cache/config.json", &cached)?;
                let manifest = ConfigManifest::new().typed("factory", |_| Ok(42_u32));
                let error = ConfigLoader::resolve_application_sources(
                    std::path::Path::new("."),
                    "APP_",
                    None,
                    Some(&manifest),
                )
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
                assert!(error.contains("Rust 类型"));
                assert!(!error.contains("dotenv"));
                Ok(())
            },
        );
    }
    #[test]
    fn explicit_code_directory_requires_a_directory_but_valid_cache_skips_sources() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(
            #[allow(
                clippy::result_large_err,
                reason = "Figment Jail 回调要求原生 figment::Error"
            )]
            |jail| {
                jail.clear_env();
                let manifest = ConfigManifest::new()
                    .section("app", |_| Ok(serde_json::json!({"name":"from-code"})));
                jail.set_env("APP_CONFIG_DIR", "PRIVATE_MISSING_DIRECTORY_SENTINEL");
                let missing = ConfigLoader::application_code_sources(".", "APP_", None, &manifest)
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
                assert!(missing.contains("APP_CONFIG_DIR"));
                assert!(!missing.contains("PRIVATE_MISSING_DIRECTORY_SENTINEL"));
                jail.create_file("not-a-directory", "private")?;
                jail.set_env("APP_CONFIG_DIR", "not-a-directory");
                assert!(
                    ConfigLoader::application_code_sources(".", "APP_", None, &manifest).is_err()
                );
                jail.create_dir("explicit-config")?;
                jail.set_env("APP_CONFIG_DIR", "explicit-config");
                let valid = ConfigLoader::application_code_sources(".", "APP_", None, &manifest)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    valid.figment().extract_inner::<String>("app.name")?,
                    "from-code"
                );
                jail.create_dir("bootstrap")?;
                jail.create_dir("bootstrap/cache")?;
                jail.create_file(
                    "bootstrap/cache/config.json",
                    r#"{"app":{"name":"from-cache"}}"#,
                )?;
                jail.create_file(".env", "broken dotenv line")?;
                jail.set_env("APP_CONFIG_DIR", "PRIVATE_MISSING_DIRECTORY_SENTINEL");
                let cached = ConfigLoader::application_code_sources(".", "APP_", None, &manifest)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(
                    cached.figment().extract_inner::<String>("app.name")?,
                    "from-cache"
                );
                assert!(ConfigRepository::new(cached).is_cached());
                Ok(())
            },
        );
    }
}
