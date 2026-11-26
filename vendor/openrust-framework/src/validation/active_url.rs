use futures_util::StreamExt as _;
use hickory_resolver::{
    TokioResolver,
    name_server::{NameServerPool, TokioConnectionProvider},
    proto::{
        op::Query,
        rr::{Name, RecordType},
        xfer::{DnsHandle as _, DnsRequestOptions, RetryDnsHandle},
    },
};

/// 查询 DNS 记录；不使用 hosts 文件或将数字地址当作解析成功。
pub(super) async fn passes(value: &str, fake: bool, resolver: Option<&TokioResolver>) -> bool {
    let Some(host) = host(value) else {
        return false;
    };
    if host == "0" {
        return false;
    }
    if fake {
        let host = host.trim_end_matches('.');
        return host.parse::<std::net::IpAddr>().is_err()
            && !host.is_empty()
            && host.len() <= 253
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label.as_bytes()[0].is_ascii_alphanumeric()
                    && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            });
    }
    let owned;
    let resolver = if let Some(resolver) = resolver {
        resolver
    } else {
        let Ok(builder) = TokioResolver::builder_tokio() else {
            return false;
        };
        owned = builder.build();
        &owned
    };
    let lookup = DnsLookup::new(resolver);
    let (a, aaaa) = tokio::join!(
        lookup.records(&host, RecordType::A),
        lookup.records(&host, RecordType::AAAA)
    );
    [a, aaaa].into_iter().any(|records| {
        records.is_ok_and(|records| {
            records
                .iter()
                .any(|record| matches!(record.record_type(), RecordType::A | RecordType::AAAA))
        })
    })
}

/// 共享成熟解析器的原始传输层，绕过hosts、localhost合成和地址查询缓存。
pub(super) struct DnsLookup {
    client: RetryDnsHandle<NameServerPool<TokioConnectionProvider>>,
    options: DnsRequestOptions,
}
impl DnsLookup {
    pub(super) fn new(resolver: &TokioResolver) -> Self {
        let pool = NameServerPool::from_config(
            resolver.config().name_servers().to_vec().into(),
            resolver.options().clone(),
            TokioConnectionProvider::default(),
        );
        let mut options = DnsRequestOptions::default();
        options.recursion_desired = resolver.options().recursion_desired;
        options.use_edns = resolver.options().edns0;
        Self {
            client: RetryDnsHandle::new(pool, resolver.options().attempts),
            options,
        }
    }
    pub(super) async fn records(
        &self,
        host: &str,
        kind: RecordType,
    ) -> Result<Vec<hickory_resolver::proto::rr::Record>, ()> {
        let host = host.strip_suffix('.').unwrap_or(host);
        let name = Name::from_ascii(format!("{host}.")).map_err(|_| ())?;
        let mut stream = self.client.lookup(Query::query(name, kind), self.options);
        match stream.next().await {
            Some(Ok(response)) => match response.response_code() {
                hickory_resolver::proto::op::ResponseCode::NoError => {
                    Ok(response.answers().to_vec())
                }
                hickory_resolver::proto::op::ResponseCode::NXDomain => Ok(Vec::new()),
                _ => Err(()),
            },
            Some(Err(error))
                if matches!(
                    error.kind(),
                    hickory_resolver::proto::ProtoErrorKind::NoRecordsFound {
                        response_code: hickory_resolver::proto::op::ResponseCode::NoError
                            | hickory_resolver::proto::op::ResponseCode::NXDomain,
                        ..
                    }
                ) =>
            {
                Ok(Vec::new())
            }
            _ => Err(()),
        }
    }
}

/// 按 PHP parse_url 的主机切分保留原文，避免 WHATWG 的 IDN、数字地址及 scheme 规范化。
fn host(input: &str) -> Option<String> {
    let authority = if let Some((scheme, rest)) = input.split_once("://") {
        if scheme.is_empty()
            || !scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
        {
            return None;
        }
        if scheme.eq_ignore_ascii_case("file") && rest.starts_with('/') {
            return None;
        }
        rest
    } else if let Some(rest) = input.strip_prefix("//") {
        rest
    } else if let Some((_, port)) = input.split_once(':') {
        let port = port.split('/').next()?;
        if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        input
    } else {
        return None;
    };
    let authority = authority.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let name = if authority.starts_with('[') && authority.ends_with(']') {
        authority
    } else if let Some((name, port)) = authority.rsplit_once(':') {
        if port.len() > 5 || !port.is_empty() && !valid_port(port) {
            return None;
        }
        name
    } else {
        authority
    };
    if name.is_empty() {
        return None;
    }
    Some(
        name.chars()
            .map(|character| {
                if character.is_ascii_control() {
                    '_'
                } else {
                    character
                }
            })
            .collect(),
    )
}

fn valid_port(port: &str) -> bool {
    let port = port.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let port = port.strip_prefix('+').unwrap_or(port);
    let digits = port.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0
        && port[..digits]
            .parse::<u32>()
            .is_ok_and(|port| port <= 65535)
}
