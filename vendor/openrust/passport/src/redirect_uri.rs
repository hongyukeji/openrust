//! 固定 League 9.4.1 的注册重定向匹配、授权选择与授权码交换绑定。
//!
//! 非 loopback 地址逐字节比较；值类型不提供从未验证请求直接建立目标的入口。
//
// 对应 thephpleague/oauth2-server 9.4.1 的 RedirectUriValidator、AbstractGrant
// 和 AuthCodeGrant。MIT License，Copyright (c) Alex Bilbie。
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use super::parameters::normalized;
use std::fmt;

/// 重定向校验失败；错误不携带未经验证的请求地址。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RedirectUriError {
    /// 授权目标不匹配注册项，或省略目标时无法唯一选择；映射 `invalid_client`。
    #[error("客户端重定向地址无效")]
    InvalidClient,
    /// 注册数组默认缺数值键0或遍历到非字符串；映射本地`server_error`，不含地址。
    #[error("客户端重定向注册格式无效")]
    MalformedRegistration,
    /// 授权时提供了地址，而交换请求缺失；映射 `redirect_uri` 的 `invalid_request`。
    #[error("缺少授权码绑定的重定向地址")]
    MissingTokenRedirectUri,
    /// 交换请求地址与授权码绑定值不同；映射带 Invalid redirect URI 提示的 `invalid_request`。
    #[error("重定向地址与授权码绑定值不匹配")]
    TokenRedirectUriMismatch,
}

/// 仅可通过注册匹配或唯一注册项选择建立的授权重定向目标。
///
/// 固定上游允许严格匹配的 URN、自定义 scheme 和 fragment；本类型不擅加 HTTPS
/// 或无 fragment 的默认限制。持久化授权码时保存 `requested_uri()`，不能保存回退目标。
#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedRedirectUri {
    target: String,
    requested: bool,
}

impl fmt::Debug for ValidatedRedirectUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedRedirectUri")
            .field("requested", &self.requested)
            .finish_non_exhaustive()
    }
}

impl ValidatedRedirectUri {
    /// 从授权请求选择目标；传入客户端公开的注册列表，不重新规范化注册值。
    ///
    /// 固定 Passport 以数组暴露注册地址。请求为空时数组长度必须恰好为一；
    /// 上游不会在该分支重新执行 URI 校验，因此单项空字符串也保留其可观察行为。
    pub fn from_authorization_request(
        requested: Option<&str>,
        registered: &[String],
    ) -> Result<Self, RedirectUriError> {
        match normalized(requested) {
            Some(requested) if matches_registered_redirect_uri(requested, registered) => Ok(Self {
                target: requested.to_owned(),
                requested: true,
            }),
            Some(_) => Err(RedirectUriError::InvalidClient),
            None => match registered {
                [target] => Ok(Self {
                    target: target.clone(),
                    requested: false,
                }),
                _ => Err(RedirectUriError::InvalidClient),
            },
        }
    }

    /// 保留客户端注册数组的原键和异质成员，按固定League分支建立授权目标。
    ///
    /// 非loopback严格字符串成员匹配会忽略其他类型；loopback逐项匹配，遇到
    /// 非字符串立即报告配置失败，但已在此前命中时不再读取后续成员。
    /// 省略请求只接受单项数组，并读取数值键0；关联数组的首值不能冒充该键。
    /// 本方法沿用现有loopback字符串比较器，其完整URI规范化仍需独立上游验收。
    pub fn from_client_array(
        requested: Option<&str>,
        registered: &crate::PhpArray,
    ) -> Result<Self, RedirectUriError> {
        let target = if let Some(requested) = normalized(requested) {
            if let Some(requested_loopback) = loopback_without_port(requested) {
                let mut matched = false;
                for allowed in registered.values() {
                    let allowed = allowed
                        .as_str()
                        .ok_or(RedirectUriError::MalformedRegistration)?;
                    if loopback_without_port(allowed).as_deref()
                        == Some(requested_loopback.as_str())
                    {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return Err(RedirectUriError::InvalidClient);
                }
            } else if !registered.contains_strict_string(requested) {
                return Err(RedirectUriError::InvalidClient);
            }
            return Ok(Self {
                target: requested.to_owned(),
                requested: true,
            });
        } else {
            if registered.values().count() != 1 {
                return Err(RedirectUriError::InvalidClient);
            }
            // PhpArray将{"0":value}规范为列表；其他关联键不能通过values().next回退。
            let zero = match registered.as_value() {
                serde_json::Value::Array(values) => values.first(),
                serde_json::Value::Object(values) => values.get("0"),
                _ => None,
            };
            zero.and_then(serde_json::Value::as_str)
                .ok_or(RedirectUriError::MalformedRegistration)?
        };
        Ok(Self {
            target: target.to_owned(),
            requested: false,
        })
    }

    /// 返回已按固定上游规则选中的目标，供响应层使用。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.target
    }

    /// 返回授权请求实际提供的地址；未提供时必须在授权码中保存空值。
    #[must_use]
    pub fn requested_uri(&self) -> Option<&str> {
        self.requested.then_some(self.target.as_str())
    }
}

/// 按授权码中保存的原始可选地址校验交换请求，不应用 loopback 端口例外。
pub fn validate_token_redirect_uri(
    stored: Option<&str>,
    requested: Option<&str>,
) -> Result<(), RedirectUriError> {
    let requested = normalized(requested);
    match (stored, requested) {
        (Some(_), None) => Err(RedirectUriError::MissingTokenRedirectUri),
        (stored, Some(requested)) if stored != Some(requested) => {
            Err(RedirectUriError::TokenRedirectUriMismatch)
        }
        _ => Ok(()),
    }
}

/// 对应 League 注册匹配器；不裁剪、不解析后比较非 loopback 地址。
#[must_use]
pub fn matches_registered_redirect_uri(requested: &str, registered: &[String]) -> bool {
    if let Some(requested) = loopback_without_port(requested) {
        registered
            .iter()
            .filter_map(|value| loopback_without_port(value))
            .any(|allowed| requested == allowed)
    } else {
        registered.iter().any(|allowed| allowed == requested)
    }
}

// URL 库只负责拒绝无效语法。比较保留原路径，避免 WHATWG 将 /a/../b、
// 127.1、反斜杠等重写成另一个已注册目标。只规范化 RFC scheme/host 大小写。
fn loopback_without_port(value: &str) -> Option<String> {
    let (scheme, rest) = value.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let (userinfo, host_port) = authority
        .rsplit_once('@')
        .map_or(("", authority), |(userinfo, host)| {
            (&authority[..=userinfo.len()], host)
        });
    let (host, port) = if let Some(suffix) = host_port.strip_prefix("[::1]") {
        ("[::1]", suffix)
    } else {
        let suffix = host_port.strip_prefix("127.0.0.1")?;
        ("127.0.0.1", suffix)
    };
    if !(port.is_empty()
        || port.starts_with(':') && port[1..].bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    url::Url::parse(value).ok()?;
    Some(format!("http://{userinfo}{host}{}", &rest[authority_end..]))
}

#[cfg(test)]
#[path = "redirect_uri_tests.rs"]
mod tests;
