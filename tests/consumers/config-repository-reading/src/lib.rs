//! 独立应用通过公开配置仓储读取业务配置。

use openrust::config::{ConfigError, repository::ConfigRepository};
use openrust::core::{BootContext, BoxFuture, ServiceProvider};
use serde_json::{Value, json};

/// 只向业务展示预先选定的名称；不接受客户端提供的配置路径。
pub fn public_display_name(repository: &ConfigRepository) -> Result<String, ConfigError> {
    let value = repository.get_or("app.name", json!("应用"))?;
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ConfigError::Invalid("app.name 必须是字符串".to_owned()))
}

/// 从同一快照取得固定业务配置，保留首次出现的顺序。
pub fn public_fields(repository: &ConfigRepository) -> Result<Vec<(String, Value)>, ConfigError> {
    repository
        .get_many_with_defaults(&[("app.name", json!("应用")), ("app.locale", json!("zh_CN"))])
}

/// 仅展示本 fixture 两个固定业务键在同一配置来源中的出现顺序。
pub fn public_reader_key_order(repository: &ConfigRepository) -> Result<Vec<String>, ConfigError> {
    let value = repository.get("reader")?;
    let Some(Value::Object(fields)) = value else {
        return Ok(Vec::new());
    };
    Ok(fields
        .keys()
        .filter(|key| matches!(key.as_str(), "z_label" | "a_label"))
        .cloned()
        .collect())
}

/// 只写入 fixture 自有的固定展示名称。
pub fn review_display_name(repository: &ConfigRepository) -> openrust::Result<String> {
    repository.set("app.name", json!("已复核应用"))?;
    public_display_name(repository).map_err(Into::into)
}

/// 生成应用可显式安装的公开 Provider fixture；最终校验读取全部 boot 覆盖后的配置。
pub struct ReadingProvider;

impl ServiceProvider for ReadingProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let repository = app.config_repository();
            let request_repository = repository.clone();
            app.routes(move |routes| {
                routes
                    .get("/api/config-reading", {
                        let repository = request_repository.clone();
                        move || {
                            let repository = repository.clone();
                            async move {
                                let name = public_display_name(&repository)?;
                                let locale = repository.get_or("app.locale", json!("zh_CN"))?;
                                Ok::<_, openrust::Error>(openrust::http::Json(
                                    json!({"name": name, "locale": locale}),
                                ))
                            }
                        }
                    })
                    .post("/api/config-reading/review", {
                        let repository = request_repository.clone();
                        move || {
                            let repository = repository.clone();
                            async move {
                                let name = review_display_name(&repository)?;
                                Ok::<_, openrust::Error>(openrust::http::Json(
                                    json!({"name": name}),
                                ))
                            }
                        }
                    })
                    .get("/api/config-reading/order", {
                        let repository = request_repository.clone();
                        move || {
                            let repository = repository.clone();
                            async move {
                                let keys = public_reader_key_order(&repository)?;
                                Ok::<_, openrust::Error>(openrust::http::Json(
                                    json!({"keys": keys}),
                                ))
                            }
                        }
                    })
            })?;
            app.validate_at_finalize(move || {
                Box::pin(async move {
                    let _ = public_display_name(&repository)?;
                    let _ = public_fields(&repository)?;
                    Ok(())
                })
            });
            Ok(())
        })
    }
}
