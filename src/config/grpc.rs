//! gRPC listener 与客户端 endpoint 配置。

use std::{collections::BTreeMap, net::SocketAddr};

use serde::{Deserialize, Serialize};

/// `[grpc]` 区段：gRPC listener 地址与具名客户端 endpoint。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrpcConfig {
    /// gRPC server 绑定地址；配置插件不会自动启动 listener。
    pub listen_addr: SocketAddr,
    /// 客户端名称到 endpoint URI 的映射。
    pub clients: BTreeMap<String, String>,
    /// 默认客户端名称；为空时调用方必须指定具名 endpoint。
    pub default_client: Option<String>,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 50051)),
            clients: BTreeMap::new(),
            default_client: None,
        }
    }
}

impl GrpcConfig {
    /// 校验具名客户端和默认 endpoint 引用。
    pub fn validate(&self) -> crate::Result<()> {
        for name in self.clients.keys() {
            if !valid_client_name(name) {
                return Err(crate::Error::custom(
                    "grpc_config_invalid",
                    "grpc.clients 的名称必须以小写字母开头，且只包含小写字母、数字、短横线和下划线",
                ));
            }
        }
        if self
            .default_client
            .as_ref()
            .is_some_and(|name| !self.clients.contains_key(name))
        {
            return Err(crate::Error::custom(
                "grpc_config_invalid",
                "grpc.default_client 必须引用已配置的 grpc.clients 名称",
            ));
        }
        Ok(())
    }
}

fn valid_client_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
}

#[cfg(test)]
mod tests {
    use super::GrpcConfig;
    use std::collections::BTreeMap;

    #[test]
    fn validates_client_names_and_default_reference() -> crate::Result<()> {
        let config = GrpcConfig {
            clients: BTreeMap::from([(
                "billing-rpc".to_owned(),
                "http://127.0.0.1:6000".to_owned(),
            )]),
            default_client: Some("billing-rpc".to_owned()),
            ..GrpcConfig::default()
        };
        config.validate()?;
        let invalid = GrpcConfig {
            default_client: Some("missing".to_owned()),
            ..config
        };
        assert_eq!(
            invalid
                .validate()
                .err()
                .map(|error| error.code().to_owned()),
            Some("grpc_config_invalid".to_owned())
        );
        Ok(())
    }
}
