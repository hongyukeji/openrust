use std::net::IpAddr;

use openrust::{
    config::{ConfigError, Environment, Value},
    translation::canonical_locale,
};
use serde_json::json;

use super::env::{get, list};

pub(super) struct Loaded<'a> {
    pub(super) config: Value,
    pub(super) name: &'a str,
    pub(super) public_path: &'a str,
    pub(super) url: String,
}

pub(super) fn load(env: &Environment) -> Result<Loaded<'_>, ConfigError> {
    let name = env.get_utf8("APP_NAME")?.unwrap_or(env!("CARGO_PKG_NAME"));
    let env_host = env.get_utf8("APP_HOST")?;
    let host = env_host.unwrap_or("127.0.0.1");
    let port = env.get_utf8("APP_PORT")?.unwrap_or("8000");
    let public_path = env.get_utf8("APP_PUBLIC_PATH")?.unwrap_or("public");
    if public_path.trim().is_empty() {
        return Err(ConfigError::InvalidType {
            key: "APP_PUBLIC_PATH".into(),
            expected: "a non-empty directory path",
        });
    }
    let (url, url_explicit) = resolve_url(env, env_host, port)?;
    let port = port
        .parse::<u16>()
        .map_or_else(|_| json!(port), |port| json!(port));
    let config = json!({
        /*
        |--------------------------------------------------------------------------
        | 应用名称与版本
        |--------------------------------------------------------------------------
        |
        | 应用名称和版本默认取 Cargo 包信息，APP_NAME 和 APP_VERSION 可分别覆盖。
        |
        */
        "name": name,
        "version": get(env, "APP_VERSION", env!("CARGO_PKG_VERSION"))?,

        /*
        |--------------------------------------------------------------------------
        | 运行环境与调试
        |--------------------------------------------------------------------------
        |
        | APP_ENV 用于区分部署环境。APP_DEBUG 控制错误响应的详细程度，生产环境应关闭。
        |
        */
        "env": get(env, "APP_ENV", "production")?,
        "debug": get(env, "APP_DEBUG", false)?,

        /*
        |--------------------------------------------------------------------------
        | 监听地址与公开 URL
        |--------------------------------------------------------------------------
        |
        | APP_HOST 和 APP_PORT 控制监听地址。部署在域名或反向代理后时，用 APP_URL 指定对外地址。
        | FRONTEND_URL 和 ASSET_URL 分别配置前端地址和资源地址。
        |
        */
        "host": host,
        "port": port,
        "url": url,
        "url_explicit": url_explicit,
        "frontend_url": get(env, "FRONTEND_URL", "http://localhost:3000")?,
        "asset_url": get(env, "ASSET_URL", Value::Null)?,

        /*
        |--------------------------------------------------------------------------
        | 时区与语言
        |--------------------------------------------------------------------------
        |
        | APP_TIMEZONE 设置应用时区。APP_LOCALE 和 APP_FALLBACK_LOCALE 设置首选和回退语言。
        | APP_FAKER_LOCALE 用于测试和填充数据。
        |
        */
        "timezone": get(env, "APP_TIMEZONE", "UTC")?,
        "locale": locale(env, "APP_LOCALE", "en")?,
        "fallback_locale": locale(env, "APP_FALLBACK_LOCALE", "en")?,
        "faker_locale": locale(env, "APP_FAKER_LOCALE", "en-US")?,
        // URL 语言查询参数名；null 表示关闭 URL 语言切换。
        "locale_query_parameter": get(env, "APP_LOCALE_QUERY_PARAMETER", "lang")?,

        /*
        |--------------------------------------------------------------------------
        | 应用加密
        |--------------------------------------------------------------------------
        |
        | APP_CIPHER 和 APP_KEY 用于应用加密。首次部署可用 artisan key:generate 生成密钥。
        | 轮换密钥时将仍需解密旧数据的密钥以逗号分隔填入 APP_PREVIOUS_KEYS。
        |
        */
        "cipher": get(env, "APP_CIPHER", "AES-256-CBC")?,
        "key": get(env, "APP_KEY", Value::Null)?,
        "previous_keys": list(env.get_utf8("APP_PREVIOUS_KEYS")?.unwrap_or_default()),

        /*
        |--------------------------------------------------------------------------
        | 维护模式
        |--------------------------------------------------------------------------
        |
        | APP_MAINTENANCE_DRIVER 选择状态存储方式。使用缓存驱动时，APP_MAINTENANCE_STORE 指定缓存仓储。
        |
        */
        "maintenance": {
            "driver": get(env, "APP_MAINTENANCE_DRIVER", "file")?,
            "store": get(env, "APP_MAINTENANCE_STORE", "database")?,
        },

        /*
        |--------------------------------------------------------------------------
        | 后台运行器
        |--------------------------------------------------------------------------
        |
        | APP_DISABLED_RUNNERS 接受逗号分隔的运行器名称，用于关闭随应用启动的对应进程内任务。
        |
        */
        "disabled_runners": list(env.get_utf8("APP_DISABLED_RUNNERS")?.unwrap_or_default()),

        /*
        |--------------------------------------------------------------------------
        | 公共目录
        |--------------------------------------------------------------------------
        |
        | APP_PUBLIC_PATH 指定应用公共目录，网站磁盘及访问前缀在 filesystems 配置。
        |
        */
        "public_path": public_path,
    });
    Ok(Loaded {
        config,
        name,
        public_path,
        url,
    })
}

/*
 * 解析应用的公开 URL，并记录它是否来自显式配置。
 *
 * 设置 APP_URL 时直接采用该值。未设置时，从监听地址和端口推导 HTTP URL。
 * 通配和回环地址统一使用 localhost，其余 IPv6 地址用方括号包围。
 * 这个标记供 URL 生成器判断是优先使用配置地址，还是从当前请求取地址。
 */
fn resolve_url(
    env: &Environment,
    host: Option<&str>,
    port: &str,
) -> Result<(String, bool), ConfigError> {
    if let Some(url) = env.get_utf8("APP_URL")? {
        return Ok((url.to_owned(), true));
    }

    let host = host.filter(|host| !host.is_empty()).unwrap_or("localhost");
    let address = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    let url = match address.parse::<IpAddr>() {
        Ok(ip) if ip.is_unspecified() || ip.is_loopback() => format!("http://localhost:{port}"),
        Ok(IpAddr::V6(_)) => format!("http://[{address}]:{port}"),
        _ => format!("http://{host}:{port}"),
    };
    Ok((url, false))
}

fn locale(env: &Environment, key: &str, default: &str) -> Result<String, ConfigError> {
    canonical_locale(env.get_utf8(key)?.unwrap_or(default)).map_err(|_| ConfigError::InvalidType {
        key: key.into(),
        expected: "BCP 47 语言标签（如 en、zh-Hans、zh-Hant）",
    })
}
