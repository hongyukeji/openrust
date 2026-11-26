use crate::validation::active_url::DnsLookup;
use hickory_resolver::{
    TokioResolver,
    proto::rr::{RData, RecordType},
};
/// 邮箱DNS沿用同一解析器配置和查询传输，不根据主机文件推定邮件可达。
pub(super) async fn passes(value: &str, fake: bool, resolver: Option<&TokioResolver>) -> bool {
    let host = value.rsplit_once('@').map_or(value, |(_, host)| host);
    let parts: Vec<_> = host.split('.').collect();
    if parts.len() <= 1
        || [
            "test",
            "example",
            "invalid",
            "localhost",
            "local",
            "intranet",
            "internal",
            "private",
            "corp",
            "home",
            "lan",
        ]
        .contains(parts.last().unwrap())
    {
        return false;
    }
    let Some(ascii) = ascii_domain(host) else {
        return false;
    };
    let host = ascii.as_str();
    if fake {
        return host.split('.').count() > 1;
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
    let parts: Vec<_> = host.split('.').collect();
    // 从可查询的父域向完整域名推进，保持既有DNS校验器的查询顺序。
    for start in (0..parts.len().saturating_sub(1)).rev() {
        let host = parts[start..].join(".");
        let (a, mx) = tokio::join!(
            lookup.records(&host, RecordType::A),
            lookup.records(&host, RecordType::MX)
        );
        let (Ok(a), Ok(mx)) = (a, mx) else {
            continue;
        };
        let aaaa = lookup.records(&host, RecordType::AAAA).await;
        let records: Vec<_> = a
            .into_iter()
            .chain(mx)
            .chain(aaaa.unwrap_or_default())
            .filter(|record| {
                matches!(
                    record.record_type(),
                    RecordType::A | RecordType::AAAA | RecordType::MX
                )
            })
            .collect();
        if records.is_empty() {
            continue;
        }
        let null_mx = records
            .iter()
            .any(|record| matches!(record.data(),RData::MX(mx)if mx.exchange().is_root()));
        if !null_mx {
            return true;
        }
    }
    false
}

fn ascii_domain(host: &str) -> Option<String> {
    use icu_normalizer::uts46::Uts46MapperBorrowed;
    use intl::unicode::idna::{Error, to_ascii};
    let mapped: String = Uts46MapperBorrowed::new()
        .map_normalize(host.chars())
        .collect();
    let mapped = mapped.strip_suffix('.').unwrap_or(&mapped);
    let mut labels = Vec::new();
    for label in mapped.split('.') {
        if label.is_empty() {
            return None;
        }
        let encoded = match to_ascii(label) {
            Ok(encoded) => encoded,
            Err(Error::ContextJ | Error::Bidi) => {
                // 默认UIDNA没有启用ContextJ/Bidi；其余检查仍由成熟实现执行。
                let (decoded, original) = if let Some(payload) = label.strip_prefix("xn--") {
                    (idna::punycode::decode_to_string(payload)?, Some(payload))
                } else {
                    (label.to_owned(), None)
                };
                let encoded = idna::punycode::encode_str(&decoded)?;
                if original.is_some_and(|original| original != encoded) {
                    return None;
                }
                let encoded = format!("xn--{encoded}");
                if encoded.len() > 63 {
                    return None;
                }
                encoded
            }
            _ => return None,
        };
        labels.push(encoded);
    }
    let host = labels.join(".");
    (host.len() <= 253).then_some(host)
}
