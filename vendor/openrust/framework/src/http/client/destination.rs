use std::{net::SocketAddr, sync::Arc};

use url::{Host, Origin, Url};

use super::{Error, Result};

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
        let addresses = self
            .domain
            .as_ref()
            .filter(|domain| domain.eq_ignore_ascii_case(name.as_str()))
            .map(|_| self.addresses.clone());
        Box::pin(async move {
            let addresses =
                addresses.ok_or_else(|| std::io::Error::other("HTTP 目标域名未固定"))?;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}
