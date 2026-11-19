use openrust_passport::{
    RedirectUriError, ValidatedRedirectUri, matches_registered_redirect_uri,
    validate_token_redirect_uri,
};
#[test]
fn fixed_php_league_redirect_corpus() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/redirect_oracle.json")).unwrap();
    let mut failures = Vec::new();
    for row in fixture["rows"].as_array().unwrap() {
        let allowed = row["registered"].as_str().unwrap();
        let requested = row["requested"].as_str().unwrap();
        let Some(expected) = row["result"].as_bool() else {
            let array = openrust_passport::PhpArray::from(vec![allowed.into()]);
            assert_eq!(
                ValidatedRedirectUri::from_client_array(Some(requested), &array),
                Err(RedirectUriError::MalformedRegistration)
            );
            continue;
        };
        let actual = matches_registered_redirect_uri(requested, &[allowed.into()]);
        if actual != expected {
            failures.push((allowed, requested, expected));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches; first {:?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
}
#[test]
fn authorization_and_exchange_use_distinct_redirect_binding_rules() {
    let registered = vec!["http://127.0.0.1:8000/cb".into()];
    let chosen = ValidatedRedirectUri::from_authorization_request(
        Some("http://127.0.0.1:54321/cb"),
        &registered,
    )
    .unwrap();
    assert_eq!(chosen.as_str(), "http://127.0.0.1:54321/cb");
    assert_eq!(
        validate_token_redirect_uri(chosen.requested_uri(), Some("http://127.0.0.1:8000/cb")),
        Err(RedirectUriError::TokenRedirectUriMismatch)
    );
    assert_eq!(
        validate_token_redirect_uri(chosen.requested_uri(), None),
        Err(RedirectUriError::MissingTokenRedirectUri)
    );
    assert!(
        ValidatedRedirectUri::from_authorization_request(
            None,
            &["https://one.test".into(), "https://two.test".into()]
        )
        .is_err()
    );
}
