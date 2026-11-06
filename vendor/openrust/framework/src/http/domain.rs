//! DNS 域路由模式与已匹配域参数的提取。

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use axum::extract::FromRequestParts;
use axum::http::{Extensions, request::Parts};
use serde::de::{DeserializeOwned, value::MapDeserializer};

use crate::core::{Error, Result};

/// 将当前已匹配域路由的标签参数提取为 serde 类型。
///
/// 参数值是规范化为小写的 DNS 标签，可提取为含 `String` 字段的结构体或字符串 map。
/// 参数独立于路径参数；域路由以外的请求返回 `http_domain_params_unavailable`，
/// 不匹配目标类型时返回 `http_domain_params_invalid`。域名只参与路由匹配，不能代替身份验证。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainParams<T>(pub T);

#[derive(Clone)]
struct DomainParamValues(BTreeMap<String, String>);

pub(crate) fn set_domain_params(extensions: &mut Extensions, values: BTreeMap<String, String>) {
    extensions.insert(DomainParamValues(values));
}

pub(crate) fn route_params(extensions: &Extensions) -> Vec<(String, String)> {
    extensions
        .get::<DomainParamValues>()
        .map_or_else(Vec::new, |values| {
            values
                .0
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
}

impl<S, T> FromRequestParts<S> for DomainParams<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = Error;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self>> + Send {
        std::future::ready((|| {
            let values = parts.extensions.get::<DomainParamValues>().ok_or_else(|| {
                Error::custom(
                    "http_domain_params_unavailable",
                    "当前请求没有已匹配的域路由参数",
                )
            })?;
            let deserializer = MapDeserializer::<_, serde::de::value::Error>::new(
                values
                    .0
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            );
            T::deserialize(deserializer).map(Self).map_err(|_| {
                Error::custom(
                    "http_domain_params_invalid",
                    "域路由参数无法反序列化为目标类型",
                )
            })
        })())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DomainPattern {
    normalized: String,
    labels: Vec<DomainLabel>,
}

#[derive(Debug, Clone)]
enum DomainLabel {
    Exact(String),
    Parameter(String),
}

impl DomainPattern {
    /// 解析 ASCII DNS 标签与占据完整标签的 `{name}` 参数。
    ///
    /// 不执行 Unicode/IDNA 转换；已经编码的 ASCII punycode 标签按普通 DNS 标签处理。
    /// IP literal、末尾点、authority 端口及 URL 其他组成部分均不支持。
    pub(crate) fn parse(pattern: &str) -> Result<Self> {
        if !valid_host_shape(pattern) {
            return Err(pattern_error());
        }
        let mut parameters = BTreeSet::new();
        let mut labels = Vec::new();
        let mut normalized = Vec::new();
        for label in pattern.split('.') {
            if let Some(name) = label.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                if !valid_parameter_name(name) || !parameters.insert(name.to_owned()) {
                    return Err(pattern_error());
                }
                labels.push(DomainLabel::Parameter(name.to_owned()));
                normalized.push(format!("{{{name}}}"));
            } else {
                if !valid_dns_label(label) {
                    return Err(pattern_error());
                }
                let label = label.to_ascii_lowercase();
                normalized.push(label.clone());
                labels.push(DomainLabel::Exact(label));
            }
        }
        Ok(Self {
            normalized: normalized.join("."),
            labels,
        })
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.normalized
    }

    /// 匹配已经由调用方剥离端口的 host；非法或未支持的 host 均不匹配。
    pub(crate) fn captures(&self, host: &str) -> Option<BTreeMap<String, String>> {
        if !valid_host_shape(host) || !host.split('.').all(valid_dns_label) {
            return None;
        }
        let host = host.to_ascii_lowercase();
        let labels: Vec<_> = host.split('.').collect();
        if labels.len() != self.labels.len() {
            return None;
        }
        let mut captures = BTreeMap::new();
        for (pattern, label) in self.labels.iter().zip(labels) {
            match pattern {
                DomainLabel::Exact(expected) if expected != label => return None,
                DomainLabel::Parameter(name) => {
                    captures.insert(name.clone(), label.to_owned());
                }
                DomainLabel::Exact(_) => {}
            }
        }
        Some(captures)
    }
}

fn valid_host_shape(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.is_ascii()
        && !host.ends_with('.')
        && !host.bytes().any(|byte| {
            byte.is_ascii_whitespace() || matches!(byte, b'/' | b':' | b'?' | b'#' | b'@' | b'\\')
        })
        && host.parse::<IpAddr>().is_err()
}

fn valid_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && label
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_parameter_name(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn pattern_error() -> Error {
    Error::custom(
        "http_domain_pattern_invalid",
        "域模式仅支持 ASCII DNS 标签及完整标签 {name}；不支持 Unicode、IP、末尾点或 URL 组成部分",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use serde::Deserialize;

    #[test]
    fn exact_domains_match_case_insensitively() -> Result<()> {
        let pattern = DomainPattern::parse("APP.Example.COM")?;
        assert_eq!(pattern.as_str(), "app.example.com");
        assert_eq!(pattern.captures("app.EXAMPLE.com"), Some(BTreeMap::new()));
        assert!(pattern.captures("example.com").is_none());
        assert!(
            DomainPattern::parse("xn--bcher-kva.example")?
                .captures("XN--BCHER-KVA.example")
                .is_some()
        );
        Ok(())
    }

    #[test]
    fn parameters_capture_one_complete_normalized_label() -> Result<()> {
        let pattern = DomainPattern::parse("{account}.{region}.Example.COM")?;
        let expected = BTreeMap::from([
            ("account".into(), "acme-42".into()),
            ("region".into(), "eu".into()),
        ]);
        assert_eq!(pattern.captures("ACME-42.EU.example.com"), Some(expected));
        for host in [
            "a.b.eu.example.com",
            "eu.example.com",
            "a.eu.example.com.attacker.test",
            "a.eu.example.com:80",
            "a.eu.example.com.",
            "a..example.com",
            "a.eu.example.com/",
            "a.eu.example.com@attacker.test",
            "é.eu.example.com",
        ] {
            assert!(pattern.captures(host).is_none(), "{host}");
        }
        assert!(
            DomainPattern::parse("{a}.{b}.{c}.{d}")?
                .captures("127.0.0.1")
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn invalid_patterns_fail_with_stable_code() {
        let long_label = format!("{}.example", "a".repeat(64));
        let label = "a".repeat(63);
        let long_host = [label.as_str(); 4].join(".");
        for pattern in [
            "",
            ".example.com",
            "a..example.com",
            "example.com.",
            "-a.example",
            "a-.example",
            "a_b.example",
            "https://example.com",
            "example.com:443",
            "example.com/path",
            "example.com?x=1",
            "example.com#x",
            "u@example.com",
            "127.0.0.1",
            "[::1]",
            "bücher.example",
            "pre-{tenant}.example",
            "{tenant}post.example",
            "{}.example",
            "{{tenant}}.example",
            "{a}.{a}.example",
            "{a?}.example",
            "{*a}.example",
            "{1a}.example",
            "{a-b}.example",
            &long_label,
            &long_host,
        ] {
            assert!(
                matches!(DomainPattern::parse(pattern), Err(error)
                if error.code() == "http_domain_pattern_invalid"),
                "{pattern}"
            );
        }
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Tenant {
        account: String,
    }

    #[derive(Debug, Deserialize)]
    struct MissingField {
        #[serde(rename = "missing")]
        _missing: String,
    }

    #[tokio::test]
    async fn extractor_requires_domain_extension_and_deserializes_captures() -> Result<()> {
        let (mut parts, _) = Request::new(Body::empty()).into_parts();
        let missing = DomainParams::<Tenant>::from_request_parts(&mut parts, &()).await;
        assert!(matches!(missing, Err(error) if error.code() == "http_domain_params_unavailable"));
        set_domain_params(
            &mut parts.extensions,
            BTreeMap::from([("account".into(), "acme".into())]),
        );
        let DomainParams(tenant) =
            DomainParams::<Tenant>::from_request_parts(&mut parts, &()).await?;
        assert_eq!(
            tenant,
            Tenant {
                account: "acme".into()
            }
        );
        let invalid = DomainParams::<MissingField>::from_request_parts(&mut parts, &()).await;
        assert!(matches!(invalid, Err(error) if error.code() == "http_domain_params_invalid"));
        Ok(())
    }
}
