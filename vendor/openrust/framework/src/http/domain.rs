//! 将当前路由已匹配的域参数提取为 serde 类型。

use crate::core::{Error, Result};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::de::{DeserializeOwned, value::MapDeserializer};

/// 将当前已匹配域路由的标签参数提取为 serde 类型。
///
/// 参数值是规范化为小写的 DNS 标签，可提取为含 `String` 字段的结构体或字符串 map。
/// 参数独立于路径参数；域路由以外的请求返回 `http_domain_params_unavailable`，
/// 不匹配目标类型时返回 `http_domain_params_invalid`。域名只参与路由匹配，不能代替身份验证。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainParams<T>(pub T);

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
            let values = crate::routing::domain::domain_param_values(&parts.extensions)
                .ok_or_else(|| {
                    Error::custom(
                        "http_domain_params_unavailable",
                        "当前请求没有已匹配的域路由参数",
                    )
                })?;
            let deserializer = MapDeserializer::<_, serde::de::value::Error>::new(
                values
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
