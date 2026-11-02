//! 框架生产者定义的安全配置诊断；原始来源不参与格式化。

use std::fmt;

use super::ConfigError;

/// 固定配置键和原因码；任意源值只保留在私有错误链中。
pub struct ConfigurationDiagnostic {
    key: &'static str,
    code: &'static str,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl ConfigurationDiagnostic {
    /// 框架定义的配置键，不是输入值。
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// 框架定义的固定原因码，不从外部错误字符串推导。
    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for ConfigurationDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} [{}]", self.key, self.code)
    }
}

impl fmt::Debug for ConfigurationDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigurationDiagnostic")
            .field("key", &self.key)
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

impl std::error::Error for ConfigurationDiagnostic {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl ConfigError {
    pub(crate) fn diagnostic(key: &'static str, code: &'static str) -> Self {
        Self::Diagnostic(ConfigurationDiagnostic {
            key,
            code,
            source: None,
        })
    }

    pub(crate) fn diagnostic_with_source<E>(
        key: &'static str,
        code: &'static str,
        source: E,
    ) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Diagnostic(ConfigurationDiagnostic {
            key,
            code,
            source: Some(Box::new(source)),
        })
    }
}
