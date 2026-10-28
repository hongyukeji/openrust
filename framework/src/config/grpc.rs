//! gRPC listener、TLS 信任材料与具名客户端配置。

use std::{collections::BTreeMap, fmt, net::SocketAddr, path::PathBuf};

use serde::{Deserialize, Serialize};

/// `[grpc]` 区段：gRPC listener、TLS 与具名客户端。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrpcConfig {
    /// gRPC server 绑定地址；配置插件不会自动启动 listener。
    pub listen_addr: SocketAddr,
    /// 服务端 TLS 配置。无 TLS 时必须显式允许明文服务。
    pub tls: Option<GrpcServerTlsConfig>,
    /// 显式允许无 TLS 的服务端监听，仅用于本机开发或可信 TLS 终止代理。
    pub allow_insecure_server: bool,
    /// 每条 HTTP/2 连接可同时打开的 RPC stream 上限。
    pub max_concurrent_streams_per_connection: u32,
    /// 单条连接可同时执行的 RPC 上限；过载时立即拒绝，不排队等待。
    pub max_concurrent_rpcs_per_connection: usize,
    /// 所有连接共享的在途 RPC 上限；stream 响应结束或客户端断开后释放名额。
    pub max_concurrent_rpcs: usize,
    /// 每条 RPC 允许的最大解码消息字节数；需在生成服务构造时应用。
    pub max_decoding_message_size_bytes: usize,
    /// 每条 RPC 允许的最大编码消息字节数；需在生成服务构造时应用。
    pub max_encoding_message_size_bytes: usize,
    /// 没有 `grpc-timeout` 时应用的总调用预算（秒）。
    pub default_timeout_secs: u64,
    /// 单次调用允许的最大总预算（秒）；客户端期限会被此值截断。
    pub max_timeout_secs: u64,
    /// 收到的 HTTP/2 metadata/header list 最大字节数。
    pub max_metadata_size_bytes: u32,
    /// 服务端 reflection 默认关闭。
    pub reflection_enabled: bool,
    /// 停机后保留 Health `Check`/`Watch` 的时间；计入服务总停机 deadline。
    pub health_drain_grace_ms: u64,
    /// 客户端名称到 endpoint 和独立 TLS 信任材料的映射。
    pub clients: BTreeMap<String, GrpcClientConfig>,
    /// 默认客户端名称；为空时调用方必须指定具名 endpoint。
    pub default_client: Option<String>,
}

/// 服务端 TLS 证书与可选客户端证书信任根。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrpcServerTlsConfig {
    /// PEM 格式的服务端证书文件。
    pub certificate_file: PathBuf,
    /// PEM 格式的服务端私钥文件。
    pub private_key_file: PathBuf,
    /// 配置后要求客户端提供由此 CA 签发的证书（mTLS）。
    pub client_ca_certificate_file: Option<PathBuf>,
}

impl fmt::Debug for GrpcServerTlsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GrpcServerTlsConfig")
            .field("certificate_file", &"[已配置]")
            .field("private_key_file", &"[已配置]")
            .field(
                "client_ca_certificate_file",
                &self.client_ca_certificate_file.as_ref().map(|_| "[已配置]"),
            )
            .finish()
    }
}

/// 具名 gRPC 客户端 endpoint 与其 TLS 配置。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrpcClientConfig {
    /// gRPC endpoint URI；支持 `http://` 或配合显式 CA 的 `https://`。
    pub endpoint: String,
    /// HTTPS endpoint 的 CA 与可选客户端身份。
    pub tls: Option<GrpcClientTlsConfig>,
}

impl fmt::Debug for GrpcClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GrpcClientConfig")
            .field("endpoint", &safe_endpoint_debug(&self.endpoint))
            .field("tls", &self.tls)
            .finish()
    }
}

/// 具名客户端的信任根、客户端身份和可选 TLS server name。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrpcClientTlsConfig {
    /// PEM 格式的服务端 CA 证书文件。
    pub ca_certificate_file: PathBuf,
    /// PEM 格式的可选客户端证书；设置时必须同时设置私钥。
    pub client_certificate_file: Option<PathBuf>,
    /// PEM 格式的可选客户端私钥。
    pub client_private_key_file: Option<PathBuf>,
    /// 可选证书 hostname/SNI 覆盖；默认使用 endpoint hostname。
    pub domain_name: Option<String>,
}

impl fmt::Debug for GrpcClientTlsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GrpcClientTlsConfig")
            .field("ca_certificate_file", &"[已配置]")
            .field(
                "client_certificate_file",
                &self.client_certificate_file.as_ref().map(|_| "[已配置]"),
            )
            .field(
                "client_private_key_file",
                &self.client_private_key_file.as_ref().map(|_| "[已配置]"),
            )
            .field("domain_name", &self.domain_name)
            .finish()
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 50051)),
            tls: None,
            allow_insecure_server: false,
            max_concurrent_streams_per_connection: 128,
            max_concurrent_rpcs_per_connection: 64,
            max_concurrent_rpcs: 1024,
            max_decoding_message_size_bytes: 4 * 1024 * 1024,
            max_encoding_message_size_bytes: 4 * 1024 * 1024,
            default_timeout_secs: 30,
            max_timeout_secs: 300,
            max_metadata_size_bytes: 8 * 1024,
            reflection_enabled: false,
            health_drain_grace_ms: 250,
            clients: BTreeMap::new(),
            default_client: None,
        }
    }
}

impl GrpcConfig {
    /// 校验具名客户端、endpoint scheme 与默认 endpoint 引用。
    pub fn validate(&self) -> crate::Result<()> {
        self.validate_runtime_limits()?;
        for (name, client) in &self.clients {
            if !valid_client_name(name) {
                return Err(crate::Error::custom(
                    "grpc_config_invalid",
                    "grpc.clients 的名称必须以小写字母开头，且只包含小写字母、数字、短横线和下划线",
                ));
            }
            let endpoint = url::Url::parse(&client.endpoint).map_err(|_| {
                crate::Error::custom(
                    "grpc_config_invalid",
                    format!("grpc.clients.{name}.endpoint 不是有效 URI"),
                )
            })?;
            if !endpoint.username().is_empty() || endpoint.password().is_some() {
                return Err(crate::Error::custom(
                    "grpc_config_invalid",
                    format!("grpc.clients.{name}.endpoint 不接受 URI 内嵌凭据"),
                ));
            }
            match endpoint.scheme() {
                "http" if client.tls.is_none() => {}
                "https" if client.tls.is_some() => {}
                "http" => {
                    return Err(crate::Error::custom(
                        "grpc_config_invalid",
                        format!("grpc.clients.{name} 的 http endpoint 不应配置 TLS"),
                    ));
                }
                "https" => {
                    return Err(crate::Error::custom(
                        "grpc_config_invalid",
                        format!("grpc.clients.{name} 的 https endpoint 必须配置显式 CA"),
                    ));
                }
                _ => {
                    return Err(crate::Error::custom(
                        "grpc_config_invalid",
                        format!("grpc.clients.{name}.endpoint 仅支持 http:// 与 https://"),
                    ));
                }
            }
            if let Some(tls) = &client.tls
                && (tls.ca_certificate_file.as_os_str().is_empty()
                    || tls
                        .client_certificate_file
                        .as_ref()
                        .is_some_and(|path| path.as_os_str().is_empty())
                    || tls
                        .client_private_key_file
                        .as_ref()
                        .is_some_and(|path| path.as_os_str().is_empty())
                    || tls.client_certificate_file.is_some()
                        != tls.client_private_key_file.is_some()
                    || tls
                        .domain_name
                        .as_ref()
                        .is_some_and(|domain| domain.trim().is_empty()))
            {
                return Err(crate::Error::custom(
                    "grpc_config_invalid",
                    format!("grpc.clients.{name}.tls 配置无效"),
                ));
            }
        }
        if self.tls.is_some() && self.allow_insecure_server {
            return Err(crate::Error::custom(
                "grpc_config_invalid",
                "grpc.tls 与 grpc.allow_insecure_server 不能同时配置",
            ));
        }
        if let Some(tls) = &self.tls
            && (tls.certificate_file.as_os_str().is_empty()
                || tls.private_key_file.as_os_str().is_empty()
                || tls
                    .client_ca_certificate_file
                    .as_ref()
                    .is_some_and(|path| path.as_os_str().is_empty()))
        {
            return Err(crate::Error::custom(
                "grpc_config_invalid",
                "grpc.tls 的证书、私钥与客户端 CA 路径不能为空",
            ));
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

    fn validate_runtime_limits(&self) -> crate::Result<()> {
        if self.max_concurrent_streams_per_connection == 0
            || self.max_concurrent_streams_per_connection > 65_535
            || self.max_concurrent_rpcs_per_connection == 0
            || self.max_concurrent_rpcs_per_connection > 65_535
            || self.max_concurrent_rpcs == 0
            || self.max_concurrent_rpcs > 1_000_000
            || !(1..=64 * 1024 * 1024).contains(&self.max_decoding_message_size_bytes)
            || !(1..=64 * 1024 * 1024).contains(&self.max_encoding_message_size_bytes)
            || self.default_timeout_secs == 0
            || self.max_timeout_secs < self.default_timeout_secs
            || self.default_timeout_secs > 86_400
            || self.max_timeout_secs > 86_400
            || self.max_metadata_size_bytes == 0
            || self.max_metadata_size_bytes > 1024 * 1024
            || self.health_drain_grace_ms > 30_000
        {
            return Err(crate::Error::custom(
                "grpc_config_invalid",
                "grpc 的消息、stream、并发、期限和 metadata 限额超出允许范围",
            ));
        }
        Ok(())
    }
}

fn safe_endpoint_debug(endpoint: &str) -> String {
    url::Url::parse(endpoint).map_or_else(
        |_| "[无效 endpoint]".to_owned(),
        |url| url.origin().ascii_serialization(),
    )
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
    use super::{GrpcClientConfig, GrpcClientTlsConfig, GrpcConfig};
    use std::collections::BTreeMap;

    #[test]
    fn validates_client_names_and_default_reference() -> crate::Result<()> {
        let config = GrpcConfig {
            clients: BTreeMap::from([(
                "billing-rpc".to_owned(),
                GrpcClientConfig {
                    endpoint: "http://127.0.0.1:6000".to_owned(),
                    tls: None,
                },
            )]),
            default_client: Some("billing-rpc".to_owned()),
            allow_insecure_server: true,
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

    #[test]
    fn rejects_inconsistent_tls_and_redacts_material_paths() {
        let invalid_pair = GrpcConfig {
            clients: BTreeMap::from([(
                "billing".to_owned(),
                GrpcClientConfig {
                    endpoint: "https://billing.internal".to_owned(),
                    tls: Some(GrpcClientTlsConfig {
                        ca_certificate_file: "ca.pem".into(),
                        client_certificate_file: Some("client.pem".into()),
                        client_private_key_file: None,
                        domain_name: None,
                    }),
                },
            )]),
            ..GrpcConfig::default()
        };
        assert_eq!(
            invalid_pair
                .validate()
                .err()
                .map(|error| error.code().to_owned()),
            Some("grpc_config_invalid".to_owned())
        );
        let debug = format!("{invalid_pair:?}");
        assert!(!debug.contains("ca.pem"));
        assert!(!debug.contains("client.pem"));
    }

    #[test]
    fn rejects_endpoint_credentials_and_redacts_endpoint_path() {
        let config = GrpcConfig {
            clients: BTreeMap::from([(
                "billing".to_owned(),
                GrpcClientConfig {
                    endpoint: "http://user:secret@example.test/private/path".to_owned(),
                    tls: None,
                },
            )]),
            ..GrpcConfig::default()
        };
        assert_eq!(
            config.validate().err().map(|error| error.code().to_owned()),
            Some("grpc_config_invalid".to_owned())
        );
        let debug = format!("{:?}", config.clients["billing"]);
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("private/path"));
    }

    #[test]
    fn rejects_unbounded_or_inconsistent_runtime_limits() {
        let zero = GrpcConfig {
            max_concurrent_rpcs: 0,
            ..GrpcConfig::default()
        };
        assert_eq!(
            zero.validate().err().map(|error| error.code().to_owned()),
            Some("grpc_config_invalid".to_owned())
        );

        let inverted_budget = GrpcConfig {
            default_timeout_secs: 30,
            max_timeout_secs: 10,
            ..GrpcConfig::default()
        };
        assert_eq!(
            inverted_budget
                .validate()
                .err()
                .map(|error| error.code().to_owned()),
            Some("grpc_config_invalid".to_owned())
        );
        assert!(GrpcConfig::default().validate().is_ok());
    }
}
