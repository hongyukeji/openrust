//! 已编译 Rust 配置函数的显式清单与同次环境快照。

use std::any::Any;
use std::fmt;
use std::sync::{Arc, Weak};
use std::thread::{self, ThreadId};

use serde::de::DeserializeOwned;

use serde::Serialize;
use serde_json::Value;

use super::repository::read_dot_value;
use super::sources::{EnvironmentSnapshot, paths_overlap};
use super::{ConfigError, ConfigSources};

/// 代码配置函数读取的同次启动环境；调试输出不包含变量或值。
#[derive(Clone)]
pub struct ConfigContext {
    environment: EnvironmentSnapshot,
    scope: Weak<ReadScope>,
    origin: Option<String>,
}

impl fmt::Debug for ConfigContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ConfigContext { .. }")
    }
}

impl ConfigContext {
    pub(crate) fn new(environment: EnvironmentSnapshot) -> Self {
        Self {
            environment,
            scope: Weak::new(),
            origin: None,
        }
    }

    /// 读取已完成配置的同次最终来源；缺失返回 `None`，显式 null 保留类型语义。
    ///
    /// 仅同步配置函数内可调用；自身、前向、typed 路径及类型不符返回脱敏错误。
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, ConfigError> {
        let state = self
            .scope
            .upgrade()
            .ok_or_else(|| self.dependency_error(DependencyKind::Expired, None, None))?;
        if state.thread != thread::current().id() {
            return Err(self.dependency_error(DependencyKind::Thread, None, None));
        }
        if key.is_empty() || key.split('.').any(str::is_empty) {
            return Err(self.dependency_error(DependencyKind::Path, Some(&state), None));
        }
        if paths_overlap(&state.names[state.active], key) {
            return Err(self.dependency_error(DependencyKind::SelfRead, Some(&state), Some(key)));
        }
        if state
            .names
            .iter()
            .skip(state.active + 1)
            .any(|name| paths_overlap(name, key))
        {
            return Err(self.dependency_error(DependencyKind::Forward, Some(&state), Some(key)));
        }
        if state.view.typed.iter().any(|name| paths_overlap(name, key)) {
            return Err(self.dependency_error(DependencyKind::Typed, Some(&state), Some(key)));
        }
        let value = state
            .view
            .raw
            .as_object()
            .and_then(|values| values.get(key))
            .or_else(|| read_dot_value(&state.view.raw, key))
            .cloned();
        value
            .map(|value| {
                serde_json::from_value(value).map_err(|_| {
                    self.dependency_error(DependencyKind::Type, Some(&state), Some(key))
                })
            })
            .transpose()
    }

    /// 读取必需的可序列化配置；缺失与存在但无法反序列化分别报错。
    pub fn require<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError> {
        self.get(key)?.ok_or_else(|| {
            self.dependency_error(
                DependencyKind::Missing,
                self.scope.upgrade().as_deref(),
                Some(key),
            )
        })
    }

    fn dependency_error(
        &self,
        kind: DependencyKind,
        scope: Option<&ReadScope>,
        key: Option<&str>,
    ) -> ConfigError {
        ConfigError::Dependency(ConfigDependencyError {
            kind,
            origin: self.origin.clone(),
            path: scope.and_then(|scope| key.and_then(|key| scope.safe_path(key))),
        })
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

    /// 按 Laravel env 保留 null，并按 PHP 布尔转换读取部署值；仅缺失时使用默认值。
    ///
    /// 支持保留字及其括号形式。其他字符串仅空串或 `0` 为 false，成对引号先去除。
    pub fn nullable_bool_or(&self, key: &str, default: Option<bool>) -> Option<bool> {
        self.env(key)
            .map_or(default, |value| nullable_env_bool(&value))
    }
}

// 固定 Illuminate\Support\Env::getOption 的保留字转换，随后执行 PHP 的布尔转换。
pub(super) fn nullable_env_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "null" | "(null)" => None,
        "true" | "(true)" => Some(true),
        "false" | "(false)" | "empty" | "(empty)" => Some(false),
        _ => {
            let quoted = (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''));
            let value = if value.len() > 1 && quoted {
                &value[1..value.len() - 1]
            } else {
                value
            };
            Some(!value.is_empty() && value != "0")
        }
    }
}

/// 配置函数依赖读取的安全错误；私有字段阻止用户构造可绕过脱敏的消息。
#[derive(Debug)]
pub struct ConfigDependencyError {
    kind: DependencyKind,
    origin: Option<String>,
    path: Option<String>,
}

#[derive(Debug)]
enum DependencyKind {
    Expired,
    Thread,
    Path,
    SelfRead,
    Forward,
    Typed,
    Type,
    Missing,
}

impl fmt::Display for ConfigDependencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            DependencyKind::Expired => "代码配置读取作用域已失效",
            DependencyKind::Thread => "代码配置读取不允许跨线程",
            DependencyKind::Path => "代码配置读取路径无效",
            DependencyKind::SelfRead => "代码配置依赖自身（自循环）",
            DependencyKind::Forward => "代码配置依赖尚未执行的声明（前向依赖）",
            DependencyKind::Typed => "代码配置依赖读取仅支持可序列化值",
            DependencyKind::Type => "代码配置依赖值类型不符（源值未公开）",
            DependencyKind::Missing => "代码配置必需依赖缺失",
        })?;
        if let Some(origin) = &self.origin {
            write!(formatter, "（声明 {origin}）")?;
        }
        if let Some(path) = &self.path {
            write!(formatter, "（依赖 {path}）")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigDependencyError {}

fn redact_user_error(error: ConfigError) -> ConfigError {
    match error {
        ConfigError::Dependency(_) => error,
        _ => ConfigError::Invalid("代码配置求值失败（源值未公开）".to_owned()),
    }
}

pub(crate) struct ReadView {
    pub(crate) raw: Value,
    pub(crate) typed: Vec<String>,
}

struct ReadScope {
    thread: ThreadId,
    names: Arc<Vec<String>>,
    active: usize,
    view: ReadView,
}

impl ReadScope {
    fn safe_path(&self, key: &str) -> Option<String> {
        if !safe_diagnostic_path(key) {
            return None;
        }
        // 只回显真实声明或同次来源已有路径，不把任意调用参数当可信路径。
        let mut known = self
            .names
            .iter()
            .chain(&self.view.typed)
            .filter(|name| safe_diagnostic_path(name) && paths_overlap(name, key))
            .max_by_key(|name| name.len())
            .cloned();
        for (end, _) in key
            .char_indices()
            .filter(|(_, character)| *character == '.')
            .chain(std::iter::once((key.len(), '.')))
        {
            let prefix = &key[..end];
            let exists = self
                .view
                .raw
                .as_object()
                .is_some_and(|values| values.contains_key(prefix))
                || read_dot_value(&self.view.raw, prefix).is_some();
            if exists && known.as_ref().is_none_or(|old| prefix.len() > old.len()) {
                known = Some(prefix.to_owned());
            }
        }
        known
    }
}

fn safe_diagnostic_path(path: &str) -> bool {
    path.len() <= 128
        && path.split('.').count() <= 8
        && path.split('.').all(|part| {
            let mut bytes = part.bytes();
            matches!(bytes.next(), Some(b'a'..=b'z' | b'_'))
                && part.len() <= 32
                && bytes
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
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
                let value = evaluate(context).map_err(redact_user_error)?;
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
                    .map_err(redact_user_error)
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
        final_view: impl Fn(&ConfigSources) -> Result<ConfigSources, ConfigError>,
    ) -> Result<(), ConfigError> {
        let initial = final_view(sources)?.read_view()?;
        self.evaluate_with(context, initial, |entry| {
            match entry {
                EvaluatedEntry::Section(name, value) => sources.merge_code_section(&name, value)?,
                EvaluatedEntry::Typed(key, value) => sources.insert_typed(&key, value)?,
            }
            sources.validate_typed_paths()?;
            let view = final_view(sources)?;
            view.validate_typed_paths()?;
            view.read_view()
        })?;
        Ok(())
    }

    pub(crate) fn typed_keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Typed { key, .. } => Some(key.as_str()),
            Entry::Section { .. } => None,
        })
    }

    pub(crate) fn validate(&self) -> Result<Vec<String>, ConfigError> {
        let mut names = Vec::new();
        let mut typed = Vec::new();
        for entry in &self.entries {
            let name = match entry {
                Entry::Section { name, .. } => name,
                Entry::Typed { key, .. } => key,
            };
            if name.is_empty() || name.split('.').any(str::is_empty) || names.contains(name) {
                return Err(ConfigError::Invalid(
                    "代码配置路径重复或无效（源值未公开）".to_owned(),
                ));
            }
            if matches!(entry, Entry::Typed { .. }) {
                if typed
                    .iter()
                    .any(|existing: &&String| paths_overlap(existing, name))
                {
                    return Err(ConfigError::Invalid(
                        "代码配置类型路径重复（源值未公开）".to_owned(),
                    ));
                }
                typed.push(name);
            }
            names.push(name.clone());
        }
        Ok(names)
    }

    pub(crate) fn evaluate_with(
        &self,
        context: &ConfigContext,
        initial: ReadView,
        mut apply: impl FnMut(EvaluatedEntry) -> Result<ReadView, ConfigError>,
    ) -> Result<usize, ConfigError> {
        let names = Arc::new(self.validate()?);
        let mut view = initial;
        for (index, entry) in self.entries.iter().enumerate() {
            let scope = Arc::new(ReadScope {
                thread: thread::current().id(),
                names: names.clone(),
                active: index,
                view,
            });
            let context = ConfigContext {
                environment: context.environment.clone(),
                scope: Arc::downgrade(&scope),
                origin: safe_diagnostic_path(&names[index]).then(|| names[index].clone()),
            };
            // 只读作用域不持锁执行用户函数；每次函数返回即使旧 Weak 失效。
            let evaluated = match entry {
                Entry::Section { name, evaluate } => {
                    EvaluatedEntry::Section(name.clone(), evaluate(&context)?)
                }
                Entry::Typed { key, evaluate } => {
                    EvaluatedEntry::Typed(key.clone(), evaluate(&context)?)
                }
            };
            drop(scope);
            view = apply(evaluated)?;
        }
        Ok(self.entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::repository::ConfigRepository;
    use crate::config::{ConfigLoader, Environment};
    use std::sync::Mutex;

    fn evaluate_test_manifest(manifest: &ConfigManifest) -> Result<(), ConfigError> {
        let mut raw = serde_json::json!({});
        let mut typed = Vec::new();
        manifest.evaluate_with(
            &ConfigContext::new(EnvironmentSnapshot::capture()),
            ReadView {
                raw: raw.clone(),
                typed: typed.clone(),
            },
            |entry| {
                match entry {
                    EvaluatedEntry::Section(key, value) => {
                        super::super::repository::set_dot_value(&mut raw, &key, value);
                    }
                    EvaluatedEntry::Typed(key, _) => typed.push(key),
                }
                Ok(ReadView {
                    raw: raw.clone(),
                    typed: typed.clone(),
                })
            },
        )?;
        Ok(())
    }

    #[test]
    fn dependency_reads_distinguish_missing_null_and_type_without_values() -> Result<(), ConfigError>
    {
        let manifest = ConfigManifest::new()
            .section("seed", |_| {
                Ok(serde_json::json!({"null":null,"secret":"PRIVATE_VALUE","number":42}))
            })
            .section("consumer", |context| {
                assert_eq!(context.get::<Value>("seed.missing")?, None);
                assert_eq!(context.get::<Value>("seed.null")?, Some(Value::Null));
                assert_eq!(context.get::<Option<String>>("seed.null")?, Some(None));
                assert_eq!(context.require::<Value>("seed.null")?, Value::Null);
                assert_eq!(context.require::<Option<String>>("seed.null")?, None);
                assert_eq!(context.require::<u16>("seed.number")?, 42);
                let missing = context
                    .require::<Value>("seed.missing")
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_default();
                assert!(missing.contains("缺失"));
                let invalid = context
                    .get::<u16>("seed.secret")
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_default();
                assert!(invalid.contains("类型不符"));
                assert!(!invalid.contains("PRIVATE_VALUE"));
                assert!(context.get::<String>("seed.null").is_err());
                Ok(serde_json::json!({"read":true}))
            });
        evaluate_test_manifest(&manifest)
    }

    #[test]
    fn dependency_scope_expires_and_rejects_cross_thread_while_env_clone_remains()
    -> Result<(), ConfigError> {
        let saved = Arc::new(Mutex::new(None));
        let capture = saved.clone();
        let manifest = ConfigManifest::new()
            .section("seed", |_| Ok(serde_json::json!({"number":42})))
            .section("consumer", move |context| {
                let copied = context.clone();
                let error = std::thread::spawn(move || copied.get::<u16>("seed.number"))
                    .join()
                    .map_err(|_| ConfigError::Invalid("线程测试失败".into()))?
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
                assert!(error.contains("跨线程"));
                assert!(error.contains("声明 consumer"));
                assert!(!error.contains("seed.number"));
                *capture
                    .lock()
                    .map_err(|_| ConfigError::Invalid("测试锁失败".into()))? =
                    Some(context.clone());
                Ok(serde_json::json!({}))
            });
        let inspect = saved.clone();
        let manifest = manifest.section("later", move |_| {
            let copied = inspect
                .lock()
                .map_err(|_| ConfigError::Invalid("测试锁失败".into()))?;
            let copied = copied
                .as_ref()
                .ok_or_else(|| ConfigError::Invalid("缺少测试上下文".into()))?;
            assert!(copied.get::<u16>("seed.number").is_err());
            Ok(serde_json::json!({}))
        });
        evaluate_test_manifest(&manifest)?;
        let copied = saved
            .lock()
            .map_err(|_| ConfigError::Invalid("测试锁失败".into()))?
            .take()
            .ok_or_else(|| ConfigError::Invalid("缺少测试上下文".into()))?;
        let expected = copied.environment.get("PATH");
        assert_eq!(copied.env("PATH"), expected);
        let error = copied
            .get::<u16>("seed.number")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("失效"));
        assert!(error.contains("声明 consumer"));
        assert!(!error.contains("seed.number"));
        Ok(())
    }

    #[test]
    fn invalid_manifest_is_rejected_before_any_function_executes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        for case in 0..3 {
            let count = Arc::new(AtomicUsize::new(0));
            let called = count.clone();
            let manifest = ConfigManifest::new().section("first", move |_| {
                called.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({}))
            });
            let manifest = match case {
                0 => manifest.section("bad..path", |_| Ok(serde_json::json!({}))),
                1 => manifest.section("first", |_| Ok(serde_json::json!({}))),
                _ => manifest
                    .typed("runtime", |_| Ok(1_u8))
                    .typed("runtime.child", |_| Ok(2_u8)),
            };
            assert!(evaluate_test_manifest(&manifest).is_err());
            assert_eq!(count.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn self_forward_and_first_edge_of_a_cycle_are_explicit_errors() {
        for (read, later, expected) in [
            ("first.value", "later", "自循环"),
            ("first", "later", "自循环"),
            ("later.value", "later", "前向依赖"),
        ] {
            let manifest = ConfigManifest::new()
                .section("first", move |context| {
                    let value = context.require::<Value>(read)?;
                    Ok(serde_json::json!({"value":value}))
                })
                .section(later, |context| {
                    let value = context.require::<Value>("first.value")?;
                    Ok(serde_json::json!({"value":value}))
                });
            let error = evaluate_test_manifest(&manifest)
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn dependency_diagnostics_locate_known_paths_and_redact_untrusted_keys()
    -> Result<(), ConfigError> {
        let manifest = ConfigManifest::new()
            .section("server", |_| {
                Ok(serde_json::json!({"port":"PRIVATE_SOURCE_VALUE"}))
            })
            .section("app", |context| {
                let error = context
                    .require::<u16>("server.port")
                    .err()
                    .ok_or_else(|| ConfigError::Invalid("缺少预期类型错误".into()))?;
                let rendered = format!("{error}; {error:?}");
                assert!(rendered.contains("声明 app"));
                assert!(rendered.contains("依赖 server.port"));
                assert!(!rendered.contains("PRIVATE_SOURCE_VALUE"));
                for key in [
                    "PRIVATE_TOKEN_AS_KEY",
                    "private_secret_as_key",
                    "server.private_secret_as_key",
                ] {
                    let error = context
                        .require::<Value>(key)
                        .err()
                        .ok_or_else(|| ConfigError::Invalid("缺少预期缺失错误".into()))?;
                    let rendered = format!("{error}; {error:?}");
                    assert!(rendered.contains("声明 app"));
                    assert!(!rendered.contains("PRIVATE_TOKEN_AS_KEY"));
                    assert!(!rendered.contains("private_secret_as_key"));
                    if key.starts_with("server.") {
                        assert!(rendered.contains("依赖 server"));
                    }
                }
                Ok(serde_json::json!({}))
            });
        evaluate_test_manifest(&manifest)?;
        for (key, expected) in [("app.url", "自循环"), ("server.port", "前向依赖")] {
            let manifest = ConfigManifest::new()
                .section("app", move |context| context.require::<Value>(key))
                .section("server", |_| Ok(serde_json::json!({"port":4000})));
            let error = manifest.evaluate_with(
                &ConfigContext::new(EnvironmentSnapshot::capture()),
                ReadView { raw: serde_json::json!({"app":{"url":"http://localhost"},"server":{"port":3000}}), typed:Vec::new() },
                |_| Err(ConfigError::Invalid("不应到达发布候选".into())),
            ).err().ok_or_else(|| ConfigError::Invalid("缺少预期依赖错误".into()))?;
            let rendered = error.to_string();
            assert!(rendered.contains(expected));
            assert!(rendered.contains("声明 app"));
            assert!(rendered.contains(&format!("依赖 {key}")));
        }
        let manifest = ConfigManifest::new().section("PRIVATE_DECLARATION", |context| {
            context.require::<Value>("PRIVATE_DEPENDENCY")
        });
        let error = evaluate_test_manifest(&manifest)
            .err()
            .ok_or_else(|| ConfigError::Invalid("缺少预期缺失错误".into()))?;
        let rendered = format!("{error}; {error:?}");
        assert!(!rendered.contains("PRIVATE_DECLARATION"));
        assert!(!rendered.contains("PRIVATE_DEPENDENCY"));
        Ok(())
    }

    #[test]
    fn arbitrary_user_errors_cannot_impersonate_safe_dependency_errors() {
        let manifest = ConfigManifest::new().section("first", |_| {
            Err::<Value, _>(ConfigError::Invalid("PRIVATE_USER_ERROR".to_owned()))
        });
        let error = evaluate_test_manifest(&manifest)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("求值失败"));
        assert!(!error.contains("PRIVATE_USER_ERROR"));
        let manifest = ConfigManifest::new().typed::<u8, _>("runtime", |_| {
            Err(ConfigError::Invalid("PRIVATE_TYPED_ERROR".to_owned()))
        });
        let error = evaluate_test_manifest(&manifest)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("求值失败"));
        assert!(!error.contains("PRIVATE_TYPED_ERROR"));
    }

    #[test]
    fn dependencies_use_final_code_toml_environment_and_preoverrides_once_in_order() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(
            #[allow(
                clippy::result_large_err,
                reason = "Figment Jail 回调要求原生 figment::Error"
            )]
            |jail| {
                jail.clear_env();
                jail.create_dir("config")?;
                jail.create_file("config/server.toml", "port = 4101\n")?;
                for (toml, environment, overrides, expected) in [
                    (false, false, false, 4100),
                    (true, false, false, 4101),
                    (true, true, false, 4102),
                    (true, true, true, 4103),
                ] {
                    if environment {
                        jail.set_env("SERVER_PORT", "4102");
                    }
                    jail.set_env("APP_NAME", "final-app");
                    let calls = Arc::new(Mutex::new(Vec::new()));
                    let server_calls = calls.clone();
                    let app_calls = calls.clone();
                    let auth_calls = calls.clone();
                    let manifest = ConfigManifest::new()
                        .section("server", move |_| {
                            server_calls.lock().map_err(|_| ConfigError::Invalid("测试锁失败".into()))?.push("server");
                            Ok(serde_json::json!({"port":4100}))
                        })
                        .section("app", move |context| {
                            app_calls.lock().map_err(|_| ConfigError::Invalid("测试锁失败".into()))?.push("app");
                            Ok(serde_json::json!({"name":"code-app", "url":format!("http://localhost:{}",context.require::<u16>("server.port")?)}))
                        })
                        .section("auth", move |context| {
                            auth_calls.lock().map_err(|_| ConfigError::Invalid("测试锁失败".into()))?.push("auth");
                            Ok(serde_json::json!({"issuer":context.require::<String>("app.name")?, "url":context.require::<String>("app.url")?}))
                        });
                    let manifest = if toml { manifest.with_toml() } else { manifest };
                    let entries = if overrides {
                        vec![("server", serde_json::json!({"port":4103}))]
                    } else {
                        Vec::new()
                    };
                    let sources = ConfigLoader::application_code_sources_with_overrides(
                        ".", "APP_", None, &manifest, entries,
                    )
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                    assert_eq!(
                        sources.figment().extract_inner::<u16>("server.port")?,
                        expected
                    );
                    assert_eq!(
                        sources.figment().extract_inner::<String>("app.url")?,
                        format!("http://localhost:{expected}")
                    );
                    assert_eq!(
                        sources.figment().extract_inner::<String>("auth.url")?,
                        format!("http://localhost:{expected}")
                    );
                    assert_eq!(
                        sources.figment().extract_inner::<String>("auth.issuer")?,
                        "final-app"
                    );
                    assert_eq!(
                        *calls
                            .lock()
                            .map_err(|_| figment::Error::from("测试锁失败"))?,
                        ["server", "app", "auth"]
                    );
                }
                Ok(())
            },
        );
    }

    #[test]
    fn overlays_do_not_mask_forward_dependencies_or_typed_raw_conflicts() {
        let _environment_guard = crate::lock_test_env();
        figment::Jail::expect_with(
            #[allow(
                clippy::result_large_err,
                reason = "Figment Jail 回调要求原生 figment::Error"
            )]
            |jail| {
                jail.clear_env();
                jail.set_env("SERVER_PORT", "4999");
                let manifest = ConfigManifest::new()
                    .section("app", |context| {
                        Ok(serde_json::json!({"port":context.require::<u16>("server.port")?}))
                    })
                    .section("server", |_| Ok(serde_json::json!({"port":4000})));
                let error = ConfigLoader::application_code_sources_with_overrides(
                    ".",
                    "APP_",
                    None,
                    &manifest,
                    [("server", serde_json::json!({"port":5000}))],
                )
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
                assert!(error.contains("前向依赖"));
                for typed_first in [false, true] {
                    let manifest = if typed_first {
                        ConfigManifest::new()
                            .typed("example.runtime", |_| Ok(42_u32))
                            .section("example", |_| Ok(serde_json::json!({"runtime":"conflict"})))
                    } else {
                        ConfigManifest::new()
                            .section("example", |_| Ok(serde_json::json!({"runtime":"conflict"})))
                            .typed("example.runtime", |_| Ok(42_u32))
                    };
                    assert!(
                        ConfigLoader::application_code_sources(".", "APP_", None, &manifest)
                            .is_err()
                    );
                }
                Ok(())
            },
        );
    }

    #[test]
    fn early_and_dotenv_selected_late_cache_skip_functions_and_reject_preoverrides() {
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
                jail.create_file("bootstrap/cache/config.json", r#"{"seed":{"number":1}}"#)?;
                jail.create_file(".env", "broken dotenv line")?;
                let manifest = ConfigManifest::new().section("seed", |_| {
                    Err::<Value, _>(ConfigError::Invalid("FUNCTION_EXECUTED".into()))
                });
                let cached = ConfigLoader::application_code_sources(".", "APP_", None, &manifest)
                    .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(cached.figment().extract_inner::<u16>("seed.number")?, 1);
                let error = ConfigLoader::application_code_sources_with_overrides(
                    ".",
                    "APP_",
                    None,
                    &manifest,
                    [("seed", serde_json::json!({"number":2}))],
                )
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
                assert!(error.contains("求值前覆盖"));
                jail.create_dir("late-app")?;
                jail.create_file("late-app/.env", "SHOP_CONFIG_CACHE=late-cache.json\n")?;
                jail.create_file("late-app/late-cache.json", r#"{"seed":{"number":3}}"#)?;
                let cached =
                    ConfigLoader::application_code_sources("late-app", "SHOP_", None, &manifest)
                        .map_err(|error| figment::Error::from(error.to_string()))?;
                assert_eq!(cached.figment().extract_inner::<u16>("seed.number")?, 3);
                assert!(ConfigRepository::new(cached).is_cached());
                let error = ConfigLoader::application_code_sources_with_overrides(
                    "late-app",
                    "SHOP_",
                    None,
                    &manifest,
                    [("seed", serde_json::json!({"number":2}))],
                )
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
                assert!(error.contains("求值前覆盖"));
                Ok(())
            },
        );
    }
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

#[cfg(test)]
mod nullable_bool_tests {
    #[test]
    fn laravel_reserved_environment_values_then_php_truthiness_are_preserved() {
        for (raw, expected) in [
            ("null", None),
            ("(NuLl)", None),
            ("false", Some(false)),
            ("(false)", Some(false)),
            ("true", Some(true)),
            ("(true)", Some(true)),
            ("", Some(false)),
            ("0", Some(false)),
            ("empty", Some(false)),
            ("(empty)", Some(false)),
            ("off", Some(true)),
            ("no", Some(true)),
            ("\"null\"", Some(true)),
            ("\"false\"", Some(true)),
            ("\"0\"", Some(false)),
            ("'0'", Some(false)),
        ] {
            assert_eq!(super::nullable_env_bool(raw), expected, "{raw:?}");
        }
    }
}
