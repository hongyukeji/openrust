//! gRPC listener、资源限额、TLS 与具名客户端的应用默认值。

use openrust::config::{
    ConfigContext, ConfigError, GrpcClientConfig, GrpcClientTlsConfig, GrpcConfig,
    GrpcServerTlsConfig,
};
use std::collections::BTreeMap;

/// 只控制下方代码默认值；实际启用前须提供证书文件。
const SERVER_TLS_ENABLED: bool = false;
/// 具名客户端配置示例默认不登记，不在启动时连接该 endpoint。
const CLIENT_ENABLED: bool = false;
/// 客户端 TLS 只与 https endpoint 一起使用；false 时使用 http endpoint。
const CLIENT_TLS_ENABLED: bool = false;

/// 保留回环 listener 和拒绝隐式明文监听的策略。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<GrpcConfig, ConfigError> {
    let clients = if CLIENT_ENABLED {
        BTreeMap::from([(
            "backend".to_owned(),
            GrpcClientConfig {
                endpoint: if CLIENT_TLS_ENABLED {
                    "https://localhost:50052".to_owned()
                } else {
                    "http://127.0.0.1:50052".to_owned()
                },
                tls: if CLIENT_TLS_ENABLED {
                    Some(GrpcClientTlsConfig {
                        ca_certificate_file: "certs/ca.pem".into(),
                        // None 不提供 mTLS 客户端身份；Some 证书/私钥必须成对配置。
                        client_certificate_file: None,
                        client_private_key_file: None,
                        // None 使用 endpoint 主机名进行证书验证及 SNI。
                        domain_name: None,
                    })
                } else {
                    None
                },
            },
        )])
    } else {
        BTreeMap::new()
    };
    Ok(GrpcConfig {
        listen_addr: std::net::SocketAddr::from(([127, 0, 0, 1], 50051)),
        tls: if SERVER_TLS_ENABLED {
            Some(GrpcServerTlsConfig {
                certificate_file: "certs/server.pem".into(),
                private_key_file: "certs/server-key.pem".into(),
                // None 不要求 mTLS；Some CA 文件要求客户端证书。
                client_ca_certificate_file: None,
            })
        } else {
            None
        },
        // 无 TLS 仍默认拒绝启动；仅可信本机开发/终止代理才显式改为 true。
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
        clients,
        // None 要求调用者显式选择具名客户端；Some 必须引用 clients 中的名称。
        default_client: None,
    })
}
