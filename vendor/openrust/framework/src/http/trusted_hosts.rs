//! 可信主机的请求局部模式快照与 PHP PCRE 分隔符边界。

use axum::http::StatusCode;
use pcre2::bytes::RegexBuilder;

use crate::config::TrustedHostsConfig;
use crate::core::{Error, Result};

/// `None` 禁用；`Some([])` 表示已启用但无 allowlist，与 Symfony 相同。
#[derive(Clone, Default)]
pub(crate) struct TrustedHostsSnapshot(Option<Vec<String>>);

impl TrustedHostsSnapshot {
    pub(crate) fn disabled() -> Self {
        Self(None)
    }

    pub(crate) fn from_config(config: &TrustedHostsConfig, app_url: &str) -> Self {
        Self::from_patterns(config.at.clone(), config.subdomains, app_url)
    }

    pub(crate) fn from_patterns(
        patterns: Option<Vec<String>>,
        subdomains: bool,
        app_url: &str,
    ) -> Self {
        let Some(mut resolved) = patterns else {
            return Self(Some(default_pattern(app_url).into_iter().collect()));
        };
        if subdomains && let Some(pattern) = default_pattern(app_url) {
            resolved.push(pattern);
        }
        // 列表按 PHP array_filter 规则过滤，字符串 "0" 也属于 PHP falsey。
        resolved.retain(|pattern| !pattern.is_empty() && pattern != "0");
        Self(Some(resolved))
    }

    pub(crate) fn check(&self, host: &str) -> Result<()> {
        let Some(patterns) = &self.0 else {
            return Ok(());
        };
        if patterns.is_empty() {
            return Ok(());
        }
        for body in patterns {
            php_brace_delimiter(body)?;
            let regex = RegexBuilder::new()
                .caseless(true)
                .build(body)
                .map_err(|_| invalid_pattern())?;
            if regex
                .is_match(host.as_bytes())
                .map_err(|_| invalid_pattern())?
            {
                return Ok(());
            }
        }
        Err(Error::custom_with_status(
            StatusCode::BAD_REQUEST,
            "http_host_untrusted",
            "请求 Host 不在信任列表中",
        ))
    }
}

fn default_pattern(app_url: &str) -> Option<String> {
    let host = url::Url::parse(app_url).ok()?.host_str()?.to_owned();
    Some(format!(r"^(.+\.)?{}$", preg_quote(&host)))
}

// PHP preg_quote 在没有显式 delimiter 参数时仍转义 PCRE 元字符。
fn preg_quote(host: &str) -> String {
    let mut quoted = String::with_capacity(host.len());
    for ch in host.chars() {
        if matches!(
            ch,
            '.' | '\\'
                | '+'
                | '*'
                | '?'
                | '['
                | ']'
                | '^'
                | '$'
                | '('
                | ')'
                | '{'
                | '}'
                | '='
                | '!'
                | '<'
                | '>'
                | '|'
                | ':'
                | '-'
                | '#'
        ) {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted
}

// Symfony 对模式体做 sprintf("{%s}i")；PHP 在 PCRE2 编译前先解析成对的花括号。
fn php_brace_delimiter(body: &str) -> Result<()> {
    let mut depth = 1usize;
    let mut escaped = false;
    for byte in body.bytes() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'{' => depth = depth.saturating_add(1),
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Err(invalid_pattern());
                }
            }
            _ => {}
        }
    }
    if depth != 1 {
        return Err(invalid_pattern());
    }
    Ok(())
}

fn invalid_pattern() -> Error {
    Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "http_trusted_host_pattern_invalid",
        "信任主机模式无法求值",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcre_body_and_php_delimiter_semantics() -> Result<()> {
        for (pattern, host) in [
            (r"^(?=api\.)api\.example$", "api.example"),
            (r"^([a-z]+)\.\1\.test$", "foo.foo.test"),
            (r"^api(\.local){0,1}\.test$", "api.local.test"),
            (r"trusted", "untrusted.test"),
        ] {
            TrustedHostsSnapshot::from_patterns(
                Some(vec![pattern.into()]),
                false,
                "http://unused.test",
            )
            .check(host)?;
        }
        let invalid = TrustedHostsSnapshot::from_patterns(
            Some(vec!["api}test".into()]),
            false,
            "http://unused.test",
        );
        assert_eq!(
            invalid.check("api.test").err().map(|error| error.status()),
            Some(StatusCode::INTERNAL_SERVER_ERROR)
        );
        Ok(())
    }

    #[test]
    fn defaults_empty_and_short_circuit() -> Result<()> {
        let default = TrustedHostsSnapshot::from_config(
            &TrustedHostsConfig::default(),
            "http://Example.Test:8080",
        );
        default.check("deep.api.example.test")?;
        assert_eq!(
            default.check("evil.test").err().map(|error| error.status()),
            Some(StatusCode::BAD_REQUEST)
        );
        TrustedHostsSnapshot::from_patterns(Some(Vec::new()), false, "http://example.test")
            .check("evil.test")?;
        TrustedHostsSnapshot::from_patterns(None, false, "http://example.test")
            .check("api.example.test")?;
        TrustedHostsSnapshot::from_patterns(None, false, "not a URL").check("evil.test")?;
        TrustedHostsSnapshot::from_patterns(
            Some(vec![String::new(), "0".into()]),
            false,
            "http://example.test",
        )
        .check("evil.test")?;
        let short = TrustedHostsSnapshot::from_patterns(
            Some(vec!["trusted".into(), "[".into()]),
            false,
            "http://example.test",
        );
        short.check("untrusted.test")?;
        assert_eq!(
            short.check("other.test").err().map(|error| error.status()),
            Some(StatusCode::INTERNAL_SERVER_ERROR)
        );
        Ok(())
    }
}
