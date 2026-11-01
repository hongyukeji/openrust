//! 域路由与请求检查共用的 Host/URI authority 校验。

use crate::http::domain::DomainPattern;
use axum::http::{StatusCode, header::HOST};

pub(crate) fn authority(
    headers: &axum::http::HeaderMap,
    uri: &axum::http::Uri,
) -> crate::core::Result<String> {
    authority_with_port(headers, uri).map(|(host, _)| host)
}

pub(crate) fn authority_with_port(
    headers: &axum::http::HeaderMap,
    uri: &axum::http::Uri,
) -> crate::core::Result<(String, Option<u16>)> {
    let mut hosts = headers.get_all(HOST).iter();
    let header = hosts.next();
    if hosts.next().is_some() {
        return Err(authority_error());
    }
    let header = header
        .map(|value| value.to_str().map_err(|_| authority_error()))
        .transpose()?;
    let uri_authority = uri.authority().map(axum::http::uri::Authority::as_str);
    let selected = header.or(uri_authority).ok_or_else(authority_error)?;
    let (host, port) = parse_authority(selected)?;
    if let (Some(header), Some(uri_authority)) = (header, uri_authority)
        && parse_authority(header)? != parse_authority(uri_authority)?
    {
        return Err(authority_error());
    }
    Ok((host, port))
}

pub(crate) fn parse_authority(value: &str) -> crate::core::Result<(String, Option<u16>)> {
    use std::str::FromStr;
    if value.is_empty()
        || value.contains(',')
        || value.contains('@')
        || value.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(authority_error());
    }
    let parsed = axum::http::uri::Authority::from_str(value).map_err(|_| authority_error())?;
    let host = parsed.host();
    if host.is_empty() || host.ends_with('.') || !host.is_ascii() {
        return Err(authority_error());
    }
    let port = parsed.port_u16();
    if if value.starts_with('[') {
        let Some((address, suffix)) = value.split_once(']') else {
            return Err(authority_error());
        };
        address[1..].parse::<std::net::Ipv6Addr>().is_err()
            || (!suffix.is_empty() && (!suffix.starts_with(':') || port.is_none()))
    } else {
        (value.contains(':') && port.is_none())
            || (host.parse::<std::net::IpAddr>().is_err() && DomainPattern::parse(host).is_err())
    } {
        return Err(authority_error());
    }
    if port == Some(0) || value.ends_with(':') {
        return Err(authority_error());
    }
    let normalized = if let Some(address) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        format!(
            "[{}]",
            address
                .parse::<std::net::Ipv6Addr>()
                .map_err(|_| authority_error())?
        )
    } else {
        host.to_ascii_lowercase()
    };
    Ok((normalized, port))
}

fn authority_error() -> crate::core::Error {
    crate::core::Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "http_domain_authority_invalid",
        "request authority must contain one valid Host matching the URI authority",
    )
}
