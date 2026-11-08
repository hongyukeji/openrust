use super::Uri;
use crate::{Result, support::Htmlable};
use serde_json::json;

#[test]
fn uri_matches_fixed_league_normalization_and_rejection() -> Result<()> {
    for (label, raw, expected) in [
        ("empty", "", Some("")),
        ("relative", "../some/a", Some("../some/a")),
        ("root", "/a/../b", Some("/a/../b")),
        ("network", "//example.com/p", Some("//example.com/p")),
        ("empty_query", "?", Some("?")),
        ("empty_fragment", "#", Some("#")),
        ("query_fragment", "?q=1#t", Some("?q=1#t")),
        ("fragment", "#x", Some("#x")),
        ("opaque", "mailto:a@b", None),
        ("urn", "urn:example:hello", Some("urn:example:hello")),
        (
            "userinfo",
            "HTTP://U:P@ExAmple.com:80/a",
            Some("http://U:P@example.com/a"),
        ),
        (
            "scheme_port",
            "https://example.com:443",
            Some("https://example.com"),
        ),
        (
            "zero_port",
            "http://example.com:0",
            Some("http://example.com:0"),
        ),
        ("absolute_no_authority", "foo:/a", Some("foo:/a")),
        ("absolute_empty_authority", "foo:///a", Some("foo:///a")),
        ("host_case", "//EXAMPLE.com:081", Some("//example.com:81")),
        (
            "unicode",
            "https://münich.example/你好?名=值#片",
            Some("https://xn--mnich-kva.example/%E4%BD%A0%E5%A5%BD?%E5%90%8D=%E5%80%BC#%E7%89%87"),
        ),
        (
            "unicode_relative",
            "/你好?q=值",
            Some("/%E4%BD%A0%E5%A5%BD?q=%E5%80%BC"),
        ),
        (
            "ipv6",
            "http://[2001:0DB8::01]/x",
            Some("http://[2001:0DB8::01]/x"),
        ),
        (
            "ipv6_zone",
            "http://[fe80::1%25eth0]/",
            Some("http://[fe80::1%25eth0]/"),
        ),
        ("bad_ipv6", "http://[zz]/", None),
        (
            "escapes",
            "/%7e%2f%2F%25?x=%3c#%3E",
            Some("/%7e%2f%2F%25?x=%3c#%3E"),
        ),
        ("bad_escape", "/%zz?x=%", Some("/%25zz?x=%25")),
        (
            "space",
            "a b?q=hello world# a",
            Some("a%20b?q=hello%20world#%20a"),
        ),
        ("ascii_control", "/a\tb", None),
        ("backslash", "a\\b", Some("a%5Cb")),
        ("http_backslash", "http:\\\\example.com\\\\p", None),
        (
            "trailing_dot",
            "https://EXAMPLE.COM./",
            Some("https://example.com./"),
        ),
        (
            "malformed_port",
            "https://example.com:99999/p",
            Some("https://example.com:99999/p"),
        ),
        ("colon_relative", "this:that", Some("this:that")),
        ("colon_path", "./a:b", Some("./a:b")),
        ("empty_host", "http://", None),
        ("empty_user", "http://@host/", Some("http://@host/")),
    ] {
        assert_uri_case(label, raw, expected)?;
    }
    Ok(())
}

#[test]
fn uri_matches_fixed_league_special_schemes_and_components() -> Result<()> {
    for (label, raw, expected) in [
        ("empty_network", "//", Some("//")),
        ("space_host", "//a b/", Some("//a%20b/")),
        ("percent_host", "//%65xample.com/", Some("//example.com/")),
        ("ipv4_short", "http://127.1/", Some("http://127.1/")),
        ("ipv4_hex", "http://0x7f000001/", Some("http://0x7f000001/")),
        ("opaque_host", "foo://my_name!/", Some("foo://my_name!/")),
        (
            "plus_port",
            "https://example.com:+1",
            Some("https://example.com:1"),
        ),
        (
            "empty_port",
            "https://example.com:/p",
            Some("https://example.com/p"),
        ),
        ("empty_fragment_special", "ws://example.test/#", None),
        (
            "mailto_valid",
            "mailto:user@example.test?subject=hello",
            Some("mailto:user@example.test?subject=hello"),
        ),
        ("mailto_no_dot", "mailto:a@b", None),
        ("file_normal", "file:///tmp/a b", Some("file:///tmp/a%20b")),
        (
            "file_host",
            "file://localhost/tmp/a",
            Some("file://localhost/tmp/a"),
        ),
        ("file_drive", "file:C:/x", None),
        (
            "data_simple",
            "data:,Hello world",
            Some("data:text/plain;charset=us-ascii,Hello%20world"),
        ),
        (
            "data_base64",
            "data:text/plain;base64,SGVsbG8=",
            Some("data:text/plain;base64,SGVsbG8="),
        ),
        ("urn_query", "urn:example:a?=q", Some("urn:example:a?=q")),
        ("urn_frag", "urn:example:a#f", Some("urn:example:a#f")),
        ("invalid_urn", "urn:x", None),
        (
            "unknown",
            "unknown:user:pass@a",
            Some("unknown:user:pass@a"),
        ),
        (
            "user_info",
            "http://a%41:p%2f@ExAmple.com/a",
            Some("http://a%41:p%2f@example.com/a"),
        ),
        ("unicode_control", "/a\u{85}b", Some("/a%C2%85b")),
    ] {
        assert_uri_case(label, raw, expected)?;
    }
    Ok(())
}

fn assert_uri_case(label: &str, raw: &str, expected: Option<&str>) -> Result<()> {
    let actual = Uri::of(raw);
    if let Some(expected) = expected {
        let uri = actual?;
        assert_eq!(uri.value(), expected, "{label}");
        assert_eq!(uri.to_html(), expected, "{label}");
        assert_eq!(
            serde_json::to_value(&uri).map_err(crate::Error::internal)?,
            json!(expected),
            "{label}"
        );
    } else {
        assert!(
            matches!(actual, Err(error) if error.code() == "support_uri_invalid"),
            "{label}"
        );
    }
    Ok(())
}

#[test]
fn uri_components_keep_empty_delimiters_and_credentials_out_of_debug() -> Result<()> {
    let uri = Uri::of("https://user:secret@Example.test:99999/a//b/?#")?;
    assert_eq!(uri.scheme(), Some("https"));
    assert_eq!(uri.authority(), Some("user:secret@example.test:99999"));
    assert_eq!(uri.user(), Some("user"));
    assert_eq!(uri.user_info(), Some("user:secret"));
    assert_eq!(uri.password(), Some("secret"));
    assert_eq!(uri.host(), Some("example.test"));
    assert_eq!(uri.port(), Some(99_999));
    assert_eq!(uri.path(), "a//b");
    assert_eq!(uri.path_segments()?.to_value()?, json!(["a", "", "b"]));
    assert_eq!(uri.query_string(), Some(""));
    assert_eq!(uri.fragment(), Some(""));
    assert!(!format!("{uri:?}").contains("secret"));
    let empty = Uri::of("")?;
    assert!(empty.is_empty());
    assert!(empty.path_segments()?.is_empty());
    assert_eq!(empty.path(), "/");
    assert_eq!(empty.query_string(), None);
    assert_eq!(empty.fragment(), None);
    Ok(())
}
