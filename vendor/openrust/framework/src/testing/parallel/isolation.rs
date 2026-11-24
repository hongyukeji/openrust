use super::{ParallelTestError, ParallelTesting, truthy};
use crate::{
    config::ConfigError,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::facades::File,
    testing::TestApp,
};
use serde_json::Value;
use std::{path::PathBuf, sync::Mutex};
#[derive(Default)]
struct Original {
    prefix: Option<String>,
    compiled: Option<Value>,
}
#[derive(Default)]
pub(super) struct Isolation(Mutex<Original>);
fn scalar_token(value: Value) -> Result<String, ParallelTestError> {
    match value {
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(if value { "1".into() } else { String::new() }),
        Value::Null => Ok(String::new()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(std::io::Error::other("并行测试路径需要字符串或数值token").into()),
    }
}
impl ParallelTesting {
    fn view_path(&self, app: &Application) -> Result<Option<PathBuf>, ParallelTestError> {
        let previous = self
            .isolation
            .0
            .lock()
            .expect("并行测试隔离配置锁中毒")
            .compiled
            .clone();
        let original = match previous {
            Some(value) => value,
            None => {
                let value = app
                    .config()?
                    .get("view.compiled")
                    .unwrap_or_else(|| Value::String(String::new()));
                if !matches!(value, Value::String(_) | Value::Null | Value::Bool(false)) {
                    return Err(ConfigError::InvalidType {
                        key: "view.compiled".into(),
                        expected: "目录路径字符串",
                    }
                    .into());
                }
                // 固定实现使用??=；显式null不锁定原值，下次仍可读取更新后的配置。
                if value.is_null() {
                    value
                } else {
                    self.isolation
                        .0
                        .lock()
                        .expect("并行测试隔离配置锁中毒")
                        .compiled
                        .get_or_insert(value)
                        .clone()
                }
            }
        };
        if !truthy(&original) {
            return Ok(None);
        }
        let original = original.as_str().ok_or_else(|| ConfigError::InvalidType {
            key: "view.compiled".into(),
            expected: "目录路径字符串",
        })?;
        let token = scalar_token(self.token()?)?;
        // token来自测试运行器；拒绝跨平台路径分隔符，确保自动清理只落在该子目录。
        if token
            .chars()
            .any(|c| c.is_control() || "/\\<>:\"|?*".contains(c))
        {
            return Err(std::io::Error::other("并行测试token不能包含路径分隔符或保留字符").into());
        }
        Ok(Some(PathBuf::from(format!(
            "{}/test_{token}",
            original.trim_end_matches(['\\', '/'])
        ))))
    }
    pub(super) async fn prepare_process(&self, app: &Application) -> Result<(), ParallelTestError> {
        if let Some(path) = self.view_path(app)? {
            File::ensure_directory_exists_with_options(app.base_path().join(path), 0o755, true)?;
        }
        Ok(())
    }
    pub(super) async fn prepare_case(&self, test: &mut TestApp) -> Result<(), ParallelTestError> {
        let app = test.app();
        if !truthy(&self.option("without_cache")?) {
            let previous = self
                .isolation
                .0
                .lock()
                .expect("并行测试隔离配置锁中毒")
                .prefix
                .clone();
            let prefix = match previous {
                Some(value) => value,
                None => match app.config()?.get("cache.prefix") {
                    Some(Value::Null) => String::new(),
                    value => {
                        let value = match value {
                            None => String::new(),
                            Some(Value::String(value)) => value,
                            _ => {
                                return Err(ConfigError::InvalidType {
                                    key: "cache.prefix".into(),
                                    expected: "字符串",
                                }
                                .into());
                            }
                        };
                        self.isolation
                            .0
                            .lock()
                            .expect("并行测试隔离配置锁中毒")
                            .prefix
                            .get_or_insert(value)
                            .clone()
                    }
                },
            };
            app.config()?.set(
                "cache.prefix",
                Value::String(format!("{prefix}test_{}_", scalar_token(self.token()?)?)),
            )?;
        }
        if let Some(path) = self.view_path(app)? {
            app.config()?.set(
                "view.compiled",
                Value::String(path.to_string_lossy().into_owned()),
            )?;
            let absolute = app.base_path().join(path);
            if app.container().resolved("view")? {
                app.container()
                    .get::<crate::view::Factory>("view")?
                    .compiler_handle()
                    .set_cache_directory(absolute.clone());
            }
            if app.container().resolved("blade.compiler")? {
                app.container()
                    .get::<crate::view::compilers::BladeCompiler>("blade.compiler")?
                    .set_cache_directory(absolute);
            }
        }
        Ok(())
    }
    pub(super) async fn cleanup_process(&self, app: &Application) -> Result<(), ParallelTestError> {
        if let Some(path) = self.view_path(app)? {
            let path = app.base_path().join(path);
            let _ = File::delete_directory(path)?;
        }
        Ok(())
    }
}
