use super::{ValidationError, ValidationFuture, ValidationResult, Value};
use crate::{contracts::validation::UncompromisedVerifier, http::client::Factory as HttpFactory};
use std::{sync::Arc, time::Duration};

/// 仅发送SHA-1前五位的泄漏范围查询；SHA-1用于外部协议，不用于密码存储。
pub struct NotPwnedVerifier {
    factory: Arc<HttpFactory>,
    timeout: Duration,
}
impl NotPwnedVerifier {
    pub fn new(factory: Arc<HttpFactory>, timeout: Option<Duration>) -> Self {
        Self {
            factory,
            timeout: timeout.unwrap_or(Duration::from_secs(30)),
        }
    }
    async fn search(&self, prefix: &str) -> ValidationResult<String> {
        let headers = http::HeaderMap::from_iter([(
            http::HeaderName::from_static("add-padding"),
            http::HeaderValue::from_static("true"),
        )]);
        match self
            .factory
            .pending_request()
            .with_headers(headers)
            .timeout(self.timeout)
            .get(&format!("https://api.pwnedpasswords.com/range/{prefix}"))
            .await
        {
            Ok(response) if response.successful() => response
                .body()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|source| ValidationError::Execution {
                    source: Box::new(source),
                }),
            Ok(_) => Ok(String::new()),
            Err(error) => {
                crate::support::facades::Exceptions::report(error)
                    .await
                    .map_err(|source| ValidationError::Execution {
                        source: Box::new(source),
                    })?;
                Ok(String::new())
            }
        }
    }
}
impl UncompromisedVerifier for NotPwnedVerifier {
    fn verify<'a>(
        &'a self,
        value: &'a Value,
        threshold: i64,
    ) -> ValidationFuture<'a, ValidationResult<bool>> {
        Box::pin(async move {
            let value = super::standard_rules::text(value).unwrap_or_else(|| match value {
                Value::Null => String::new(),
                Value::Array(_) | Value::Object(_) => "Array".into(),
                _ => String::new(),
            });
            if value.is_empty() || value == "0" {
                return Ok(false);
            }
            use aws_lc_rs::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
            let hash = digest(&SHA1_FOR_LEGACY_USE_ONLY, value.as_bytes())
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>();
            let prefix = &hash[..5];
            let body = self.search(prefix).await?;
            Ok(!body
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .split('\n')
                .filter_map(|line| line.split_once(':'))
                .any(|(suffix, count)| {
                    let count = count.split(':').next().unwrap_or_default();
                    hash.len() == prefix.len() + suffix.len()
                        && hash.starts_with(prefix)
                        && &hash[5..] == suffix
                        && crate::support::php_integer_less_than_value(
                            threshold,
                            &Value::String(count.into()),
                        )
                }))
        })
    }
}
