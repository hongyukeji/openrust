//! 固定 Passport Bridge `ClientRepository`：复用同一读取仓储及统一 `HashManager`。

use super::{BridgeClient, ClientLookup, ClientLookupContext};
use openrust::{
    Error, Result,
    container::Scope,
    hashing::{HashManager, HashOptions},
};
use serde_json::Value;
use std::sync::Arc;

/// League 客户端投影与 secret 校验；grant 资格仍由实体和实际 grant 流程负责。
#[derive(Clone)]
pub struct BridgeClientRepository {
    clients: Arc<dyn ClientLookup>,
    hashing: Arc<HashManager>,
}
impl BridgeClientRepository {
    /// 与模型仓储共用入口生命周期和应用默认哈希驱动。
    pub fn new(clients: Arc<dyn ClientLookup>, hashing: Arc<HashManager>) -> Self {
        Self { clients, hashing }
    }
    /// 查找活动模型后通过已有 `BridgeClient` 投影；不查询 secret、不检查 grant。
    pub async fn get_client_entity(
        &self,
        scope: &Scope,
        identifier: &str,
    ) -> Result<Option<BridgeClient>> {
        let context = ClientLookupContext::from(scope);
        context.ensure_open().await?;
        self.clients
            .find_active_by_protocol(&context, identifier)
            .await?
            .map(|record| BridgeClient::from_client_model(record.as_ref()))
            .transpose()
    }
    /// 固定版本忽略 grant 参数；缺失/撤销/empty secret 为 false，哈希错误原样传播。
    /// 明文保持原字节；空数组和单字节 `b"0"` 仍按 PHP empty 语义拒绝。
    /// 已编码 hash 仍经原模型属性读取和字符串 cast，不变更 secret setter 或明文字段。
    pub async fn validate_client(
        &self,
        scope: &Scope,
        identifier: &str,
        secret: Option<&[u8]>,
        _grant: Option<&str>,
    ) -> Result<bool> {
        let context = ClientLookupContext::from(scope);
        context.ensure_open().await?;
        let Some(record) = self
            .clients
            .find_active_by_protocol(&context, identifier)
            .await?
        else {
            return Ok(false);
        };
        let Some(secret) = secret.filter(|secret| !secret.is_empty() && *secret != b"0") else {
            return Ok(false);
        };
        // `$record->secret` 是正常属性读取；confidential() 才使用原始 secret。
        let encoded = match openrust::database::eloquent::AttributeCast::String
            .read(record.get_attribute("secret")?)?
        {
            None | Some(Value::Null) => None,
            Some(Value::String(encoded)) => Some(encoded),
            Some(_) => {
                return Err(Error::custom_with_status(
                    openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "passport_client_secret_attribute_invalid",
                    "客户端 secret 访问器必须返回字符串或 null",
                ));
            }
        };
        self.hashing
            .check(secret, encoded.as_deref(), &HashOptions::default())
            .await
    }
}

#[cfg(test)]
#[path = "bridge_repository_tests.rs"]
mod tests;
