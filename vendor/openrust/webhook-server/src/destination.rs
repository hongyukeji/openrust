use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use url::{Host, Url};

use crate::{Error, Result};

const MAX_ADDRESSES: usize = 64;
pub(crate) const MAX_URL_BYTES: usize = 8192;

/// 应用选择的出站目的地规则。默认要求 HTTPS 与公开地址。
#[derive(Default)]
pub struct TargetPolicy {
    trusted: Option<(Url, Vec<SocketAddr>)>,
}

/// 一次尝试获准连接的目标与地址。DNS 结果不跨尝试持久化。
pub struct BoundTarget {
    url: Url,
    addresses: Vec<SocketAddr>,
}

impl BoundTarget {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn addresses(&self) -> &[SocketAddr] {
        &self.addresses
    }
}

impl TargetPolicy {
    /// 允许来自应用可信配置的固定来源、路径及指定地址，供内部端点使用。
    pub fn trusted_endpoint(url: &str, addresses: Vec<SocketAddr>) -> Result<Self> {
        let url = parse_target(url)?;
        validate_addresses(&url, &addresses)?;
        Ok(Self {
            trusted: Some((url, addresses)),
        })
    }

    pub(crate) fn validate_target(&self, url: &Url) -> Result<()> {
        if let Some((endpoint, _)) = &self.trusted {
            return if url.origin() == endpoint.origin() && url.path() == endpoint.path() {
                Ok(())
            } else {
                Err(Error::TargetDenied)
            };
        }
        if url.scheme() != "https" || literal_ip(url).is_some_and(|ip| !public_ip(ip)) {
            return Err(Error::TargetDenied);
        }
        Ok(())
    }

    /// 复核解析出的全部地址。调用者应把结果交给固定地址的 HTTP 传输。
    pub fn bind(&self, url: &str, addresses: Vec<SocketAddr>) -> Result<BoundTarget> {
        let url = parse_target(url)?;
        self.validate_target(&url)?;
        validate_addresses(&url, &addresses)?;
        if !addresses.iter().all(|address| match &self.trusted {
            Some((_, allowed)) => allowed.contains(address),
            None => public_ip(address.ip()),
        }) {
            return Err(Error::TargetDenied);
        }
        Ok(BoundTarget { url, addresses })
    }

    pub(crate) async fn resolve(&self, value: &str) -> Result<BoundTarget> {
        let url = parse_target(value)?;
        self.validate_target(&url)?;
        let port = url.port_or_known_default().ok_or(Error::TargetDenied)?;
        let addresses = if let Some((_, addresses)) = &self.trusted {
            addresses.clone()
        } else if let Some(ip) = literal_ip(&url) {
            vec![SocketAddr::new(ip, port)]
        } else {
            let Some(Host::Domain(domain)) = url.host() else {
                return Err(Error::TargetDenied);
            };
            tokio::net::lookup_host((domain, port))
                .await
                .map_err(|_| Error::TargetResolutionFailed)?
                .take(MAX_ADDRESSES + 1)
                .collect()
        };
        self.bind(value, addresses)
    }
}

pub(crate) fn parse_target(value: &str) -> Result<Url> {
    if value.len() > MAX_URL_BYTES
        || value.contains('\\')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || !value.contains("://")
    {
        return Err(Error::TargetDenied);
    }
    let url = Url::parse(value).map_err(|_| Error::TargetDenied)?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default().is_none_or(|port| port == 0)
    {
        return Err(Error::TargetDenied);
    }
    Ok(url)
}

fn validate_addresses(url: &Url, addresses: &[SocketAddr]) -> Result<()> {
    let port = url.port_or_known_default().ok_or(Error::TargetDenied)?;
    let literal = literal_ip(url);
    if addresses.is_empty()
        || addresses.len() > MAX_ADDRESSES
        || addresses.iter().any(|addr| {
            addr.port() != port
                || addr.ip().is_unspecified()
                || addr.ip().is_multicast()
                || addr.ip() == Ipv4Addr::BROADCAST
                || literal.is_some_and(|ip| addr.ip() != ip)
        })
    {
        return Err(Error::TargetDenied);
    }
    Ok(())
}

fn literal_ip(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        Host::Ipv4(ip) => Some(ip.into()),
        Host::Ipv6(ip) => Some(ip.into()),
        Host::Domain(_) => None,
    }
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_ipv4(ip),
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map_or_else(|| public_ipv6(ip), public_ipv4),
    }
}

fn public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !matches!(
        (a, b, c),
        (0 | 10 | 127 | 224..=255, _, _)
            | (100, 64..=127, _)
            | (169, 254, _)
            | (172, 16..=31, _)
            | (192, 0, 0 | 2)
            | (192, 88, 99)
            | (192, 168, _)
            | (198, 18 | 19, _)
            | (198, 51, 100)
            | (203, 0, 113)
    )
}

fn public_ipv6(ip: Ipv6Addr) -> bool {
    let [a, b, ..] = ip.segments();
    // 只接受全局单播段，排除特殊协议、文档及嵌套 IPv4 的过渡地址。
    a & 0xe000 == 0x2000
        && !(a == 0x2001 && (b < 0x0200 || b == 0x0db8))
        && a != 0x2002
        && !(a == 0x3fff && b < 0x1000)
}
