//! 支持可信代理的客户端地址提取。

use std::net::{IpAddr, SocketAddr};

use crate::core::{AppState, Error, Result};
use axum::extract::{FromRequestParts, Request, connect_info::ConnectInfo};
use http::{HeaderMap, header::HeaderName, request::Parts};

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// 根据 socket 对端地址和已配置的可信代理解析出的请求客户端 IP。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientIp(IpAddr);

impl ClientIp {
    /// 返回为此请求确定的客户端 IP。
    pub const fn ip(self) -> IpAddr {
        self.0
    }

    pub(crate) fn from_request(request: &Request, state: &AppState) -> Result<Self> {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .ok_or_else(|| Error::internal(MissingConnectInfo))?
            .0
            .ip();
        let ranges = parse_ranges(&state.config().server.trusted_proxy_ranges)?;
        resolve_client_ip(peer, request.headers(), &ranges).map(Self)
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Error;

    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .ok_or_else(|| Error::internal(MissingConnectInfo))?
            .0
            .ip();
        let ranges = parse_ranges(&state.config().server.trusted_proxy_ranges)?;
        resolve_client_ip(peer, &parts.headers, &ranges).map(Self)
    }
}

/// 在服务器开始接受请求前校验可信代理配置。
pub(crate) fn validate_trusted_proxy_ranges(ranges: &[String]) -> Result<()> {
    parse_ranges(ranges).map(|_| ())
}

fn resolve_client_ip(peer: IpAddr, headers: &HeaderMap, trusted: &[IpRange]) -> Result<IpAddr> {
    if !trusted.iter().any(|range| range.contains(peer)) {
        return Ok(peer);
    }
    let mut chain = Vec::new();
    for header in headers.get_all(&X_FORWARDED_FOR) {
        let value = header
            .to_str()
            .map_err(|_| Error::BadRequest("invalid X-Forwarded-For header".into()))?;
        for entry in value.split(',') {
            let address = entry
                .trim()
                .parse::<IpAddr>()
                .map_err(|_| Error::BadRequest("invalid X-Forwarded-For header".into()))?;
            chain.push(address);
        }
    }

    let mut client = peer;
    for hop in chain.into_iter().rev() {
        if !trusted.iter().any(|range| range.contains(client)) {
            break;
        }
        client = hop;
    }
    Ok(client)
}

#[derive(Clone, Copy, Debug)]
struct IpRange {
    address: IpAddr,
    prefix: u8,
}

impl IpRange {
    fn parse(value: &str) -> Result<Self> {
        let invalid = || {
            Error::Config(crate::config::ConfigError::Invalid(
                "server.trusted_proxy_ranges must contain IP addresses or valid CIDRs".into(),
            ))
        };
        let (address, prefix) = if let Some((address, prefix)) = value.split_once('/') {
            let address = address.parse::<IpAddr>().map_err(|_| invalid())?;
            let prefix = prefix.parse::<u8>().map_err(|_| invalid())?;
            let maximum = if address.is_ipv4() { 32 } else { 128 };
            if prefix > maximum {
                return Err(invalid());
            }
            (address, prefix)
        } else {
            let address = value.parse::<IpAddr>().map_err(|_| invalid())?;
            let prefix = if address.is_ipv4() { 32 } else { 128 };
            (address, prefix)
        };
        Ok(Self { address, prefix })
    }

    fn contains(self, address: IpAddr) -> bool {
        match (self.address, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32_u8 - self.prefix)
                };
                (u32::from(network) & mask) == (u32::from(address) & mask)
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u128::MAX << (128_u8 - self.prefix)
                };
                (u128::from(network) & mask) == (u128::from(address) & mask)
            }
            _ => false,
        }
    }
}

fn parse_ranges(values: &[String]) -> Result<Vec<IpRange>> {
    values.iter().map(|value| IpRange::parse(value)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn untrusted_peers_cannot_supply_a_forwarded_address() -> Result<()> {
        let peer = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 4));
        let headers = HeaderMap::from_iter([(
            X_FORWARDED_FOR.clone(),
            "198.51.100.9".parse().map_err(Error::internal)?,
        )]);
        assert_eq!(resolve_client_ip(peer, &headers, &[])?, peer);
        Ok(())
    }

    #[test]
    fn trusted_proxy_chain_is_walked_from_the_peer_toward_the_client() -> Result<()> {
        let peer = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let trusted = parse_ranges(&["10.0.0.0/8".to_owned()])?;
        let headers = HeaderMap::from_iter([(
            X_FORWARDED_FOR.clone(),
            "198.51.100.9, 10.0.0.2".parse().map_err(Error::internal)?,
        )]);
        assert_eq!(
            resolve_client_ip(peer, &headers, &trusted)?,
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9))
        );
        Ok(())
    }

    #[test]
    fn spoofed_prefix_before_the_client_is_not_selected() -> Result<()> {
        let peer = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let trusted = parse_ranges(&["10.0.0.0/8".to_owned()])?;
        let headers = HeaderMap::from_iter([(
            X_FORWARDED_FOR.clone(),
            "192.0.2.99, 198.51.100.9, 10.0.0.2"
                .parse()
                .map_err(Error::internal)?,
        )]);
        assert_eq!(
            resolve_client_ip(peer, &headers, &trusted)?,
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9))
        );
        Ok(())
    }

    #[test]
    fn cidr_boundaries_support_ipv4_ipv6_and_reject_invalid_prefixes() -> Result<()> {
        let ipv4 = IpRange::parse("192.0.2.0/24")?;
        assert!(ipv4.contains("192.0.2.255".parse().map_err(Error::internal)?));
        assert!(!ipv4.contains("192.0.3.0".parse().map_err(Error::internal)?));
        let ipv6 = IpRange::parse("2001:db8::/32")?;
        assert!(ipv6.contains("2001:db8::1".parse().map_err(Error::internal)?));
        assert!(!ipv6.contains("2001:db9::1".parse().map_err(Error::internal)?));
        assert!(IpRange::parse("192.0.2.1/33").is_err());
        Ok(())
    }

    #[test]
    fn malformed_forwarding_header_is_rejected_only_for_trusted_peers() -> Result<()> {
        let trusted_peer = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let trusted = parse_ranges(&["10.0.0.0/8".to_owned()])?;
        let headers = HeaderMap::from_iter([(
            X_FORWARDED_FOR.clone(),
            "not-an-ip".parse().map_err(Error::internal)?,
        )]);
        assert!(resolve_client_ip(trusted_peer, &headers, &trusted).is_err());
        assert_eq!(
            resolve_client_ip(
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, 4)),
                &headers,
                &trusted
            )?,
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 4))
        );
        Ok(())
    }
}

#[derive(Debug)]
struct MissingConnectInfo;

impl std::fmt::Display for MissingConnectInfo {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HTTP request is missing socket ConnectInfo")
    }
}

impl std::error::Error for MissingConnectInfo {}
