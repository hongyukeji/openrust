//! API 响应配置。

use serde::{Deserialize, Serialize};

/// `[api]` 区段：JSON 响应策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    /// 应用 API 路由前缀；默认 `/api`，空值表示应用根。
    pub prefix: String,
    /// 框架 JSON 响应的 HTTP 状态码策略。
    pub http_status: HttpStatusMode,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            prefix: "/api".into(),
            http_status: HttpStatusMode::default(),
        }
    }
}

impl ApiConfig {
    /// 按路由组规则去除两端斜线；拒绝不能作为可挂载前缀的路径。
    ///
    /// 普通参数段 `{tenant}` 可用；末尾 catch-all 无法再挂载子路由，明确拒绝。
    pub fn normalized_prefix(&self) -> Result<String, super::ConfigError> {
        let prefix = self.prefix.trim_matches('/');
        let invalid = || super::ConfigError::diagnostic("api.prefix", "invalid_prefix");
        if prefix
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '?' | '#' | '\\'))
        {
            return Err(invalid());
        }
        if prefix.is_empty() {
            return Ok(String::new());
        }
        let mut parameters = std::collections::HashSet::new();
        for segment in prefix.split('/') {
            if segment.is_empty()
                || matches!(segment, "." | "..")
                || segment.starts_with([':', '*'])
            {
                return Err(invalid());
            }
            if segment.contains(['{', '}']) {
                let name = segment
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .ok_or_else(invalid)?;
                if name.is_empty()
                    || name.contains(['{', '}', '*'])
                    || !parameters.insert(name)
                    || parameters.len() > 25
                {
                    return Err(invalid());
                }
            }
        }
        Ok(format!("/{prefix}"))
    }

    /// 按相同前缀的完整路径段判断 API 请求；`/v1` 不匹配 `/v10`。
    ///
    /// 启动时先调用 `normalized_prefix` 校验；参数段只匹配一个非空路径段。
    pub fn matches_path(&self, path: &str) -> bool {
        let prefix = self.prefix.trim_matches('/');
        if prefix.is_empty() {
            return path.starts_with('/');
        }
        let Some(path) = path.strip_prefix('/') else {
            return false;
        };
        let mut actual = path.split('/');
        prefix.split('/').all(|segment| {
            actual.next().is_some_and(|part| {
                if segment.starts_with('{') && segment.ends_with('}') {
                    !part.is_empty()
                } else {
                    segment == part
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_normalization_and_request_boundaries_share_route_group_semantics()
    -> Result<(), super::super::ConfigError> {
        for (input, expected) in [
            ("/api", "/api"),
            ("v1/", "/v1"),
            ("/", ""),
            ("", ""),
            ("//v1//", "/v1"),
        ] {
            let config = ApiConfig {
                prefix: input.into(),
                ..ApiConfig::default()
            };
            assert_eq!(config.normalized_prefix()?, expected);
        }
        let config = ApiConfig {
            prefix: "/v1/".into(),
            ..ApiConfig::default()
        };
        assert!(config.matches_path("/v1"));
        assert!(config.matches_path("/v1/users"));
        assert!(!config.matches_path("/v10"));
        assert!(!config.matches_path("/api"));
        let config = ApiConfig {
            prefix: "tenant/{tenant}/api".into(),
            ..ApiConfig::default()
        };
        assert_eq!(config.normalized_prefix()?, "/tenant/{tenant}/api");
        assert!(config.matches_path("/tenant/acme/api/users"));
        assert!(!config.matches_path("/tenant//api"));
        Ok(())
    }

    #[test]
    fn malformed_prefix_fails_without_echoing_source() {
        for prefix in [
            "a//b",
            "a/../b",
            "a?secret-token",
            "bad path",
            "a/{unclosed",
            "a/{*rest}",
            "a/\\b",
        ] {
            let config = ApiConfig {
                prefix: prefix.into(),
                ..ApiConfig::default()
            };
            assert!(
                matches!(config.normalized_prefix(), Err(error) if error.to_string() == "api.prefix [invalid_prefix]")
            );
        }
    }
}

/// 框架 JSON 响应的 HTTP 状态码策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpStatusMode {
    /// 始终返回 HTTP 200，并在响应正文中提供业务响应码；仅在应用显式选择时使用。
    AlwaysOk,
    /// 保留语义 HTTP 状态码，同时返回响应正文。
    #[default]
    Semantic,
}
