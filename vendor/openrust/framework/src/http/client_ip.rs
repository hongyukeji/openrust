//! 支持可信代理的客户端地址提取。

use std::net::IpAddr;

use crate::core::Error;
use crate::core::Result;
use crate::foundation::ApplicationState;
use axum::extract::{FromRequestParts, Request};
use http::request::Parts;

/// 根据 socket 对端地址和已配置的可信代理解析出的请求客户端 IP。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientIp(IpAddr);

impl ClientIp {
    /// 返回为此请求确定的客户端 IP。
    pub const fn ip(self) -> IpAddr {
        self.0
    }

    pub(crate) fn from_request(request: &Request, state: &ApplicationState) -> Result<Self> {
        request
            .extensions()
            .get::<crate::http::proxy::ProxySnapshot>()
            .cloned()
            .unwrap_or_else(|| {
                crate::http::proxy::ProxySnapshot::capture(
                    request.headers(),
                    request.uri(),
                    request.extensions(),
                    Some(&state.config().server),
                )
            })
            .context()?
            .client_ips()?
            .first()
            .copied()
            .map(Self)
            .ok_or_else(|| Error::internal(MissingConnectInfo))
    }
}

impl FromRequestParts<ApplicationState> for ClientIp {
    type Rejection = Error;

    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, state: &ApplicationState) -> Result<Self> {
        parts
            .extensions
            .get::<crate::http::proxy::ProxySnapshot>()
            .cloned()
            .unwrap_or_else(|| {
                crate::http::proxy::ProxySnapshot::capture(
                    &parts.headers,
                    &parts.uri,
                    &parts.extensions,
                    Some(&state.config().server),
                )
            })
            .context()?
            .client_ips()?
            .first()
            .copied()
            .map(Self)
            .ok_or_else(|| Error::internal(MissingConnectInfo))
    }
}

/// 在服务器开始接受请求前校验可信代理配置。
pub(crate) fn validate_trusted_proxy_ranges(ranges: Option<&[String]>) -> Result<()> {
    ranges.map_or(Ok(()), |ranges| parse_ranges(ranges).map(|_| ()))
}

#[cfg(test)]
pub(crate) fn filter_client_ips(
    chain: &[String],
    peer: IpAddr,
    trusted: &[IpRange],
) -> Vec<IpAddr> {
    filter_client_ip_strings(chain, peer, trusted)
        .into_iter()
        .filter_map(|address| address.parse().ok())
        .collect()
}

pub(crate) fn filter_client_ip_strings(
    chain: &[String],
    peer: IpAddr,
    trusted: &[IpRange],
) -> Vec<String> {
    let mut addresses = chain
        .iter()
        .filter_map(|value| {
            let value = value.trim();
            let value = if value.contains('.') {
                value
                    .find(':')
                    .filter(|index| *index > 0)
                    .map_or(value, |index| &value[..index])
            } else if let Some(rest) = value.strip_prefix('[') {
                rest.split_once(']').map_or(rest, |(address, _)| address)
            } else {
                value
            };
            value.parse::<IpAddr>().ok().map(|_| value.to_owned())
        })
        .collect::<Vec<_>>();
    addresses.push(peer.to_string());
    let mut first_trusted = None;
    addresses.retain(|address| {
        if address
            .parse()
            .is_ok_and(|parsed| trusted.iter().any(|range| range.contains(parsed)))
        {
            if first_trusted.is_none() {
                first_trusted = Some(address.clone());
            }
            false
        } else {
            true
        }
    });
    addresses.reverse();
    if addresses.is_empty() {
        first_trusted.into_iter().collect()
    } else {
        addresses
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct IpRange {
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

    pub(crate) fn contains(self, address: IpAddr) -> bool {
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

pub(crate) fn parse_ranges(values: &[String]) -> Result<Vec<IpRange>> {
    values
        .iter()
        .map(|value| match value.as_str() {
            "*" | "**" => Ok(vec![IpRange::parse("0.0.0.0/0")?, IpRange::parse("::/0")?]),
            "REMOTE_ADDR" => Ok(Vec::new()),
            _ => Ok(vec![IpRange::parse(value)?]),
        })
        .collect::<Result<Vec<_>>>()
        .map(|groups| groups.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn trusted_proxy_chain_is_walked_from_the_peer_toward_the_client() -> Result<()> {
        let peer = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let trusted = parse_ranges(&["10.0.0.0/8".to_owned()])?;
        let chain = ["198.51.100.9".to_owned(), "10.0.0.2".to_owned()];
        assert_eq!(
            filter_client_ips(&chain, peer, &trusted),
            [IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9))]
        );
        Ok(())
    }

    #[test]
    fn spoofed_prefix_before_the_client_is_not_selected() -> Result<()> {
        let peer = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let trusted = parse_ranges(&["10.0.0.0/8".to_owned()])?;
        let chain = [
            "192.0.2.99".to_owned(),
            "198.51.100.9".to_owned(),
            "10.0.0.2".to_owned(),
        ];
        assert_eq!(
            filter_client_ips(&chain, peer, &trusted),
            [
                IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9)),
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 99))
            ]
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
    fn invalid_forwarded_ip_is_filtered_but_valid_port_forms_survive() -> Result<()> {
        let trusted_peer = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let trusted = parse_ranges(&["10.0.0.0/8".to_owned()])?;
        let chain = [
            "not-an-ip".to_owned(),
            "198.51.100.9:123".to_owned(),
            "[2001:db8::1]:456".to_owned(),
        ];
        assert_eq!(
            filter_client_ips(&chain, trusted_peer, &trusted),
            [
                "2001:db8::1".parse::<IpAddr>().map_err(Error::internal)?,
                "198.51.100.9".parse::<IpAddr>().map_err(Error::internal)?
            ]
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
