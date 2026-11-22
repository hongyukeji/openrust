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
    let Ok(name) = Name::from_ascii(format!("{host}.")) else {
        return false;
    };
    // 直接使用成熟解析器的传输层，绕过 localhost 合成结果、hosts 与地址查询缓存。
    let pool = NameServerPool::from_config(
        resolver.config().name_servers().to_vec().into(),
        resolver.options().clone(),
        TokioConnectionProvider::default(),
    );
    let client = RetryDnsHandle::new(pool, resolver.options().attempts);
    let mut options = DnsRequestOptions::default();
    options.recursion_desired = resolver.options().recursion_desired;
    options.use_edns = resolver.options().edns0;
    let query = |kind| {
        let mut stream = client.lookup(Query::query(name.clone(), kind), options);
        async move {
            stream.next().await.is_some_and(|response| {
                response.is_ok_and(|response| {
                    response.answers().iter().any(|record| {
                        matches!(record.record_type(), RecordType::A | RecordType::AAAA)
                    })
                })
            })
        }
    };
    let (a, aaaa) = tokio::join!(query(RecordType::A), query(RecordType::AAAA));
    a || aaaa
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
