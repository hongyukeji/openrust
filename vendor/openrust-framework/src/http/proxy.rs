use std::{
    net::{IpAddr, SocketAddr},
    sync::OnceLock,
};

use axum::extract::ConnectInfo;
use http::{HeaderMap, Uri, header::HOST, uri::Authority};

use super::{HttpError, Request};
use crate::{
    config::{ConfigError, Value},
    contracts::config::Repository,
};

/// Native transport security supplied by the server accepting the connection.
#[derive(Clone, Copy, Debug)]
pub struct RequestSecurity(pub bool);

/// Header flags accepted by the trusted proxy policy.
pub struct TrustedProxyHeaders;

impl TrustedProxyHeaders {
    pub const FORWARDED: u8 = 1;
    pub const FOR: u8 = 2;
    pub const HOST: u8 = 4;
    pub const PROTO: u8 = 8;
    pub const PORT: u8 = 16;
    pub const PREFIX: u8 = 32;
    pub const AWS_ELB: u8 = Self::FOR | Self::PROTO | Self::PORT;
    pub const DEFAULT: u8 = Self::FOR | Self::HOST | Self::PROTO | Self::PORT | Self::PREFIX;
}

#[derive(Default)]
pub(crate) struct ProxyRequest {
    pub(crate) trusted: bool,
    pub(crate) uri: Option<Uri>,
    pub(crate) secure: Option<bool>,
    pub(crate) ips: Option<Vec<IpAddr>>,
    pub(crate) prefix: String,
    pub(crate) peer: OnceLock<IpAddr>,
}

#[derive(Clone, Default)]
pub(crate) struct ProxyPolicy {
    ranges: Vec<(IpAddr, u8)>,
    trust_peer: bool,
    trust_all: bool,
    headers: u8,
}

impl ProxyPolicy {
    pub(crate) fn from_config(config: &dyn Repository) -> Result<Self, ConfigError> {
        let values = config.get("trustedproxy.proxies");
        let invalid = || {
            invalid_configuration(
                "trustedproxy.proxies",
                "a comma-separated string or an array of proxy addresses",
            )
        };
        let addresses: Vec<_> = match &values {
            None | Some(Value::Null) => return Ok(Self::default()),
            Some(Value::String(value)) => value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| value.as_str().ok_or_else(invalid))
                .collect::<Result<_, _>>()?,
            _ => return Err(invalid()),
        };
        use TrustedProxyHeaders as H;
        let headers = match config.get("trustedproxy.headers") {
            None | Some(Value::Null) => H::DEFAULT,
            Some(Value::String(name)) => match name.as_str() {
                "HEADER_FORWARDED" => H::FORWARDED,
                "HEADER_X_FORWARDED_FOR" => H::FOR,
                "HEADER_X_FORWARDED_HOST" => H::HOST,
                "HEADER_X_FORWARDED_PROTO" => H::PROTO,
                "HEADER_X_FORWARDED_PORT" => H::PORT,
                "HEADER_X_FORWARDED_PREFIX" => H::PREFIX,
                "HEADER_X_FORWARDED_AWS_ELB" => H::AWS_ELB,
                _ => H::DEFAULT,
            },
            Some(value) => value
                .as_u64()
                .and_then(|number| u8::try_from(number).ok())
                .ok_or_else(|| {
                    invalid_configuration("trustedproxy.headers", "a header mask or preset name")
                })?,
        };
        Self::new(&addresses, headers)
    }

    pub(crate) fn new(at: &[&str], headers: u8) -> Result<Self, ConfigError> {
        use TrustedProxyHeaders as H;
        if headers & !(H::DEFAULT | H::FORWARDED) != 0 {
            return Err(invalid_configuration(
                "trustedproxy.headers",
                "a supported proxy header mask",
            ));
        }
        let mut policy = Self {
            headers,
            ..Self::default()
        };
        for value in at.iter().map(|value| value.trim()) {
            match value {
                "*" | "**" => policy.trust_all = true,
                "REMOTE_ADDR" => policy.trust_peer = true,
                _ => {
                    let range = parse_range(value).ok_or_else(|| {
                        invalid_configuration(
                            "trustedproxy.proxies",
                            "IP addresses, CIDRs, REMOTE_ADDR or *",
                        )
                    })?;
                    policy.ranges.push(range);
                }
            }
        }
        Ok(policy)
    }

    fn trusted(&self, address: IpAddr, peer: IpAddr) -> bool {
        self.trust_all
            || (self.trust_peer && address == peer)
            || self
                .ranges
                .iter()
                .any(|&(network, prefix)| contains(network, prefix, address))
    }

    pub(crate) fn apply(&self, request: &mut Request) -> Result<(), HttpError> {
        let incoming = request.parts();
        let transport = incoming
            .extensions
            .get::<RequestSecurity>()
            .map(|value| value.0);
        let native_secure = transport.unwrap_or_else(|| incoming.uri.scheme_str() == Some("https"));
        let peer = incoming
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip().to_canonical());
        let trusted = peer.filter(|&peer| self.trusted(peer, peer));
        let mut effective = ProxyRequest {
            trusted: trusted.is_some(),
            ..Default::default()
        };
        if let Some(peer) = peer {
            let _ = effective.peer.set(peer);
        }
        let mut host = None;
        let mut port = None;
        let mut secure = native_secure;
        if let Some(peer) = trusted {
            use TrustedProxyHeaders as H;
            let headers = &incoming.headers;
            let mut rfc = Forwarded::default();
            if self.headers & H::FORWARDED != 0 {
                rfc = parse_forwarded(headers)?;
            }
            let values = |name, mask| header_values(headers, name, self.headers & mask != 0);
            let forwarded_for = values("x-forwarded-for", H::FOR)?;
            if !forwarded_for.is_empty() || !rfc.addresses.is_empty() {
                let filter = |values: &[String]| {
                    client_ips(values, peer, |address| self.trusted(address, peer))
                };
                let x = (!forwarded_for.is_empty()).then(|| filter(&forwarded_for));
                let f = (!rfc.addresses.is_empty()).then(|| filter(&rfc.addresses));
                effective.ips = choose(x, f)?;
            }
            let rfc_authority = rfc
                .hosts
                .first()
                .map(|value| parse_authority(value))
                .transpose()?;
            host = first_matching(values("x-forwarded-host", H::HOST)?, rfc.hosts)?;
            let scheme = first_matching(values("x-forwarded-proto", H::PROTO)?, rfc.protocols)?;
            if let Some(scheme) = scheme {
                secure = matches!(
                    scheme.to_ascii_lowercase().as_str(),
                    "https" | "on" | "ssl" | "1"
                );
            }
            let forwarded_ports = values("x-forwarded-port", H::PORT)?;
            if let Some(port_value) = forwarded_ports.first() {
                port = Some(
                    port_value
                        .parse::<u16>()
                        .ok()
                        .filter(|&port| port != 0)
                        .ok_or(HttpError::InvalidRequest)?,
                );
            }
            if let (Some(port), Some(authority)) = (port, rfc_authority)
                && port
                    != authority
                        .port_u16()
                        .unwrap_or(if secure { 443 } else { 80 })
            {
                return Err(HttpError::InvalidRequest);
            }
            if let Some(prefix) = values("x-forwarded-prefix", H::PREFIX)?.first() {
                effective.prefix = normalize_prefix(prefix)?;
            }
        }
        if trusted.is_some() || transport.is_some() {
            effective.secure = Some(secure);
            let forwarded_host = host.is_some();
            let authority = host
                .as_deref()
                .or_else(|| incoming.uri.authority().map(Authority::as_str))
                .or_else(|| incoming.headers.get(HOST)?.to_str().ok());
            if let Some(authority) = authority {
                let authority = parse_authority(authority)?;
                let authority = match port {
                    Some(port) if forwarded_host || authority.port_u16() != Some(port) => {
                        format!("{}:{port}", authority.host())
                            .parse::<Authority>()
                            .map_err(invalid_request)?
                    }
                    _ => authority,
                };
                if let Some(target) = incoming.uri.path_and_query() {
                    effective.uri = Some(
                        Uri::builder()
                            .scheme(if secure { "https" } else { "http" })
                            .authority(authority)
                            .path_and_query(target.clone())
                            .build()
                            .map_err(invalid_request)?,
                    );
                }
            }
        }
        request.proxy = effective;
        Ok(())
    }
}

fn parse_range(value: &str) -> Option<(IpAddr, u8)> {
    let (address, prefix) = value
        .split_once('/')
        .map_or((value, None), |(address, prefix)| (address, Some(prefix)));
    let address: IpAddr = address.parse().ok()?;
    let maximum = if address.is_ipv4() { 32 } else { 128 };
    let prefix = match prefix {
        Some(prefix) => prefix.parse().ok()?,
        None => maximum,
    };
    if prefix > maximum {
        return None;
    }
    let canonical = address.to_canonical();
    if canonical != address && prefix >= 96 {
        Some((canonical, prefix - 96))
    } else {
        Some((address, prefix))
    }
}

fn contains(network: IpAddr, prefix: u8, address: IpAddr) -> bool {
    match (network, address) {
        (IpAddr::V4(network), IpAddr::V4(address)) => {
            prefix == 0
                || (u32::from(network) ^ u32::from(address)).leading_zeros() >= u32::from(prefix)
        }
        (IpAddr::V6(network), IpAddr::V6(address)) => {
            prefix == 0
                || (u128::from(network) ^ u128::from(address)).leading_zeros() >= u32::from(prefix)
        }
        _ => false,
    }
}

fn header_values(
    headers: &HeaderMap,
    name: &'static str,
    enabled: bool,
) -> Result<Vec<String>, HttpError> {
    if !enabled {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for value in headers.get_all(name) {
        values.extend(
            value
                .to_str()
                .map_err(invalid_request)?
                .split(',')
                .map(|value| value.trim().to_owned()),
        );
    }
    Ok(values)
}

fn client_ips(values: &[String], peer: IpAddr, trusted: impl Fn(IpAddr) -> bool) -> Vec<IpAddr> {
    let mut first_trusted = None;
    let mut clients: Vec<_> = values
        .iter()
        .filter_map(|value| parse_ip(value))
        .chain([peer])
        .filter(|&address| {
            if trusted(address) {
                first_trusted.get_or_insert(address);
                false
            } else {
                true
            }
        })
        .collect();
    clients.reverse();
    if clients.is_empty() {
        clients.extend(first_trusted);
    }
    clients
}

fn parse_ip(value: &str) -> Option<IpAddr> {
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|address| address.ip()))
        .or_else(|| value.strip_prefix('[')?.strip_suffix(']')?.parse().ok())
        .map(|address| address.to_canonical())
}

fn first_matching(x: Vec<String>, rfc: Vec<String>) -> Result<Option<String>, HttpError> {
    let x = (!x.is_empty()).then_some(x);
    let rfc = (!rfc.is_empty()).then_some(rfc);
    Ok(choose(x, rfc)?.and_then(|values| values.into_iter().next()))
}

fn choose<T: PartialEq>(left: Option<T>, right: Option<T>) -> Result<Option<T>, HttpError> {
    if left
        .as_ref()
        .zip(right.as_ref())
        .is_some_and(|(left, right)| left != right)
    {
        return Err(HttpError::InvalidRequest);
    }
    Ok(right.or(left))
}

#[derive(Default)]
struct Forwarded {
    addresses: Vec<String>,
    hosts: Vec<String>,
    protocols: Vec<String>,
}

fn parse_forwarded(headers: &HeaderMap) -> Result<Forwarded, HttpError> {
    let mut forwarded = Forwarded::default();
    for value in headers.get_all("forwarded") {
        for entry in split_quoted(value.to_str().map_err(invalid_request)?, ',')? {
            let mut seen = 0;
            for pair in split_quoted(entry, ';')? {
                let (key, value) = pair
                    .trim()
                    .split_once('=')
                    .ok_or(HttpError::InvalidRequest)?;
                let (collection, flag) = match key.to_ascii_lowercase().as_str() {
                    "for" => (&mut forwarded.addresses, 1),
                    "host" => (&mut forwarded.hosts, 2),
                    "proto" => (&mut forwarded.protocols, 4),
                    _ => continue,
                };
                if seen & flag != 0 {
                    return Err(HttpError::InvalidRequest);
                }
                seen |= flag;
                let value = value.trim();
                if let Some(value) = value
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
                {
                    let mut characters = value.chars();
                    let mut decoded = String::new();
                    while let Some(character) = characters.next() {
                        decoded.push(if character == '\\' {
                            characters.next().ok_or(HttpError::InvalidRequest)?
                        } else {
                            character
                        });
                    }
                    collection.push(decoded);
                } else if !value.is_empty() && !value.contains(['"', '\\', ' ', '\t']) {
                    collection.push(value.into());
                } else {
                    return Err(HttpError::InvalidRequest);
                }
            }
        }
    }
    Ok(forwarded)
}

fn split_quoted(raw: &str, delimiter: char) -> Result<Vec<&str>, HttpError> {
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
        return Err(HttpError::InvalidRequest);
    }
    parts.push(&raw[start..]);
    Ok(parts)
}

pub(super) fn parse_authority(value: &str) -> Result<Authority, HttpError> {
    let authority = value.parse::<Authority>().map_err(invalid_request)?;
    let suffix = authority
        .as_str()
        .strip_prefix(authority.host())
        .ok_or(HttpError::InvalidRequest)?;
    if authority.host().is_empty()
        || value.contains(['@', '%', '\\'])
        || (!suffix.is_empty() && authority.port_u16().is_none())
    {
        return Err(HttpError::InvalidRequest);
    }
    Ok(authority)
}

fn normalize_prefix(value: &str) -> Result<String, HttpError> {
    if value.is_empty() || value == "/" {
        return Ok(String::new());
    }
    if !value.starts_with('/')
        || value.starts_with("//")
        || value.contains(['?', '#', '\\'])
        || value
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
        || value.parse::<http::uri::PathAndQuery>().is_err()
    {
        return Err(HttpError::InvalidRequest);
    }
    Ok(value.trim_end_matches('/').to_owned())
}

fn invalid_configuration(key: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.into(),
        expected,
    }
}

fn invalid_request(_: impl std::fmt::Display) -> HttpError {
    HttpError::InvalidRequest
}
