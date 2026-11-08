//! 对照固定 League URI 的引用切分、编码和组件约束。

use super::invalid_uri;
use crate::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Components {
    pub(super) scheme: Option<String>,
    pub(super) user: Option<String>,
    pub(super) password: Option<String>,
    pub(super) host: Option<String>,
    pub(super) port: Option<i64>,
    pub(super) path: String,
    pub(super) query: Option<String>,
    pub(super) fragment: Option<String>,
}

pub(super) fn parse(input: &str) -> Result<Components> {
    if input.starts_with(' ') || input.ends_with(' ') || input.bytes().any(|b| b.is_ascii_control())
    {
        return Err(invalid_uri());
    }
    let (without_fragment, fragment) = split_delimiter(input, '#');
    let (hierarchy, query) = split_delimiter(without_fragment, '?');
    let (scheme, hierarchy) = split_scheme(hierarchy)?;
    let mut uri = Components {
        scheme,
        query: query.map(|s| encode(s, b":@/?")),
        fragment: fragment.map(|s| encode(s, b":@/?")),
        ..Components::default()
    };
    let path = if let Some(rest) = hierarchy.strip_prefix("//") {
        let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        parse_authority(authority, &mut uri)?;
        path
    } else {
        hierarchy
    };
    uri.path = match uri.scheme.as_deref() {
        Some("data") => encode(&data_path(path)?, b":@/"),
        Some("file") => file_path(encode(path, b":@/")),
        _ => encode(path, b":@/"),
    };
    if validate_state(&uri) {
        Ok(uri)
    } else {
        Err(invalid_uri())
    }
}

fn split_delimiter(input: &str, delimiter: char) -> (&str, Option<&str>) {
    input
        .split_once(delimiter)
        .map_or((input, None), |(left, right)| (left, Some(right)))
}

fn split_scheme(input: &str) -> Result<(Option<String>, &str)> {
    let Some((scheme, rest)) = input.split_once(':').filter(|(s, _)| !s.contains('/')) else {
        return Ok((None, input));
    };
    let mut bytes = scheme.bytes();
    if !bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        || !bytes.all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
    {
        return Err(invalid_uri());
    }
    Ok((Some(scheme.to_ascii_lowercase()), rest))
}

fn parse_authority(input: &str, uri: &mut Components) -> Result<()> {
    let host_and_port = if let Some((user_info, host_and_port)) = input.split_once('@') {
        let (user, password) = split_delimiter(user_info, ':');
        uri.user = Some(encode(user, b""));
        uri.password = password.map(|s| encode(s, b":"));
        host_and_port
    } else {
        input
    };
    let (host, port) = if host_and_port.starts_with('[') {
        let end = host_and_port.find(']').ok_or_else(invalid_uri)? + 1;
        let (host, suffix) = host_and_port.split_at(end);
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':').ok_or_else(invalid_uri)?)
        };
        (host, port)
    } else {
        split_delimiter(host_and_port, ':')
    };
    uri.host = Some(normalize_host(host)?);
    let port = port
        .filter(|value| !value.is_empty())
        .map(parse_port)
        .transpose()?;
    uri.port = port.filter(|port| default_port(uri.scheme.as_deref()) != Some(*port));
    Ok(())
}

fn parse_port(port: &str) -> Result<i64> {
    let digits = port.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(0);
    }
    let unsigned = digits.strip_prefix(['+', '-']).unwrap_or(digits);
    if unsigned.is_empty()
        || (unsigned.len() > 1 && unsigned.starts_with('0'))
        || !unsigned.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid_uri());
    }
    digits
        .parse::<i64>()
        .ok()
        .filter(|value| *value >= 0)
        .ok_or_else(invalid_uri)
}

fn default_port(scheme: Option<&str>) -> Option<i64> {
    Some(match scheme? {
        "acap" => 674,
        "afp" => 548,
        "dict" => 2628,
        "dns" => 53,
        "ftp" => 21,
        "http" | "ws" | "xmpp" => 80,
        "https" | "wss" => 443,
        "git" => 9418,
        "gopher" => 70,
        "imap" => 143,
        "imaps" => 993,
        "ipp" | "ipps" => 631,
        "irc" => 194,
        "ircs" => 6697,
        "ldap" => 389,
        "ldaps" => 636,
        "mms" => 1755,
        "msrp" | "msrps" => 2855,
        "mtqp" => 1038,
        "nfs" => 111,
        "nntp" => 119,
        "nntps" => 563,
        "pop" => 110,
        "prospero" => 1525,
        "redis" => 6379,
        "rsync" => 873,
        "rtsp" => 554,
        "rtsps" => 322,
        "rtspu" => 5005,
        "sftp" | "ssh" => 22,
        "smb" => 445,
        "smtp" => 25,
        "snmp" => 161,
        "svn" => 3690,
        "telnet" | "tn3270" => 23,
        "ventrilo" => 3784,
        "vnc" => 5900,
        "wais" => 210,
        _ => return None,
    })
}

fn normalize_host(host: &str) -> Result<String> {
    if let Some(ip) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        return valid_ip_literal(ip)
            .then(|| host.to_owned())
            .ok_or_else(invalid_uri);
    }
    if host.bytes().any(|b| b":/?#[]@".contains(&b)) {
        return Err(invalid_uri());
    }
    if host.is_ascii() || host.contains(['%', ' ']) {
        return normalize_registered_host(host);
    }
    match url::Host::parse(host).map_err(|_| invalid_uri())? {
        url::Host::Domain(name) => Ok(name),
        _ => Err(invalid_uri()),
    }
}

fn normalize_registered_host(host: &str) -> Result<String> {
    let bytes = host.as_bytes();
    let has_escape = host.contains(['%', ' ']);
    let mut encoded = String::with_capacity(host.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let decoded = escaped_byte(bytes, index).ok_or_else(invalid_uri)?;
            if unreserved(decoded) {
                encoded.push(char::from(decoded.to_ascii_lowercase()));
            } else {
                push_escape(&mut encoded, decoded);
            }
            index += 3;
        } else {
            if !unreserved(byte) && !sub_delimiter(byte) && byte != b' ' && byte.is_ascii() {
                return Err(invalid_uri());
            }
            if byte == b' ' || !byte.is_ascii() || (has_escape && sub_delimiter(byte)) {
                push_escape(&mut encoded, byte);
            } else {
                encoded.push(char::from(byte.to_ascii_lowercase()));
            }
            index += 1;
        }
    }
    Ok(encoded)
}

fn valid_ip_literal(input: &str) -> bool {
    if let Some((version, address)) = input
        .strip_prefix(['v', 'V'])
        .and_then(|s| s.split_once('.'))
    {
        return !version.is_empty()
            && !matches!(version, "4" | "6")
            && version.bytes().all(|b| b.is_ascii_hexdigit())
            && !address.is_empty()
            && address
                .bytes()
                .all(|b| unreserved(b) || sub_delimiter(b) || b == b':');
    }
    let (address, scope) = split_delimiter(input, '%');
    let Ok(address) = address.parse::<Ipv6Addr>() else {
        return false;
    };
    scope.is_none_or(|scope| {
        address.segments()[0] == 0xfe80
            && decode(&format!("%{scope}"))
                .bytes()
                .all(|b| (b' '..=0x7f).contains(&b) && !b":/?#[]@ ".contains(&b))
    })
}

fn encode(input: &str, extra: &[u8]) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' && escaped_byte(bytes, index).is_some() {
            output.push_str(&input[index..index + 3]);
            index += 3;
            continue;
        }
        if unreserved(byte) || sub_delimiter(byte) || extra.contains(&byte) {
            output.push(char::from(byte));
        } else {
            push_escape(&mut output, byte);
        }
        index += 1;
    }
    output
}

fn decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if let Some(decoded) = escaped_byte(bytes, index) {
            output.push(decoded);
            index += 3;
        } else {
            output.push(byte);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~".contains(&byte)
}

fn sub_delimiter(byte: u8) -> bool {
    b"!$&'()*+,;=".contains(&byte)
}

fn escaped_byte(bytes: &[u8], index: usize) -> Option<u8> {
    if bytes.get(index) != Some(&b'%') {
        return None;
    }
    let high = char::from(*bytes.get(index + 1)?).to_digit(16)?;
    let low = char::from(*bytes.get(index + 2)?).to_digit(16)?;
    u8::try_from(high * 16 + low).ok()
}

fn push_escape(output: &mut String, byte: u8) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    output.push('%');
    output.push(char::from(HEX[usize::from(byte >> 4)]));
    output.push(char::from(HEX[usize::from(byte & 0x0f)]));
}

fn file_path(path: String) -> String {
    let (prefix, drive) = path
        .strip_prefix('/')
        .map_or(("", path.as_str()), |s| ("/", s));
    if drive
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
    {
        let (letter, rest) = drive.split_at(1);
        if let Some(rest) = rest.strip_prefix(':').or_else(|| rest.strip_prefix("%7C")) {
            return format!("{prefix}{letter}:{rest}");
        }
    }
    path
}

fn data_path(path: &str) -> Result<String> {
    if path.is_empty() {
        return Ok("text/plain;charset=us-ascii,".to_owned());
    }
    if !path.is_ascii() {
        return Err(invalid_uri());
    }
    let (meta, data) = path.split_once(',').ok_or_else(invalid_uri)?;
    let (mime, params) = split_delimiter(meta, ';');
    let mime = if mime.is_empty() { "text/plain" } else { mime };
    let params = params
        .filter(|params| !params.is_empty())
        .unwrap_or("charset=us-ascii");
    let (top, subtype) = mime.split_once('/').ok_or_else(invalid_uri)?;
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    if top.is_empty()
        || subtype.is_empty()
        || !top.bytes().all(word)
        || !subtype.bytes().all(|b| word(b) || b"-.+".contains(&b))
    {
        return Err(invalid_uri());
    }
    let (params_to_validate, binary) = if params == "base64" {
        ("", true)
    } else if let Some(rest) = params.strip_suffix(";base64") {
        (rest, true)
    } else {
        (params, false)
    };
    if params_to_validate
        .split(';')
        .filter(|param| !param.is_empty() && *param != "0")
        .any(|param| {
            param
                .split_once('=')
                .is_none_or(|(key, val)| key.eq_ignore_ascii_case("base64") || val.contains('='))
        })
        || (binary
            && !STANDARD
                .decode(data)
                .is_ok_and(|bytes| STANDARD.encode(bytes) == data))
    {
        return Err(invalid_uri());
    }
    Ok(format!("{mime};{params},{data}"))
}

fn validate_state(uri: &Components) -> bool {
    let Some(scheme) = uri.scheme.as_deref() else {
        return true;
    };
    let has_host = uri.host.as_deref().is_some_and(|host| !host.is_empty());
    let no_query_or_fragment = uri.query.is_none() && uri.fragment.is_none();
    match scheme {
        "http" | "https" => has_host,
        "ws" | "wss" | "ipp" | "ipps" => has_host && uri.fragment.is_none(),
        "ftp" | "gopher" | "afp" | "dict" | "msrp" | "msrps" | "mtqp" | "rsync" | "ssh" | "svn"
        | "snmp" => has_host && no_query_or_fragment,
        "ldap" | "ldaps" | "acap" | "imaps" | "imap" | "redis" => uri.fragment.is_none(),
        "prospero" => no_query_or_fragment && uri.user.is_none(),
        "telnet" | "tn3270" => no_query_or_fragment && matches!(uri.path.as_str(), "" | "/"),
        "vnc" => uri.host.is_some() && uri.fragment.is_none() && uri.path.is_empty(),
        "data" | "about" | "javascript" => uri.host.is_none() && no_query_or_fragment,
        "file" => {
            uri.host.is_some() && uri.user.is_none() && uri.port.is_none() && no_query_or_fragment
        }
        "mailto" => valid_mailto(uri),
        "urn" => valid_urn(uri),
        "blob" => valid_blob(uri),
        "bitcoin" | "geo" | "magnet" | "pkcs11" | "sip" | "sips" | "tel" => uri.host.is_none(),
        _ => default_port(Some(scheme)).is_none() || uri.host.is_some(),
    }
}

fn valid_urn(uri: &Components) -> bool {
    let Some((nid, nss)) = uri.path.split_once(':') else {
        return false;
    };
    let nid = nid.as_bytes();
    uri.host.is_none()
        && (2..=32).contains(&nid.len())
        && nid.first().is_some_and(u8::is_ascii_alphanumeric)
        && nid.last().is_some_and(u8::is_ascii_alphanumeric)
        && nid.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        && !nss.is_empty()
        && uri
            .fragment
            .as_deref()
            .is_none_or(|frag| !frag.is_empty() && !frag.contains('?'))
        && uri.query.as_deref().is_none_or(valid_urn_query)
}

fn valid_urn_query(query: &str) -> bool {
    let component = |value: &str| !value.is_empty() && !value.contains('?');
    if let Some(remaining) = query.strip_prefix('+') {
        return remaining.split_once("?=").map_or_else(
            || component(remaining),
            |(r, q)| component(r) && component(q),
        );
    }
    query.strip_prefix('=').is_some_and(component)
}

fn valid_blob(uri: &Components) -> bool {
    if uri.host.is_some() || uri.query.is_some() || uri.fragment.is_some() {
        return false;
    }
    let Some((origin, uuid)) = uri.path.rsplit_once('/') else {
        return false;
    };
    let uuid = uuid.as_bytes();
    if uuid.len() != 36
        || !uuid.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            14 => b"12345".contains(b),
            19 => b"89abAB".contains(b),
            _ => b.is_ascii_hexdigit(),
        })
    {
        return false;
    }
    if origin == "null" {
        return true;
    }
    let Some((scheme, _)) = origin.split_once(':') else {
        return false;
    };
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "ftp" | "http" | "https" | "ws" | "wss"
    ) && split_scheme(origin).is_ok_and(|(_, tail)| {
        let hierarchy = tail.strip_prefix("//").unwrap_or(tail);
        !hierarchy.contains('/')
    })
}

fn valid_mailto(uri: &Components) -> bool {
    if uri.host.is_some() || uri.fragment.is_some() {
        return false;
    }
    let mut has_to = false;
    if let Some(query) = &uri.query {
        if query.contains('?') {
            return false;
        }
        for field in query.split('&').filter(|value| !value.is_empty()) {
            let (name, value) = split_delimiter(field, '=');
            let name = decode(name).to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "to" | "cc"
                    | "bcc"
                    | "reply-to"
                    | "from"
                    | "sender"
                    | "resent-to"
                    | "resent-cc"
                    | "resent-bcc"
                    | "resent-from"
                    | "resent-sender"
                    | "return-path"
                    | "delivery-to"
                    | "site-owner"
            ) {
                if !value.is_some_and(valid_email_list) {
                    return false;
                }
                has_to |= name == "to";
            } else if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"'\x60#$%&*+.^_|~!-".contains(&b))
            {
                return false;
            }
        }
    }
    if uri.path.is_empty() {
        has_to
    } else {
        valid_email_list(&uri.path)
    }
}

fn valid_email_list(input: &str) -> bool {
    input.split(',').all(|email| {
        let decoded = decode(email);
        let Some((local, domain)) = decoded.rsplit_once('@') else {
            return false;
        };
        if local.is_empty()
            || local.len() > 64
            || decoded.len() > 254
            || !local.split('.').all(|part| {
                !part.is_empty()
                    && part.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_\x60{|}~".contains(&b)
                    })
            })
        {
            return false;
        }
        if let Some(ip) = domain.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            return ip.parse::<Ipv4Addr>().is_ok()
                || ip
                    .strip_prefix("IPv6:")
                    .is_some_and(|value| value.parse::<Ipv6Addr>().is_ok());
        }
        domain.contains('.')
            && domain.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    })
}
