use std::{future::Future, sync::Arc};

use pcre2::bytes::{Regex, RegexBuilder};

use crate::{config::ConfigError, contracts::config::Repository};

use super::{HttpError, HttpFuture, Request, proxy::parse_authority};

type Resolver = dyn Fn() -> HttpFuture<'static, Result<Vec<String>, ConfigError>> + Send + Sync;

#[derive(Clone)]
enum Source {
    Patterns(Arc<[Regex]>),
    Resolver(Arc<Resolver>),
}

#[derive(Clone)]
pub(crate) struct TrustedHosts {
    source: Source,
    allow_subdomains: bool,
    application_pattern: Option<Regex>,
}

impl TrustedHosts {
    pub(crate) fn new(at: &[&str], subdomains: bool) -> Result<Self, ConfigError> {
        Ok(Self {
            source: Source::Patterns(compile_patterns(at.iter().copied())?),
            allow_subdomains: subdomains,
            application_pattern: None,
        })
    }

    pub(crate) fn using<Fut>(at: impl Fn() -> Fut + Send + Sync + 'static, subdomains: bool) -> Self
    where
        Fut: Future<Output = Result<Vec<String>, ConfigError>> + Send + 'static,
    {
        Self {
            source: Source::Resolver(Arc::new(move || Box::pin(at()))),
            allow_subdomains: subdomains,
            application_pattern: None,
        }
    }

    pub(crate) fn configure(&mut self, config: &dyn Repository) -> Result<(), ConfigError> {
        if self.allow_subdomains
            && let Some(value) = config.get("app.url")
            && let Some(url) = value.as_str().and_then(|url| url::Url::parse(url).ok())
            && let Some(host) = url.host_str()
        {
            self.application_pattern =
                Some(compile(&format!(r"^(.+\.)?{}$", regex::escape(host)))?);
        }
        Ok(())
    }

    pub(crate) async fn snapshot(&self) -> Result<HostSnapshot, HttpError> {
        let patterns = match &self.source {
            Source::Patterns(patterns) => patterns.clone(),
            Source::Resolver(resolver) => {
                let names = resolver().await.map_err(internal)?;
                compile_patterns(names.iter().map(String::as_str)).map_err(internal)?
            }
        };
        Ok(HostSnapshot {
            patterns,
            application_pattern: self.application_pattern.clone(),
        })
    }
}

pub(crate) struct HostSnapshot {
    patterns: Arc<[Regex]>,
    application_pattern: Option<Regex>,
}

impl HostSnapshot {
    pub(crate) fn check_incoming(&self, request: &Request) -> Result<(), HttpError> {
        if self.patterns.is_empty() && self.application_pattern.is_none() {
            return Ok(());
        }
        let host_headers = request.headers().get_all(http::header::HOST);
        let mut values = host_headers.iter();
        if let Some(host) = values.next() {
            if values.next().is_some() {
                return Err(HttpError::InvalidRequest);
            }
            let authority = parse_authority(host.to_str().map_err(|_| HttpError::InvalidRequest)?)?;
            self.check(authority.host())?;
        }
        if let Some(host) = request.parts().uri.host() {
            self.check(host)?;
        } else if !request.headers().contains_key(http::header::HOST) {
            return Err(HttpError::InvalidRequest);
        }
        Ok(())
    }

    pub(crate) fn check_effective(&self, request: &Request) -> Result<(), HttpError> {
        if let Some(host) = request.proxy.uri.as_ref().and_then(http::Uri::host) {
            self.check(host)?;
        }
        Ok(())
    }

    fn check(&self, host: &str) -> Result<(), HttpError> {
        if self.patterns.is_empty() && self.application_pattern.is_none() {
            return Ok(());
        }
        for pattern in self.patterns.iter().chain(self.application_pattern.iter()) {
            if pattern.is_match(host.as_bytes()).map_err(internal)? {
                return Ok(());
            }
        }
        Err(HttpError::InvalidRequest)
    }
}

fn compile_patterns<'a>(
    patterns: impl Iterator<Item = &'a str>,
) -> Result<Arc<[Regex]>, ConfigError> {
    patterns
        .filter(|pattern| !pattern.is_empty() && *pattern != "0")
        .map(compile)
        .collect()
}

fn compile(pattern: &str) -> Result<Regex, ConfigError> {
    RegexBuilder::new()
        .caseless(true)
        .build(pattern)
        .map_err(|_| ConfigError::InvalidType {
            key: "trusted hosts".into(),
            expected: "valid PCRE host patterns",
        })
}

fn internal(source: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
