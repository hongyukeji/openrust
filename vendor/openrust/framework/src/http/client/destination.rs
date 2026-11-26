use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use url::{Host, Origin, Url};

use super::{Error, Result};

/// 通过应用指定的数字地址连接 SOCKS5，目标只使用本地校验的地址。
pub struct PinnedSocksProxy(pub(super) reqwest::Proxy);

impl PinnedSocksProxy {
    pub fn new(address: SocketAddr) -> Result<Self> {
        proxy_at(address, "socks5").map(Self)
    }

    pub fn with_credentials(self, username: &str, password: &str) -> Result<Self> {
        validate_proxy_credentials(username, password)?;
        Ok(Self(self.0.basic_auth(username, password)))
    }
}

/// 通过应用指定的 HTTP 或 HTTPS 代理对已校验数字地址建立 CONNECT 隧道。
pub struct PinnedHttpProxy(reqwest::Proxy);

impl PinnedHttpProxy {
    pub fn new(address: SocketAddr) -> Result<Self> {
        proxy_at(address, "http").map(Self)
    }

    /// 用独立的服务名与信任根校验代理；TCP 连接始终使用指定地址。
    pub fn https(
        address: SocketAddr,
        server_name: &str,
        certificates: Vec<super::TlsCertificate>,
    ) -> Result<Self> {
        proxy_at(address, "https")?
            .with_connect_proxy_tls(server_name, certificates)
            .map(Self)
            .map_err(|_| Error::Configuration)
    }

    pub fn with_credentials(self, username: &str, password: &str) -> Result<Self> {
        validate_proxy_credentials(username, password)?;
        if username.contains(':') {
            return Err(Error::Configuration);
        }
        Ok(Self(self.0.basic_auth(username, password)))
    }
}

/// 显式授权的固定地址出站代理。
pub enum PinnedProxy {
    Socks5(PinnedSocksProxy),
    HttpConnect(PinnedHttpProxy),
}

impl From<PinnedSocksProxy> for PinnedProxy {
    fn from(proxy: PinnedSocksProxy) -> Self {
        Self::Socks5(proxy)
    }
}

impl From<PinnedHttpProxy> for PinnedProxy {
    fn from(proxy: PinnedHttpProxy) -> Self {
        Self::HttpConnect(proxy)
    }
}

impl PinnedProxy {
    pub(super) fn bind(self, destination: &PinnedDestination) -> reqwest::Proxy {
        match self {
            Self::Socks5(proxy) => proxy.0,
            Self::HttpConnect(proxy) => proxy.0.with_connect_target(destination.addresses[0]),
        }
    }
}

fn proxy_at(address: SocketAddr, scheme: &str) -> Result<reqwest::Proxy> {
    if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
        return Err(Error::Configuration);
    }
    reqwest::Proxy::all(format!("{scheme}://{address}")).map_err(|_| Error::Configuration)
}

fn validate_proxy_credentials(username: &str, password: &str) -> Result<()> {
    if [username, password].iter().any(|value| {
        value.is_empty() || value.len() > 255 || value.bytes().any(|byte| byte.is_ascii_control())
    }) {
        return Err(Error::Configuration);
    }
    Ok(())
}

pub(super) struct PinnedDestination {
    origin: Origin,
    domain: Option<String>,
    addresses: Vec<SocketAddr>,
}

impl PinnedDestination {
    pub(super) fn new(url: &str, addresses: Vec<SocketAddr>) -> Result<Self> {
        let url = Url::parse(url).map_err(|_| Error::DestinationMismatch)?;
        if addresses.is_empty()
            || addresses.len() > 64
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(Error::DestinationMismatch);
        }
        let port = url
            .port_or_known_default()
            .ok_or(Error::DestinationMismatch)?;
        if port == 0 || addresses.iter().any(|addr| addr.port() != port) {
            return Err(Error::DestinationMismatch);
        }
        let domain = match url.host() {
            Some(Host::Domain(host)) => Some(host.to_owned()),
            Some(Host::Ipv4(ip)) if addresses.iter().all(|addr| addr.ip() == ip) => None,
            Some(Host::Ipv6(ip)) if addresses.iter().all(|addr| addr.ip() == ip) => None,
            _ => return Err(Error::DestinationMismatch),
        };
        Ok(Self {
            origin: url.origin(),
            domain,
            addresses,
        })
    }

    pub(super) fn is_https(&self) -> bool {
        matches!(&self.origin, Origin::Tuple(scheme, _, _) if scheme == "https")
    }

    pub(super) fn apply(&self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        builder.dns_resolver(Arc::new(PinnedResolver {
            domain: self.domain.clone(),
            addresses: self.addresses.clone(),
        }))
    }

    pub(super) fn check_url(&self, url: &Url) -> Result<()> {
        if url.origin() != self.origin || !url.username().is_empty() || url.password().is_some() {
            return Err(Error::DestinationMismatch);
        }
        Ok(())
    }

    pub(super) fn check_request(&self, request: &reqwest::Request) -> Result<()> {
        self.check_url(request.url())?;
        if request.headers().contains_key(http::header::HOST) {
            return Err(Error::DestinationMismatch);
        }
        Ok(())
    }
}

struct PinnedResolver {
    domain: Option<String>,
    addresses: Vec<SocketAddr>,
}

impl reqwest::dns::Resolve for PinnedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str();
        let matches = match &self.domain {
            Some(domain) => domain.eq_ignore_ascii_case(host),
            None => host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| self.addresses.first().is_some_and(|addr| addr.ip() == ip)),
        };
        let addresses = matches.then(|| self.addresses.clone());
        Box::pin(async move {
            let addresses =
                addresses.ok_or_else(|| std::io::Error::other("HTTP 目标域名未固定"))?;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}
