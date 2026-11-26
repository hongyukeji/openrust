use serde::Deserialize;
use url::{Host, Url};

use crate::{Error, Result, extensions};

/// 按目标协议选择已授权的具名代理，并可为指定主机选择直接连接。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyMap {
    http: Option<String>,
    https: Option<String>,
    #[serde(default)]
    no: Vec<String>,
}

impl ProxyMap {
    pub fn new(http: Option<String>, https: Option<String>, no: Vec<String>) -> Self {
        Self { http, https, no }
    }

    pub(crate) fn select(&self, target: &Url) -> Result<Option<&str>> {
        for proxy in [&self.http, &self.https].into_iter().flatten() {
            extensions::validate_name("proxy", proxy)?;
        }
        if self.no.len() > 128 {
            return Err(Error::InvalidConfig("proxy.no"));
        }
        let host = target.host().ok_or(Error::TargetDenied)?;
        let mut excluded = false;
        for pattern in &self.no {
            excluded |= excludes(&host, pattern)?;
        }
        if excluded {
            return Ok(None);
        }
        match target.scheme() {
            "http" => Ok(self.http.as_deref()),
            "https" => Ok(self.https.as_deref()),
            _ => Err(Error::TargetDenied),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ProxySetting {
    Reference(String),
    Map(ProxyMap),
}

impl ProxySetting {
    pub(crate) fn select(&self, target: &Url) -> Result<Option<&str>> {
        match self {
            Self::Reference(reference) => {
                extensions::validate_name("proxy", reference)?;
                Ok(Some(reference))
            }
            Self::Map(map) => map.select(target),
        }
    }
}

fn excludes(host: &Host<&str>, pattern: &str) -> Result<bool> {
    if pattern == "*" {
        return Ok(true);
    }
    let invalid = || Error::InvalidConfig("proxy.no");
    let (name, subdomains_only) = match pattern.strip_prefix('.') {
        Some(name) => (name, true),
        None => (pattern, false),
    };
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('.')
        || name.contains(['/', '\\', '@', '#', '?', '%', '*'])
        || name.chars().any(char::is_whitespace)
    {
        return Err(invalid());
    }
    let pattern = Host::parse(name)
        .or_else(|error| {
            name.parse::<std::net::Ipv6Addr>()
                .map(Host::Ipv6)
                .map_err(|_| error)
        })
        .map_err(|_| invalid())?;
    match (host, pattern) {
        (Host::Domain(host), Host::Domain(pattern)) => {
            let (host, pattern) = (host.trim_end_matches('.'), pattern.trim_end_matches('.'));
            if pattern.is_empty() {
                return Err(invalid());
            }
            Ok(!subdomains_only && host == pattern
                || host
                    .strip_suffix(pattern)
                    .is_some_and(|prefix| prefix.ends_with('.')))
        }
        (_, Host::Ipv4(_) | Host::Ipv6(_)) if subdomains_only => Err(invalid()),
        (Host::Ipv4(host), Host::Ipv4(pattern)) => Ok(*host == pattern),
        (Host::Ipv6(host), Host::Ipv6(pattern)) => Ok(*host == pattern),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_scheme_and_normalizes_exclusions_on_host_boundaries() {
        let map = ProxyMap::new(
            Some("egress-http".into()),
            Some("egress-tls".into()),
            vec![
                ".example.net".into(),
                "example.org".into(),
                "127.0.0.1".into(),
                "::1".into(),
            ],
        );
        let select = |url| map.select(&Url::parse(url).unwrap()).unwrap();
        assert_eq!(select("http://outside.invalid"), Some("egress-http"));
        assert_eq!(select("https://outside.invalid"), Some("egress-tls"));
        assert_eq!(select("https://EXAMPLE.ORG:8443/"), None);
        assert_eq!(select("https://a.example.org./"), None);
        assert_eq!(select("https://a.example.net/"), None);
        assert_eq!(select("https://example.net/"), Some("egress-tls"));
        assert_eq!(select("https://notexample.org/"), Some("egress-tls"));
        assert_eq!(select("https://example.org.invalid/"), Some("egress-tls"));
        assert_eq!(select("https://127.0.0.1:8443/"), None);
        assert_eq!(select("https://[::1]:8443/"), None);
        let all = ProxyMap::new(None, Some("egress-tls".into()), vec!["*".into()]);
        assert_eq!(
            all.select(&Url::parse("https://outside.invalid").unwrap())
                .unwrap(),
            None
        );
    }

    #[test]
    fn rejects_invalid_names_and_rules_even_on_excluded_targets() {
        let url = Url::parse("https://example.test").unwrap();
        for invalid in [
            "",
            "example.test/path",
            "https://example.test",
            "example.test:443",
            "*example.test",
        ] {
            let map = ProxyMap::new(
                None,
                Some("egress".into()),
                vec!["*".into(), invalid.into()],
            );
            assert!(
                matches!(map.select(&url), Err(Error::InvalidConfig("proxy.no"))),
                "{invalid}"
            );
        }
        let map = ProxyMap::new(None, Some("http://proxy.invalid".into()), vec!["*".into()]);
        assert!(matches!(
            map.select(&url),
            Err(Error::InvalidConfig("proxy"))
        ));
    }
}
