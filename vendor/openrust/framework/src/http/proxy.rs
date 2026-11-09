//! 同一请求快照中的可信代理来源；各公开属性独立解析并保留错误边界。

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::connect_info::ConnectInfo;
use http::{Extensions, HeaderMap, Uri};

use crate::{Result, config::ServerConfig, core::Error};

use crate::http::authority;
use crate::http::client_ip;
use crate::http::transport::CloudIngress;
use crate::http::transport::RequestSecurity;

/// Symfony `Request::HEADER_*` 位掩码；默认使用全部 X-Forwarded 位。
pub struct TrustedProxyHeaders;
impl TrustedProxyHeaders {
    /// RFC 7239 `Forwarded`。
    pub const FORWARDED: u8 = 0b00_0001;
    /// `X-Forwarded-For`。
    pub const FOR: u8 = 0b00_0010;
    /// `X-Forwarded-Host`。
    pub const HOST: u8 = 0b00_0100;
    /// `X-Forwarded-Proto`。
    pub const PROTO: u8 = 0b00_1000;
    /// `X-Forwarded-Port`。
    pub const PORT: u8 = 0b01_0000;
    /// `X-Forwarded-Prefix`。
    pub const PREFIX: u8 = 0b10_0000;
    /// AWS ELB 子集，不包含 Host 和 Prefix。
    pub const AWS_ELB: u8 = Self::FOR | Self::PROTO | Self::PORT;
    /// 默认掩码，数值 62。
    pub const DEFAULT: u8 = Self::FOR | Self::HOST | Self::PROTO | Self::PORT | Self::PREFIX;
}

#[derive(Clone)]
pub(crate) struct ProxyContext {
    peer: Option<IpAddr>,
    trusted: Vec<client_ip::IpRange>,
    headers: HeaderMap,
    mask: u8,
    native_secure: bool,
    platform_host: bool,
}

#[derive(Clone)]
pub(crate) struct ProxySnapshot(
    pub(crate) std::result::Result<Arc<ProxyContext>, Arc<crate::core::Error>>,
);

impl ProxySnapshot {
    pub(crate) fn capture(
        headers: &HeaderMap,
        uri: &Uri,
        extensions: &Extensions,
        config: Option<&ServerConfig>,
    ) -> Self {
        Self(
            ProxyContext::resolve(headers, uri, extensions, config)
                .map(Arc::new)
                .map_err(Arc::new),
        )
    }

    pub(crate) fn context(&self) -> Result<&ProxyContext> {
        self.0
            .as_ref()
            .map(Arc::as_ref)
            .map_err(|error| Error::internal(Arc::clone(error)))
    }
}

impl ProxyContext {
    pub(crate) fn resolve(
        headers: &HeaderMap,
        uri: &Uri,
        extensions: &Extensions,
        config: Option<&ServerConfig>,
    ) -> Result<Self> {
        let peer = extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        let native_secure = extensions
            .get::<RequestSecurity>()
            .is_some_and(|value| value.is_secure());
        let mut trusted = Vec::new();
        let mut mask = TrustedProxyHeaders::DEFAULT;
        let mut platform_host = false;
        if let Some(config) = config {
            validate_headers(config.trusted_proxy_headers)?;
            trusted =
                client_ip::parse_ranges(config.trusted_proxy_ranges.as_deref().unwrap_or(&[]))?;
            mask = if config.trusted_proxy_headers == 0 {
                TrustedProxyHeaders::DEFAULT
            } else {
                config.trusted_proxy_headers
            };
            if let Some(peer) = peer {
                if config
                    .trusted_proxy_ranges
                    .as_deref()
                    .is_some_and(|ranges| ranges.iter().any(|range| range == "REMOTE_ADDR"))
                {
                    trusted.extend(client_ip::parse_ranges(&[peer.to_string()])?);
                }
                platform_host = self::platform_host(headers, uri);
                if config.trusted_proxy_ranges.is_none()
                    && (platform_host || extensions.get::<CloudIngress>().is_some())
                {
                    trusted.extend(client_ip::parse_ranges(&["*".to_owned()])?);
                }
            }
        }
        Ok(Self {
            peer,
            trusted,
            headers: headers.clone(),
            mask,
            native_secure,
            platform_host,
        })
    }

    fn has_trusted_peer(&self) -> bool {
        self.peer
            .is_some_and(|peer| self.trusted.iter().any(|range| range.contains(peer)))
    }

    pub(crate) fn client_ips(&self) -> Result<Vec<IpAddr>> {
        let Some(peer) = self.peer else {
            return Ok(Vec::new());
        };
        if !self.has_trusted_peer() {
            return Ok(vec![peer]);
        }
        let x = values(
            &self.headers,
            "x-forwarded-for",
            self.mask & TrustedProxyHeaders::FOR != 0,
        )?;
        let rfc = self
            .forwarded()?
            .into_iter()
            .filter_map(|entry| entry.for_ip)
            .collect::<Vec<_>>();
        let x = if x.is_empty() {
            Vec::new()
        } else {
            client_ip::filter_client_ip_strings(&x, peer, &self.trusted)
        };
        let rfc = if rfc.is_empty() {
            Vec::new()
        } else {
            client_ip::filter_client_ip_strings(&rfc, peer, &self.trusted)
        };
        if !x.is_empty() && !rfc.is_empty() && x != rfc {
            return Err(invalid_forwarded());
        }
        let selected = if !rfc.is_empty() {
            rfc
        } else if !x.is_empty() {
            x
        } else {
            vec![peer.to_string()]
        };
        selected
            .into_iter()
            .map(|address| address.parse().map_err(Error::internal))
            .collect()
    }

    pub(crate) fn host(&self) -> Result<Option<(String, Option<i64>)>> {
        if !self.has_trusted_peer() {
            return Ok(None);
        }
        let x = values(
            &self.headers,
            "x-forwarded-host",
            self.mask & TrustedProxyHeaders::HOST != 0 && !self.platform_host,
        )?;
        let rfc = self
            .forwarded()?
            .into_iter()
            .filter_map(|entry| entry.host)
            .collect::<Vec<_>>();
        let selected = first_matching(&x, &rfc)?;
        selected.map(parse_trusted_host).transpose()
    }

    pub(crate) fn port(&self) -> Result<Option<i64>> {
        if !self.has_trusted_peer() {
            return Ok(None);
        }
        let x = values(
            &self.headers,
            "x-forwarded-port",
            self.mask & TrustedProxyHeaders::PORT != 0,
        )?
        .into_iter()
        .map(|value| format!("0.0.0.0:{value}"))
        .collect::<Vec<_>>();
        let rfc = self
            .forwarded()?
            .into_iter()
            .filter_map(|entry| entry.host)
            .map(|host| -> Result<String> {
                let suffix = if host.ends_with(']') {
                    None
                } else {
                    host.rfind(':').map(|index| &host[index..])
                };
                let suffix = match suffix {
                    Some(suffix) => suffix,
                    None if self.secure()? => ":443",
                    None => ":80",
                };
                Ok(format!("0.0.0.0{suffix}"))
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(selected) = first_matching(&x, &rfc)? {
            return Ok(Some(port_from_host(selected, self)?));
        }
        let x_host = values(
            &self.headers,
            "x-forwarded-host",
            self.mask & TrustedProxyHeaders::HOST != 0 && !self.platform_host,
        )?;
        let rfc_host = self
            .forwarded()?
            .into_iter()
            .filter_map(|entry| entry.host)
            .collect::<Vec<_>>();
        first_matching(&x_host, &rfc_host)?
            .map(|host| port_from_host(host, self))
            .transpose()
    }

    pub(crate) fn secure(&self) -> Result<bool> {
        if !self.has_trusted_peer() {
            return Ok(self.native_secure);
        }
        let x = values(
            &self.headers,
            "x-forwarded-proto",
            self.mask & TrustedProxyHeaders::PROTO != 0,
        )?;
        let rfc = self
            .forwarded()?
            .into_iter()
            .filter_map(|entry| entry.proto)
            .collect::<Vec<_>>();
        let selected = first_matching(&x, &rfc)?;
        Ok(selected.map_or(self.native_secure, |value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "https" | "on" | "ssl" | "1"
            )
        }))
    }

    pub(crate) fn prefix(&self) -> Result<String> {
        if !self.has_trusted_peer() {
            return Ok(String::new());
        }
        let x = values(
            &self.headers,
            "x-forwarded-prefix",
            self.mask & TrustedProxyHeaders::PREFIX != 0,
        )?;
        Ok(x.first()
            .map_or_else(String::new, |value| value.trim_end_matches('/').to_owned()))
    }

    fn forwarded(&self) -> Result<Vec<ForwardedEntry>> {
        if self.mask & TrustedProxyHeaders::FORWARDED == 0 {
            return Ok(Vec::new());
        }
        parse_forwarded(&self.headers)
    }
}

pub(crate) fn validate_headers(mask: u8) -> Result<()> {
    if mask & !0b11_1111 != 0 {
        return Err(Error::Config(crate::config::ConfigError::Invalid(
            "server.trusted_proxy_headers contains unsupported bits".into(),
        )));
    }
    Ok(())
}

pub(crate) fn platform_host(headers: &HeaderMap, uri: &Uri) -> bool {
    authority::authority_with_port(headers, uri)
        .is_ok_and(|(host, _)| host.ends_with(".on-forge.com") || host.ends_with(".on-vapor.com"))
}

fn values(headers: &HeaderMap, name: &str, enabled: bool) -> Result<Vec<String>> {
    if !enabled {
        return Ok(Vec::new());
    }
    let Some(header) = headers.get(name) else {
        return Ok(Vec::new());
    };
    let header = header.to_str().map_err(|_| invalid_forwarded())?;
    Ok(header
        .split(',')
        .map(|value| value.trim().to_owned())
        .collect())
}

fn first_matching<'a>(x: &'a [String], rfc: &'a [String]) -> Result<Option<&'a str>> {
    if !x.is_empty() && !rfc.is_empty() && x != rfc {
        return Err(invalid_forwarded());
    }
    Ok(rfc.first().or_else(|| x.first()).map(String::as_str))
}

#[derive(Default)]
struct ForwardedEntry {
    for_ip: Option<String>,
    host: Option<String>,
    proto: Option<String>,
}

fn parse_forwarded(headers: &HeaderMap) -> Result<Vec<ForwardedEntry>> {
    let Some(raw) = headers.get("forwarded") else {
        return Ok(Vec::new());
    };
    let raw = raw.to_str().map_err(|_| invalid_forwarded())?;
    let mut entries = Vec::new();
    for entry in split_quoted(raw, ',')? {
        let mut parsed = ForwardedEntry::default();
        for pair in split_quoted(entry, ';')? {
            let (key, value) = pair.trim().split_once('=').ok_or_else(invalid_forwarded)?;
            let value = unquote_forwarded(value.trim());
            match key.to_ascii_lowercase().as_str() {
                "for" => parsed.for_ip = Some(value.clone()),
                "host" => parsed.host = Some(value.clone()),
                "proto" => parsed.proto = Some(value),
                _ => {}
            }
        }
        entries.push(parsed);
    }
    Ok(entries)
}

fn unquote_forwarded(raw: &str) -> String {
    let mut value = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => {}
            '\\' => {
                if let Some(next) = characters.next() {
                    value.push(next);
                }
            }
            _ => value.push(character),
        }
    }
    value
}

fn split_quoted(raw: &str, delimiter: char) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (position, character) in raw.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == delimiter && !quoted {
            parts.push(&raw[start..position]);
            start = position + character.len_utf8();
        }
    }
    if quoted || escaped {
        return Err(invalid_forwarded());
    }
    parts.push(&raw[start..]);
    Ok(parts)
}

fn invalid_forwarded() -> Error {
    Error::BadRequest("invalid trusted forwarding header".into())
}

fn port_from_host(host: &str, context: &ProxyContext) -> Result<i64> {
    let separator = if host.starts_with('[') {
        host.rfind(']')
            .and_then(|end| host[end + 1..].find(':').map(|offset| end + 1 + offset))
    } else {
        host.rfind(':')
    };
    let value = separator.map_or("", |index| &host[index + 1..]);
    if value.is_empty() || value == "0" {
        return Ok(if context.secure()? { 443 } else { 80 });
    }
    Ok(php_port(value))
}

fn parse_trusted_host(value: &str) -> Result<(String, Option<i64>)> {
    let suffix = value.rsplit_once(':').filter(|(_, suffix)| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    });
    let (host, port) = suffix.map_or((value, None), |(host, port)| (host, Some(php_port(port))));
    let (host, _) = authority::parse_authority(host)?;
    Ok((host, port))
}

// PHP `(int)` 对数值字符串截去小数部分；这里的截断是公开端口语义。
#[allow(clippy::cast_possible_truncation)]
fn php_numeric_i64(number: f64) -> i64 {
    number as i64
}

fn php_port(value: &str) -> i64 {
    let value = value.trim_start();
    let bytes = value.as_bytes();
    let mut end = usize::from(matches!(bytes.first().copied(), Some(b'+' | b'-')));
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut decimal = false;
    if bytes.get(end) == Some(&b'.') {
        decimal = true;
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
    }
    if bytes
        .get(end)
        .is_some_and(|byte| matches!(*byte, b'e' | b'E'))
    {
        let exponent = end;
        end += 1;
        if bytes
            .get(end)
            .is_some_and(|byte| matches!(*byte, b'+' | b'-'))
        {
            end += 1;
        }
        let digits = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == digits {
            end = exponent;
        } else {
            decimal = true;
        }
    }
    let numeric = &value[..end];
    if decimal {
        numeric.parse::<f64>().map_or(0, php_numeric_i64)
    } else {
        numeric
            .parse::<i64>()
            .unwrap_or_else(|_| numeric.parse::<f64>().map_or(0, php_numeric_i64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(
        peer: &str,
        headers: &HeaderMap,
        config: &ServerConfig,
        tls: bool,
    ) -> Result<ProxyContext> {
        let mut extensions = Extensions::new();
        let peer = format!("{peer}:8080")
            .parse::<SocketAddr>()
            .map_err(Error::internal)?;
        extensions.insert(ConnectInfo(peer));
        extensions.insert(RequestSecurity::new(tls));
        ProxyContext::resolve(
            headers,
            &"/path".parse().map_err(Error::internal)?,
            &extensions,
            Some(config),
        )
    }

    fn trusted_config(range: &str) -> ServerConfig {
        ServerConfig {
            trusted_proxy_ranges: Some(vec![range.into()]),
            ..ServerConfig::default()
        }
    }

    fn headers(values: &[(&str, &str)]) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        for &(name, value) in values {
            headers.insert(
                http::header::HeaderName::from_bytes(name.as_bytes()).map_err(Error::internal)?,
                value.parse().map_err(Error::internal)?,
            );
        }
        Ok(headers)
    }

    #[test]
    fn untrusted_peer_ignores_all_forwarding_facts() -> Result<()> {
        let config = ServerConfig::default();
        let parsed = context(
            "203.0.113.8",
            &headers(&[
                ("host", "internal.test"),
                ("x-forwarded-for", "198.51.100.1"),
                ("x-forwarded-proto", "https"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(
            parsed.client_ips()?,
            ["203.0.113.8".parse::<IpAddr>().map_err(Error::internal)?]
        );
        assert!(!parsed.secure()?);
        assert_eq!(parsed.host()?, None);
        Ok(())
    }

    #[test]
    fn trusted_proto_overrides_native_tls_including_empty_value() -> Result<()> {
        let mut config = trusted_config("REMOTE_ADDR");
        for (proto, expected) in [("http", false), ("https", true), ("", false)] {
            let parsed = context(
                "10.0.0.1",
                &headers(&[("host", "internal.test"), ("x-forwarded-proto", proto)])?,
                &config,
                true,
            )?;
            assert_eq!(parsed.secure()?, expected);
        }
        config.trusted_proxy_headers = TrustedProxyHeaders::FOR;
        assert!(
            context(
                "10.0.0.1",
                &headers(&[("host", "internal.test"), ("x-forwarded-proto", "http")])?,
                &config,
                true
            )?
            .secure()?
        );
        Ok(())
    }

    #[test]
    fn star_trusts_both_ip_families_and_mask_controls_origin() -> Result<()> {
        let mut config = trusted_config("**");
        let parsed = context(
            "[2001:db8::1]",
            &headers(&[
                ("host", "internal.test:8080"),
                ("x-forwarded-for", "198.51.100.7"),
                ("x-forwarded-host", "public.test"),
                ("x-forwarded-port", "8443"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-prefix", "/gateway"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(
            parsed.client_ips()?,
            ["198.51.100.7".parse::<IpAddr>().map_err(Error::internal)?]
        );
        assert_eq!(parsed.host()?, Some(("public.test".into(), None)));
        assert_eq!(parsed.port()?, Some(8443));
        assert_eq!(parsed.prefix()?, "/gateway");
        assert!(parsed.secure()?);
        config.trusted_proxy_headers = TrustedProxyHeaders::AWS_ELB;
        let parsed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("x-forwarded-host", "public.test"),
                ("x-forwarded-prefix", "/gateway"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(parsed.host()?, None);
        assert_eq!(parsed.prefix()?, "");
        Ok(())
    }

    #[test]
    fn conflicting_forwarded_host_does_not_hide_independent_prefix() -> Result<()> {
        let mut config = trusted_config("REMOTE_ADDR");
        config.trusted_proxy_headers =
            TrustedProxyHeaders::DEFAULT | TrustedProxyHeaders::FORWARDED;
        let parsed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("forwarded", "for=198.51.100.2;host=rfc.test;proto=https"),
                ("x-forwarded-host", "other.test"),
                ("x-forwarded-prefix", "/edge"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(parsed.prefix()?, "/edge");
        assert!(parsed.host().is_err());
        assert!(parsed.secure()?);
        Ok(())
    }

    #[test]
    fn quoted_forwarded_pairs_decode_escaped_characters() -> Result<()> {
        let mut config = trusted_config("REMOTE_ADDR");
        config.trusted_proxy_headers =
            TrustedProxyHeaders::DEFAULT | TrustedProxyHeaders::FORWARDED;
        let parsed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                (
                    "forwarded",
                    r#"for="198\.51.100.2";proto="h\ttps";host="public\.test""#,
                ),
                ("x-forwarded-for", "198.51.100.2"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-host", "public.test"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(
            parsed.client_ips()?,
            ["198.51.100.2".parse::<IpAddr>().map_err(Error::internal)?]
        );
        assert!(parsed.secure()?);
        assert_eq!(parsed.host()?, Some(("public.test".into(), None)));
        Ok(())
    }

    #[test]
    fn forwarded_client_ip_conflict_preserves_ipv6_text() -> Result<()> {
        let mut config = trusted_config("REMOTE_ADDR");
        config.trusted_proxy_headers = TrustedProxyHeaders::FOR | TrustedProxyHeaders::FORWARDED;
        let parsed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("forwarded", r#"for="[2001:0DB8:0:0:0:0:0:1]""#),
                ("x-forwarded-for", "2001:db8::1"),
            ])?,
            &config,
            false,
        )?;
        assert!(parsed.client_ips().is_err());
        Ok(())
    }

    #[test]
    fn ip_port_normalization_precedes_forwarded_conflict_check() -> Result<()> {
        let mut config = trusted_config("REMOTE_ADDR");
        config.trusted_proxy_headers = TrustedProxyHeaders::FOR | TrustedProxyHeaders::FORWARDED;
        let parsed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("forwarded", "for=198.51.100.7:123"),
                ("x-forwarded-for", "198.51.100.7"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(
            parsed.client_ips()?,
            ["198.51.100.7".parse::<IpAddr>().map_err(Error::internal)?]
        );
        Ok(())
    }

    #[test]
    fn published_snapshot_keeps_original_headers_after_mutation() -> Result<()> {
        let config = trusted_config("REMOTE_ADDR");
        let mut extensions = Extensions::new();
        extensions.insert(ConnectInfo(
            "10.0.0.1:8080"
                .parse::<SocketAddr>()
                .map_err(Error::internal)?,
        ));
        let mut headers = headers(&[
            ("host", "internal.test"),
            ("x-forwarded-for", "198.51.100.7"),
        ])?;
        let snapshot = ProxySnapshot::capture(
            &headers,
            &"/".parse().map_err(Error::internal)?,
            &extensions,
            Some(&config),
        );
        extensions.insert(snapshot.clone());
        headers.insert(
            "x-forwarded-for",
            "203.0.113.9".parse().map_err(Error::internal)?,
        );
        let published = extensions
            .get::<ProxySnapshot>()
            .ok_or_else(|| Error::BadRequest("missing proxy snapshot".into()))?;
        let first = snapshot
            .0
            .as_ref()
            .map_err(|_| Error::BadRequest("bad snapshot".into()))?;
        let second = published
            .0
            .as_ref()
            .map_err(|_| Error::BadRequest("bad snapshot".into()))?;
        assert!(Arc::ptr_eq(first, second));
        assert_eq!(
            published.context()?.client_ips()?,
            ["198.51.100.7".parse::<IpAddr>().map_err(Error::internal)?]
        );
        Ok(())
    }

    #[test]
    fn invalid_trusted_host_does_not_block_independent_port_or_prefix() -> Result<()> {
        let config = trusted_config("REMOTE_ADDR");
        let parsed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("x-forwarded-host", "bad/host"),
                ("x-forwarded-prefix", "/edge"),
            ])?,
            &config,
            false,
        )?;
        assert!(parsed.host().is_err());
        assert_eq!(parsed.port()?, Some(80));
        assert_eq!(parsed.prefix()?, "/edge");
        assert!(!parsed.secure()?);
        Ok(())
    }

    #[test]
    fn trusted_host_port_is_not_limited_by_listener_u16() -> Result<()> {
        let config = trusted_config("REMOTE_ADDR");
        let parsed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("x-forwarded-host", "public.test:65536"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(parsed.host()?, Some(("public.test".into(), Some(65536))));
        assert_eq!(parsed.port()?, Some(65536));
        Ok(())
    }

    #[test]
    fn port_lists_compare_before_numeric_conversion_and_preserve_php_values() -> Result<()> {
        let mut config = trusted_config("REMOTE_ADDR");
        for (raw, expected) in [
            ("65536", 65536),
            ("-1", -1),
            ("not-port", 0),
            ("1e3", 1000),
            ("80.5", 80),
            ("80oops", 80),
            ("0", 443),
            ("", 443),
        ] {
            let parsed = context(
                "10.0.0.1",
                &headers(&[("host", "internal.test"), ("x-forwarded-port", raw)])?,
                &config,
                true,
            )?;
            assert_eq!(parsed.port()?, Some(expected), "{raw}");
        }
        config.trusted_proxy_headers |= TrustedProxyHeaders::FORWARDED;
        for raw in ["080", "80, 81"] {
            let parsed = context(
                "10.0.0.1",
                &headers(&[
                    ("host", "internal.test"),
                    ("x-forwarded-port", raw),
                    ("forwarded", "host=rfc.test:80, host=rfc.test:82"),
                ])?,
                &config,
                false,
            )?;
            assert!(parsed.port().is_err(), "{raw}");
            assert_eq!(parsed.prefix()?, "");
        }
        Ok(())
    }

    #[test]
    fn mapped_ipv6_bare_address_survives_but_bracketed_form_matches_php_filter() -> Result<()> {
        let config = trusted_config("REMOTE_ADDR");
        let bare = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("x-forwarded-for", "::ffff:198.51.100.7"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(
            bare.client_ips()?,
            ["::ffff:198.51.100.7"
                .parse::<IpAddr>()
                .map_err(Error::internal)?]
        );
        let bracketed = context(
            "10.0.0.1",
            &headers(&[
                ("host", "internal.test"),
                ("x-forwarded-for", "[::ffff:198.51.100.7]"),
            ])?,
            &config,
            false,
        )?;
        assert_eq!(
            bracketed.client_ips()?,
            ["10.0.0.1".parse::<IpAddr>().map_err(Error::internal)?]
        );
        Ok(())
    }

    #[test]
    fn explicit_empty_ranges_disable_forge_auto_trust() -> Result<()> {
        let mut config = ServerConfig::default();
        let values = headers(&[
            ("host", "app.on-forge.com"),
            ("x-forwarded-for", "198.51.100.7"),
        ])?;
        assert_eq!(
            context("10.0.0.1", &values, &config, false)?.client_ips()?,
            ["198.51.100.7".parse::<IpAddr>().map_err(Error::internal)?]
        );
        config.trusted_proxy_ranges = Some(Vec::new());
        assert_eq!(
            context("10.0.0.1", &values, &config, false)?.client_ips()?,
            ["10.0.0.1".parse::<IpAddr>().map_err(Error::internal)?]
        );
        Ok(())
    }

    #[test]
    fn cloud_ingress_trusts_only_when_ranges_are_unconfigured() -> Result<()> {
        let mut extensions = Extensions::new();
        extensions.insert(ConnectInfo(
            "10.0.0.1:8080"
                .parse::<SocketAddr>()
                .map_err(Error::internal)?,
        ));
        extensions.insert(CloudIngress);
        let headers = headers(&[
            ("host", "ordinary.test"),
            ("x-forwarded-for", "198.51.100.8"),
            ("x-forwarded-host", "public.test"),
        ])?;
        let uri = "/path".parse().map_err(Error::internal)?;
        let mut config = ServerConfig::default();
        let origin = ProxyContext::resolve(&headers, &uri, &extensions, Some(&config))?;
        assert_eq!(
            origin.client_ips()?,
            ["198.51.100.8".parse::<IpAddr>().map_err(Error::internal)?]
        );
        assert_eq!(origin.host()?, Some(("public.test".into(), None)));
        config.trusted_proxy_ranges = Some(Vec::new());
        let origin = ProxyContext::resolve(&headers, &uri, &extensions, Some(&config))?;
        assert_eq!(
            origin.client_ips()?,
            ["10.0.0.1".parse::<IpAddr>().map_err(Error::internal)?]
        );
        assert_eq!(origin.host()?, None);
        Ok(())
    }

    #[test]
    fn malformed_trusted_port_preserves_php_zero_result() -> Result<()> {
        let config = trusted_config("REMOTE_ADDR");
        let parsed = context(
            "10.0.0.1",
            &headers(&[("host", "internal.test"), ("x-forwarded-port", "invalid")])?,
            &config,
            false,
        )?;
        assert_eq!(parsed.port()?, Some(0));
        assert_eq!(php_port("12trailing"), 12);
        Ok(())
    }
}
