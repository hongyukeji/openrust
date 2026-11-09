//! 固定 League Bearer 输入与签名/时间验证的定向回归。

use super::*;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use openrust::{
    Error,
    http::axum::{
        body::Body,
        extract::FromRequest,
        http::{Request as Wire, StatusCode},
        response::IntoResponse as _,
    },
    support::calendar::CalendarInstant,
};
use serde_json::{Value, json};

use crate::test_keys::{OTHER_PUBLIC, PRIVATE, PUBLIC};

type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(crate) async fn resource_request(headers: &[&str]) -> openrust::Result<Request> {
    let mut wire = Wire::builder().uri("/oauth/resource");
    for value in headers {
        wire = wire.header(AUTHORIZATION, *value);
    }
    let wire = wire.body(Body::empty()).map_err(Error::internal)?;
    Request::from_request(wire, &()).await
}

pub(crate) fn resource_token(
    jti: &str,
    client: &str,
    user: Option<&str>,
    scopes: &[String],
) -> openrust::Result<String> {
    let now = u64::try_from(SystemClock.now()?.epoch_seconds()).map_err(Error::internal)?;
    let signer = crate::OAuthAccessSigner::from_pem(PRIVATE).map_err(Error::internal)?;
    signer
        .sign_at(client, jti, user, scopes, now, now + 300)
        .map_err(Error::internal)
}

pub(crate) fn assert_unprojected(request: &Request) -> openrust::Result<()> {
    for key in [
        "oauth_access_token_id",
        "oauth_client_id",
        "oauth_user_id",
        "oauth_scopes",
    ] {
        assert!(request.attributes().get(key)?.is_none(), "{key}");
    }
    Ok(())
}

pub(crate) fn projected(request: &Request, key: &str) -> openrust::Result<ParameterValue> {
    request.attributes().get(key)?.ok_or_else(|| {
        Error::custom(
            "resource_fixture_attribute_missing",
            format!("缺少请求属性 {key}"),
        )
    })
}

pub(crate) async fn rejected(
    server: &ResourceServer,
    request: &Request,
) -> openrust::Result<OAuthError> {
    server
        .validate_authenticated_request(request)
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "resource_fixture_expected_rejection",
                "无有效授权的资源请求通过验证",
            )
        })
}

pub(crate) fn signed(value: &Value) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::new(Algorithm::RS256),
        value,
        &EncodingKey::from_rsa_pem(PRIVATE)?,
    )
}

#[test]
fn first_bearer_value_uses_fixed_prefix_and_trim_rules() -> TestResult {
    assert_eq!(bearer(Some(b" \tbeARer\t compact \t"))?, "compact");
    assert_eq!(bearer(Some(b"compact"))?, "compact");
    assert_eq!(bearer(Some(b"Bearercompact"))?, "Bearercompact");
    for (header, hint) in [
        (None, "Missing \"Authorization\" header"),
        (Some(b"Bearer\t  ".as_slice()), "Missing \"Bearer\" token"),
        (
            Some(b"\xff".as_slice()),
            "Access token could not be verified",
        ),
    ] {
        let error = bearer(header).err().ok_or_else(|| {
            Error::custom(
                "resource_fixture_expected_rejection",
                "无有效授权值的头通过验证",
            )
        })?;
        assert_eq!(serde_json::to_value(&error)?["hint"], hint);
        assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

#[test]
fn compact_shape_errors_keep_fixed_safe_hints() -> TestResult {
    check_compact_parts("header.claims.signature")?;
    for (token, hint) in [
        ("header.claims", "The JWT string must have two dots"),
        (
            "header.claims.signature.extra",
            "The JWT string must have two dots",
        ),
        (
            ".claims.signature",
            "The JWT string is missing the Header part",
        ),
        (
            "header..signature",
            "The JWT string is missing the Claim part",
        ),
        (
            "header.claims.",
            "The JWT string is missing the Signature part",
        ),
    ] {
        let error = check_compact_parts(token).err().ok_or_else(|| {
            Error::custom(
                "resource_fixture_expected_rejection",
                "不完整 JWT 通过结构校验",
            )
        })?;
        assert_eq!(serde_json::to_value(&error)?["hint"], hint);
    }
    Ok(())
}

#[test]
fn resource_claims_use_present_dates_and_keep_audience_order() -> TestResult {
    let verifier = OAuthAccessVerifier::from_pem(PUBLIC)?;
    let now = CalendarInstant::new(100, 500_000)?;
    let token = signed(&json!({
        "jti": "resource", "aud": ["first", "second"], "iss": "arbitrary",
        "iat": "100.500000", "nbf": 100.5, "exp": "100.500001"
    }))?;
    let claims = verifier.verify_resource_at(&token, now)?;
    assert_eq!(claims.aud, ["first", "second"]);
    assert_eq!(claims.jti, "resource");
    assert!(claims.sub.is_none());
    assert!(claims.scopes.is_none());
    assert!(verifier.verify_at(&token, "first", 100).is_err());
    assert!(
        OAuthAccessVerifier::from_pem(OTHER_PUBLIC)?
            .verify_resource_at(&token, now)
            .is_err()
    );

    let minimal = signed(&json!({"jti": "resource", "aud": "only"}))?;
    assert_eq!(verifier.verify_resource_at(&minimal, now)?.aud, ["only"]);
    for invalid in [
        json!({"jti":"resource", "aud":"only", "iat":100.500_001}),
        json!({"jti":"resource", "aud":"only", "nbf":100.500_001}),
        json!({"jti":"resource", "aud":"only", "exp":100.5}),
        json!({"jti":"resource", "aud":"only", "exp":null}),
        json!({"jti":"resource", "aud":[]}),
        json!({"jti":"", "aud":"only"}),
        json!({"jti":"resource", "aud":"only", "sub":42}),
        json!({"jti":"resource", "aud":"only", "scopes":[42]}),
        json!({"jti":"resource", "aud":"only", "":true}),
        json!(["only", "resource", "user", ["scope"]]),
    ] {
        assert!(
            verifier
                .verify_resource_at(&signed(&invalid)?, now)
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn resource_validation_requires_rs256_even_with_a_valid_signature_of_another_algorithm()
-> TestResult {
    let verifier = OAuthAccessVerifier::from_pem(PUBLIC)?;
    let now = CalendarInstant::new(100, 0)?;
    let claims = json!({"jti":"resource", "aud":"only"});
    for (algorithm, key) in [
        (Algorithm::HS256, EncodingKey::from_secret(PUBLIC)),
        (Algorithm::RS512, EncodingKey::from_rsa_pem(PRIVATE)?),
    ] {
        let token = encode(&Header::new(algorithm), &claims, &key)?;
        assert!(verifier.verify_resource_at(&token, now).is_err());
    }
    Ok(())
}
