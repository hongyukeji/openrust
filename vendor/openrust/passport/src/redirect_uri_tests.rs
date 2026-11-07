//! 固定上游注册匹配案例与授权请求/交换请求的不同绑定规则。

use super::*;

fn registered(value: &str) -> Vec<String> {
    vec![value.to_owned()]
}

#[test]
fn fixed_league_redirect_validator_examples() {
    for (allowed, requested, valid) in [
        (
            "https://example.com:8443/endpoint",
            "https://example.com/endpoint",
            false,
        ),
        (
            "https://example.com:8443/endpoint",
            "https://example.com:8443/endpoint",
            true,
        ),
        (
            "http://127.0.0.1:8443/endpoint",
            "http://127.0.0.1:8443/different/endpoint",
            false,
        ),
        (
            "http://127.0.0.1:8443/endpoint",
            "http://127.0.0.1:8080/endpoint",
            true,
        ),
        (
            "http://[::1]:8443/endpoint",
            "http://[::1]:8080/endpoint",
            true,
        ),
        (
            "urn:ietf:wg:oauth:2.0:oob",
            "urn:ietf:wg:oauth:2.0:oob",
            true,
        ),
        ("msal://redirect", "msal://redirect", true),
        (
            "com.example.app:/oauth2redirect/example-provider",
            "com.example.app:/oauth2redirect/example-provider",
            true,
        ),
    ] {
        assert_eq!(
            matches_registered_redirect_uri(requested, &registered(allowed)),
            valid
        );
    }
}

#[test]
fn non_loopback_requires_exact_registered_bytes() {
    for requested in [
        "https://EXAMPLE.com/callback?x=%2F",
        "https://example.com:443/callback?x=%2F",
        "https://example.com/callback?x=%2f",
        "https://example.com/a/../callback?x=%2F",
        "https://example.com/callback?x=%2F#fragment",
        "https://example.com/callback?x=%2F&other=1",
    ] {
        assert!(!matches_registered_redirect_uri(
            requested,
            &registered("https://example.com/callback?x=%2F")
        ));
    }
}

#[test]
fn loopback_exception_does_not_expand_hosts_or_change_other_components() {
    let allowed = registered("http://127.0.0.1:8000/callback?x=1#fragment");
    assert!(matches_registered_redirect_uri(
        "http://127.0.0.1:9000/callback?x=1#fragment",
        &allowed
    ));
    for requested in [
        "http://localhost:9000/callback?x=1#fragment",
        "http://127.1:9000/callback?x=1#fragment",
        "http://127.0.0.2:9000/callback?x=1#fragment",
        "https://127.0.0.1:9000/callback?x=1#fragment",
        "http://127.0.0.1.evil:9000/callback?x=1#fragment",
        "http://127.0.0.1:9000/a/../callback?x=1#fragment",
        "http://127.0.0.1:9000/callback?x=2#fragment",
        "http://127.0.0.1:9000/callback?x=1#different",
        "http://user@127.0.0.1:9000/callback?x=1#fragment",
    ] {
        assert!(!matches_registered_redirect_uri(requested, &allowed));
    }
    assert!(!matches_registered_redirect_uri(
        "https://127.0.0.1:9000/callback",
        &registered("https://127.0.0.1:8000/callback")
    ));
}

#[test]
fn fragment_is_not_silently_removed_or_globally_forbidden() -> Result<(), RedirectUriError> {
    let allowed = registered("https://example.com/callback#registered");
    let target = ValidatedRedirectUri::from_authorization_request(Some(&allowed[0]), &allowed)?;
    assert_eq!(target.as_str(), allowed[0]);
    assert!(
        ValidatedRedirectUri::from_authorization_request(
            Some("https://example.com/callback#other"),
            &allowed
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn missing_authorization_uri_selects_only_single_registered_array_item()
-> Result<(), RedirectUriError> {
    let target = ValidatedRedirectUri::from_authorization_request(
        None,
        &registered("https://example.com/callback"),
    )?;
    assert_eq!(target.as_str(), "https://example.com/callback");
    assert_eq!(target.requested_uri(), None);
    assert_eq!(
        ValidatedRedirectUri::from_authorization_request(None, &[]),
        Err(RedirectUriError::InvalidClient)
    );
    let duplicate = vec!["https://example.com/callback".to_owned(); 2];
    assert_eq!(
        ValidatedRedirectUri::from_authorization_request(None, &duplicate),
        Err(RedirectUriError::InvalidClient)
    );
    // 固定 Passport 暴露数组；League 的数组分支只检查元素数量。
    assert_eq!(
        ValidatedRedirectUri::from_authorization_request(None, &registered(""))?.as_str(),
        ""
    );
    Ok(())
}

#[test]
fn authorization_normalizes_only_php_trim_characters() -> Result<(), RedirectUriError> {
    let allowed = registered("https://example.com/callback");
    let target = ValidatedRedirectUri::from_authorization_request(
        Some("\0 \thttps://example.com/callback\r\n\u{b}"),
        &allowed,
    )?;
    assert_eq!(target.requested_uri(), Some(allowed[0].as_str()));
    assert_eq!(
        ValidatedRedirectUri::from_authorization_request(
            Some("\u{c}https://example.com/callback"),
            &allowed
        ),
        Err(RedirectUriError::InvalidClient)
    );
    Ok(())
}

#[test]
fn code_exchange_requires_original_option_and_exact_bytes() {
    let stored = Some("http://127.0.0.1:8000/callback");
    assert_eq!(validate_token_redirect_uri(stored, stored), Ok(()));
    assert_eq!(
        validate_token_redirect_uri(stored, None),
        Err(RedirectUriError::MissingTokenRedirectUri)
    );
    assert_eq!(
        validate_token_redirect_uri(stored, Some("http://127.0.0.1:9000/callback")),
        Err(RedirectUriError::TokenRedirectUriMismatch)
    );
    assert_eq!(validate_token_redirect_uri(None, None), Ok(()));
    assert_eq!(
        validate_token_redirect_uri(None, stored),
        Err(RedirectUriError::TokenRedirectUriMismatch)
    );
    assert_eq!(
        validate_token_redirect_uri(stored, Some("\0 http://127.0.0.1:8000/callback \n")),
        Ok(())
    );
}

#[test]
fn invalid_target_errors_and_debug_do_not_expose_request_values() -> Result<(), RedirectUriError> {
    let allowed = registered("https://example.com/secret");
    let target = ValidatedRedirectUri::from_authorization_request(Some(&allowed[0]), &allowed)?;
    assert!(!format!("{target:?}").contains("secret"));
    let error = ValidatedRedirectUri::from_authorization_request(
        Some("https://attacker.example/secret"),
        &allowed,
    );
    assert_eq!(error, Err(RedirectUriError::InvalidClient));
    assert!(!format!("{error:?}").contains("attacker"));
    Ok(())
}

#[test]
fn client_array_non_loopback_ignores_heterogeneous_members_without_coercion()
-> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    for values in [
        json!([true, 1, null, [], {"nested":"value"}, "https://client.example/callback"]),
        json!({"bad":false,"selected":"https://client.example/callback","tail":42}),
    ] {
        let registered = crate::PhpArray::try_from(values)?;
        let selected = ValidatedRedirectUri::from_client_array(
            Some("https://client.example/callback"),
            &registered,
        )?;
        assert_eq!(selected.as_str(), "https://client.example/callback");
        assert_eq!(
            selected.requested_uri(),
            Some("https://client.example/callback")
        );
        assert_eq!(
            ValidatedRedirectUri::from_client_array(
                Some("https://attacker.example/secret"),
                &registered
            ),
            Err(RedirectUriError::InvalidClient)
        );
    }
    let registered = crate::PhpArray::try_from(json!([true, 1, null]))?;
    assert_eq!(
        ValidatedRedirectUri::from_client_array(Some("1"), &registered),
        Err(RedirectUriError::InvalidClient)
    );
    Ok(())
}

#[test]
fn client_array_default_uses_numeric_zero_instead_of_first_associative_value()
-> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    for values in [
        json!({"foo":"https://client.example/callback"}),
        json!({"1":"https://client.example/callback"}),
        json!({"00":"https://client.example/callback"}),
    ] {
        let registered = crate::PhpArray::try_from(values)?;
        assert_eq!(
            ValidatedRedirectUri::from_client_array(None, &registered),
            Err(RedirectUriError::MalformedRegistration)
        );
        // 显式请求做成员匹配，不要求成员的键是0。
        assert!(
            ValidatedRedirectUri::from_client_array(
                Some("https://client.example/callback"),
                &registered
            )
            .is_ok()
        );
    }
    for values in [
        json!(["https://client.example/callback"]),
        json!({"0":"https://client.example/callback"}),
    ] {
        let registered = crate::PhpArray::try_from(values)?;
        let selected = ValidatedRedirectUri::from_client_array(None, &registered)?;
        assert_eq!(selected.as_str(), "https://client.example/callback");
        assert_eq!(selected.requested_uri(), None);
    }
    Ok(())
}

#[test]
fn client_array_default_count_check_precedes_element_type_check()
-> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    for values in [
        json!([]),
        json!({}),
        json!([false, "https://client.example/callback"]),
        json!({"foo":false,"bar":null}),
    ] {
        let registered = crate::PhpArray::try_from(values)?;
        assert_eq!(
            ValidatedRedirectUri::from_client_array(None, &registered),
            Err(RedirectUriError::InvalidClient)
        );
    }
    for values in [
        json!([false]),
        json!([1]),
        json!([null]),
        json!([[]]),
        json!([{"nested":"https://client.example/callback"}]),
    ] {
        let registered = crate::PhpArray::try_from(values)?;
        assert_eq!(
            ValidatedRedirectUri::from_client_array(None, &registered),
            Err(RedirectUriError::MalformedRegistration)
        );
    }
    // 固定数组分支没有拒绝空字符串默认，不能把它当作scalar-registration分支。
    let registered = crate::PhpArray::try_from(json!([""]))?;
    assert_eq!(
        ValidatedRedirectUri::from_client_array(None, &registered)?.as_str(),
        ""
    );
    Ok(())
}

#[test]
fn client_array_loopback_non_string_failure_occurs_only_when_iteration_reaches_it()
-> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    for malformed in [
        json!(false),
        json!(42),
        json!(null),
        json!([]),
        json!({"nested":"value"}),
    ] {
        let before = crate::PhpArray::try_from(json!([
            malformed.clone(),
            "http://127.0.0.1:8000/callback"
        ]))?;
        assert_eq!(
            ValidatedRedirectUri::from_client_array(
                Some("http://127.0.0.1:9000/callback"),
                &before
            ),
            Err(RedirectUriError::MalformedRegistration)
        );
        let after =
            crate::PhpArray::try_from(json!(["http://127.0.0.1:8000/callback", malformed]))?;
        let selected = ValidatedRedirectUri::from_client_array(
            Some("http://127.0.0.1:9000/callback"),
            &after,
        )?;
        assert_eq!(selected.as_str(), "http://127.0.0.1:9000/callback");
    }
    let registered = crate::PhpArray::try_from(json!([
        "http://127.0.0.1:8000/other",
        false,
        "http://127.0.0.1:8000/callback"
    ]))?;
    assert_eq!(
        ValidatedRedirectUri::from_client_array(
            Some("http://127.0.0.1:9000/callback"),
            &registered
        ),
        Err(RedirectUriError::MalformedRegistration)
    );
    let registered = crate::PhpArray::try_from(json!(["http://127.0.0.1:8000/other"]))?;
    assert_eq!(
        ValidatedRedirectUri::from_client_array(
            Some("http://127.0.0.1:9000/callback"),
            &registered
        ),
        Err(RedirectUriError::InvalidClient)
    );
    Ok(())
}

#[test]
fn client_array_strict_uri_matching_keeps_registered_bytes_and_request_trim_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    use serde_json::json;
    let registered =
        crate::PhpArray::try_from(json!([true, "https://client.example/callback?path=%2F"]))?;
    for requested in [
        "https://CLIENT.example/callback?path=%2F",
        "https://client.example:443/callback?path=%2F",
        "https://client.example/callback?path=%2f",
        "https://client.example/callback?path=%2F#other",
    ] {
        assert_eq!(
            ValidatedRedirectUri::from_client_array(Some(requested), &registered),
            Err(RedirectUriError::InvalidClient)
        );
    }
    assert_eq!(
        ValidatedRedirectUri::from_client_array(
            Some("\0 https://client.example/callback?path=%2F\n"),
            &registered
        )?
        .as_str(),
        "https://client.example/callback?path=%2F"
    );
    let whitespace_registration =
        crate::PhpArray::try_from(json!([" https://client.example/callback "]))?;
    assert_eq!(
        ValidatedRedirectUri::from_client_array(
            Some("https://client.example/callback"),
            &whitespace_registration
        ),
        Err(RedirectUriError::InvalidClient)
    );
    assert!(!format!("{:?}", RedirectUriError::MalformedRegistration).contains("client.example"));
    Ok(())
}
