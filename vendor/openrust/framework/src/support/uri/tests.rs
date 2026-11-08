use super::Uri;
use crate::{Error, Result, support::Htmlable};
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

// 固定 Laravel Support\\Uri 和 league/uri be89e2382e37acdef232259071ade5180821397b，PHP 8.5.12。
fn query_oracle() -> Result<serde_json::Value> {
    serde_json::from_str(include_str!("query_oracle.json"))
        .map_err(|error| Error::custom("uri_query_fixture", error.to_string()))
}

fn fixture_error() -> Error {
    Error::custom("uri_query_fixture", "固定 PHP 探针缺少用例")
}

#[test]
fn uri_query_matches_fixed_league_reading() -> Result<()> {
    let oracle = query_oracle()?;
    for (label, row) in oracle["rows"].as_object().ok_or_else(fixture_error)? {
        let uri = Uri::of(row["input"].as_str().ok_or_else(fixture_error)?)?;
        let query = uri.query();
        assert_eq!(query.value(), row["raw"], "{label}");
        assert_eq!(query.to_array()?, row["array"], "{label}");
        assert_eq!(query.decode()?, row["decode"], "{label}");
        assert_eq!(uri.decode()?, row["uri_decode"], "{label}");
        assert_eq!(
            json!([
                query.get("dot.key")?,
                query.get("nested.a")?,
                query.get("list.*")?
            ]),
            row["get"],
            "{label}"
        );
        assert_eq!(
            query.all_keys(&["dot.key", "x", "missing"])?,
            row["all"],
            "{label}"
        );
        assert_eq!(query.has("dot.key")?, label == "keys");
        assert!(query.missing("missing")?);
    }
    Ok(())
}

#[test]
fn uri_query_matches_fixed_laravel_mutation() -> Result<()> {
    let oracle = query_oracle()?;
    let expected = &oracle["mutate"];
    let uri = Uri::of(expected["original"].as_str().ok_or_else(fixture_error)?)?;
    for (operation, updated) in [
        (
            "merge",
            uri.with_query(&json!({"nested.a":"new","nested.b":"add","missing":null}))?,
        ),
        (
            "replace",
            uri.replace_query(&json!({"nested.a":"new","empty":null,"bool":false}))?,
        ),
        (
            "missing",
            uri.with_query_if_missing(
                &json!({"nested.a":"ignored","nested.b":"added","x":"ignored","blank":0}),
            )?,
        ),
        ("push_list", uri.push_onto_query("list", json!(["a", "b"]))?),
        (
            "push_assoc",
            uri.push_onto_query("map", json!({"z":"new","extra":"c"}))?,
        ),
        ("push_scalar", uri.push_onto_query("scalar", json!("two"))?),
        ("without", uri.without_query(&["nested.a", "x", "list"])?),
        ("clear", uri.replace_query(&json!([]))?),
    ] {
        assert_eq!(updated.value(), expected[operation], "{operation}");
    }
    assert_eq!(uri.value(), expected["original"]);
    Ok(())
}

#[test]
fn uri_query_covers_php_list_and_decode_edges() -> Result<()> {
    let uri = Uri::of("/a?existing[]=one&list[]=1&d.key=x&nested[a]=v&bool=")?;
    assert!(
        uri.push_onto_query("existing", json!([["nested"]]))
            .is_err()
    );
    assert_eq!(
        uri.push_onto_query("list", json!([true, 1, "1", null, false, ""]))?
            .value(),
        "/a?existing%5B0%5D=one&list%5B0%5D=1&d.key=x&nested%5Ba%5D=v&bool="
    );
    assert_eq!(
        Uri::of("/a")?
            .push_onto_query("list", json!([["nested"]]))?
            .value(),
        "/a?list%5B0%5D%5B0%5D=nested"
    );
    assert_eq!(
        uri.with_query_if_missing(
            &json!({"d.key":"replace","nested.a":"replace","nested.b":"add"})
        )?
        .value(),
        "/a?existing%5B0%5D=one&list%5B0%5D=1&d.key=x&nested%5Ba%5D=v&nested%5Bb%5D=add&bool="
    );
    assert_eq!(
        Uri::of("/a?d.key=x&z=v")?.without_query(&["z"])?.value(),
        "/a?d%5Bkey%5D=x"
    );
    assert!(
        Uri::of("ftp://example.test/a")?
            .with_query(&json!({"x": 1}))
            .is_err()
    );

    let raw = Uri::of("/a?q=%FF")?;
    assert_eq!(raw.query().decode_bytes(), b"q=\xff");
    assert!(raw.query().decode().is_err());
    assert!(raw.query().to_array().is_err());
    assert_eq!(
        Uri::of("/q=hello%20world?q=hello%20world#q=hello%20world")?.decode()?,
        "/q=hello world?q=hello world#q=hello world"
    );
    Ok(())
}

#[test]
fn uri_query_rejects_input_and_output_beyond_shared_limits() -> Result<()> {
    let fields = (0..1025)
        .map(|i| format!("a{i}=v"))
        .collect::<Vec<_>>()
        .join("&");
    assert!(
        Uri::of(&format!("/a?{fields}"))?
            .query()
            .to_array()
            .is_err()
    );
    assert!(
        Uri::of("/a")?
            .with_query(&json!({"key": " ".repeat(45000)}))
            .is_err()
    );
    Ok(())
}

#[test]
fn uri_component_edits_match_fixed_laravel_and_league() -> Result<()> {
    // 固定 Laravel SupportUriTest 与 League be89e2382e37acdef232259071ade5180821397b。
    let oracle: serde_json::Value = serde_json::from_str(include_str!("components_oracle.json"))
        .map_err(|error| Error::custom("uri_component_fixture", error.to_string()))?;
    for (label, row) in oracle["rows"].as_object().ok_or_else(fixture_error)? {
        let uri = Uri::of(row["input"].as_str().ok_or_else(fixture_error)?)?;
        let edited = apply_component_edit(&uri, row);
        if row["error"].is_string() {
            assert_eq!(
                edited.err().as_ref().map(Error::code),
                Some("support_uri_invalid"),
                "{label}"
            );
        } else {
            let edited = edited?;
            assert_eq!(edited.value(), row["value"], "{label}");
            assert_eq!(json!(edited.authority()), row["authority"], "{label}");
            assert_eq!(json!(edited.user()), row["user"], "{label}");
            assert_eq!(json!(edited.user_info()), row["user_info"], "{label}");
            assert_eq!(json!(edited.path()), row["path"], "{label}");
            assert_eq!(json!(edited.port()), row["port"], "{label}");
            assert_eq!(json!(edited.fragment()), row["fragment"], "{label}");
        }
        assert_eq!(uri.value(), row["original"], "{label}");
    }
    let composed = Uri::of("")?
        .with_host("laravel.com")?
        .with_scheme("https")?
        .with_user(Some("taylor"), Some("password"))?
        .with_path("/docs/installation")?
        .with_port(Some(80))?
        .with_query(&json!({"version": 1}))?
        .with_fragment("hello")?;
    assert_eq!(composed.value(), oracle["chain"]);
    Ok(())
}

fn apply_component_edit(uri: &Uri, row: &serde_json::Value) -> Result<Uri> {
    let first = &row["args"][0];
    let required = || first.as_str().ok_or_else(fixture_error);
    match row["method"].as_str() {
        Some("withScheme") => uri.with_scheme(required()?),
        Some("withHost") => uri.with_host(required()?),
        Some("withUser") => uri.with_user(first.as_str(), row["args"][1].as_str()),
        Some("withPort") => uri.with_port(first.as_i64()),
        Some("withPath") => uri.with_path(required()?),
        Some("withFragment") => uri.with_fragment(required()?),
        Some("withoutFragment") => uri.without_fragment(),
        _ => Err(fixture_error()),
    }
}
