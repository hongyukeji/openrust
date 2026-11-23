use crate::{
    ProviderConfig,
    identity::{Expected, verify},
    provider::now,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, jwk::JwkSet};
use serde_json::{Value, json};
fn config() -> ProviderConfig {
    ProviderConfig::from_config(&json!({"issuer":"https://op.test","client_id":"client","redirect":"https://rp.test/callback"})).unwrap()
}
fn claims() -> Value {
    let t = now().unwrap();
    json!({"iss":"https://op.test","sub":"007","aud":"client","exp":t+300,"iat":t,"nonce":"expected","email":"a@test","email_verified":true})
}
fn keys() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/keys.json")).unwrap()["keys"].clone()
}
fn token(v: &Value, header: Header, second: bool) -> String {
    let pem = if second {
        include_bytes!("../fixtures/second.der").as_slice()
    } else {
        include_bytes!("../fixtures/private.der").as_slice()
    };
    encode(&header, v, &EncodingKey::from_rsa_der(pem)).unwrap()
}
fn header() -> Header {
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("test-key".into());
    h
}
fn check(
    v: &Value,
    h: Header,
    k: Value,
    second: bool,
    max_age: Option<u64>,
) -> crate::Result<crate::VerifiedIdentity> {
    let config = config();
    verify(
        &token(v, h, second),
        &serde_json::from_value::<JwkSet>(k).unwrap(),
        Expected {
            config: &config,
            nonce: "expected",
            code: "code",
            access_token: "access",
            algorithms: &[Algorithm::RS256],
            max_age,
        },
    )
}
#[test]
fn signature_claims_and_key_metadata_fail_closed() {
    let original = claims();
    let identity = check(&original, header(), keys(), false, None).unwrap();
    assert_eq!(identity.subject(), "007");
    assert!(identity.email_verified());
    assert!(!format!("{identity:?}").contains("a@test"));
    for field in ["iss", "sub", "aud", "exp", "iat", "nonce"] {
        let mut v = original.clone();
        v.as_object_mut().unwrap().remove(field);
        assert!(
            check(&v, header(), keys(), false, None).is_err(),
            "missing {field}"
        );
    }
    let t = now().unwrap();
    for (field, value) in [
        ("iss", json!("https://wrong.test")),
        ("sub", json!("")),
        ("sub", json!("é")),
        ("sub", json!("a\nb")),
        ("sub", json!("a".repeat(256))),
        ("aud", json!("other")),
        ("aud", json!([])),
        ("aud", json!(["client", "other"])),
        ("azp", json!("other")),
        ("nonce", json!("wrong")),
        ("exp", json!(t)),
        ("exp", json!("9999999999")),
        ("iat", json!(t + 120)),
        ("iat", json!(t - 1000)),
        ("nbf", json!(t + 120)),
        ("auth_time", json!(t + 120)),
        ("at_hash", json!("wrong")),
        ("c_hash", json!("wrong")),
        ("email_verified", json!("true")),
    ] {
        let mut v = original.clone();
        v[field] = value;
        assert!(
            check(&v, header(), keys(), false, None).is_err(),
            "invalid {field}: {v}"
        );
    }
    assert!(check(&original, header(), keys(), true, None).is_err());
    let mut h = header();
    h.kid = None;
    assert!(check(&original, h, keys(), false, None).is_err());
    let mut h = header();
    h.typ = Some("other".into());
    assert!(check(&original, h, keys(), false, None).is_err());
    let mut h = header();
    h.jku = Some("https://evil.test/keys".into());
    assert!(check(&original, h, keys(), false, None).is_err());
    let mut k = keys();
    let duplicate = k["keys"][0].clone();
    k["keys"].as_array_mut().unwrap().push(duplicate);
    assert!(check(&original, header(), k, false, None).is_err());
    for (field, value) in [
        ("use", json!("enc")),
        ("key_ops", json!(["sign"])),
        ("alg", json!("RS512")),
        ("kid", json!("other")),
    ] {
        let mut k = keys();
        k["keys"][0][field] = value;
        assert!(
            check(&original, header(), k, false, None).is_err(),
            "{field}"
        );
    }
    let config = config();
    let h = Header::new(Algorithm::HS256);
    let jwt = encode(&h, &original, &EncodingKey::from_secret(b"secret")).unwrap();
    assert!(
        verify(
            &jwt,
            &serde_json::from_value(keys()).unwrap(),
            Expected {
                config: &config,
                nonce: "expected",
                code: "code",
                access_token: "access",
                algorithms: &[Algorithm::HS256],
                max_age: None
            }
        )
        .is_err()
    );
}
#[test]
fn rotation_audience_auth_time_and_hash_claims() {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let mut v = claims();
    v["aud"] = json!(["client", "other"]);
    v["azp"] = json!("client");
    v["auth_time"] = json!(now().unwrap());
    for (field, input) in [("at_hash", "access"), ("c_hash", "code")] {
        let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, input.as_bytes());
        v[field] = json!(URL_SAFE_NO_PAD.encode(&digest.as_ref()[..16]));
    }
    check(&v, header(), keys(), false, Some(60)).unwrap();
    let rotated=serde_json::from_str::<Value>(include_str!("../fixtures/keys.json")).unwrap()["rotated_keys"].clone();
    check(&v, header(), rotated, true, Some(60)).unwrap();
    v["auth_time"] = json!(now().unwrap() - 100);
    assert!(check(&v, header(), keys(), false, Some(60)).is_err());
    v.as_object_mut().unwrap().remove("auth_time");
    assert!(check(&v, header(), keys(), false, Some(60)).is_err());
}
#[test]
fn configuration_rejects_unsafe_urls_algorithms_and_types() {
    let v = json!({"issuer":"https://op.test","client_id":"client","redirect":"https://rp.test/callback"});
    for (field, value) in [
        ("issuer", json!("http://op.test")),
        ("issuer", json!("https://op.test?q=x")),
        ("issuer", json!("https://u:p@op.test")),
        ("redirect", json!("https://rp.test/#fragment")),
        ("post_logout_redirect", json!("http://rp.test/exit")),
        ("post_logout_redirect", json!("https://u:p@rp.test/exit")),
        (
            "post_logout_redirect",
            json!("https://rp.test/exit#fragment"),
        ),
        (
            "post_logout_redirect",
            json!("https://rp.test/exit?state=preexisting"),
        ),
        ("post_logout_redirect", json!(false)),
        ("algorithms", json!(["HS256"])),
        ("algorithms", json!([])),
        ("client_auth", json!(false)),
        ("client_auth", json!("client_secret_basic")),
        ("attempt_ttl", json!(0)),
        ("timeout", json!(61)),
        ("userinfo", json!("true")),
        ("allowed_origins", json!(["https://other.test/path"])),
    ] {
        let mut c = v.clone();
        c[field] = value;
        assert!(ProviderConfig::from_config(&c).is_err(), "{field}");
    }
    let mut c = v;
    c["allow_loopback_http"] = json!(true);
    c["issuer"] = json!("http://127.0.0.1:1234");
    ProviderConfig::from_config(&c).unwrap();
    c["issuer"] = json!("http://localhost:1234");
    assert!(ProviderConfig::from_config(&c).is_err());
    c["issuer"] = json!("http://[::1]:1234");
    ProviderConfig::from_config(&c).unwrap();
}

#[test]
fn all_explicit_asymmetric_algorithms_verify_with_matching_hash_widths() {
    use aws_lc_rs::signature::{
        ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P384_SHA384_FIXED_SIGNING, EcdsaKeyPair,
        KeyPair as _,
    };
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    for alg in [
        Algorithm::RS256,
        Algorithm::RS384,
        Algorithm::RS512,
        Algorithm::PS256,
        Algorithm::PS384,
        Algorithm::PS512,
        Algorithm::ES256,
        Algorithm::ES384,
    ] {
        let label = format!("{alg:?}");
        let config=ProviderConfig::from_config(&json!({"issuer":"https://op.test","client_id":"client","redirect":"https://rp.test/callback","algorithms":[label]})).unwrap();
        let (encoding, jwks) = if matches!(alg, Algorithm::ES256 | Algorithm::ES384) {
            let signing = if alg == Algorithm::ES256 {
                &ECDSA_P256_SHA256_FIXED_SIGNING
            } else {
                &ECDSA_P384_SHA384_FIXED_SIGNING
            };
            let pkcs8 =
                EcdsaKeyPair::generate_pkcs8(signing, &aws_lc_rs::rand::SystemRandom::new())
                    .unwrap();
            let pair = EcdsaKeyPair::from_pkcs8(signing, pkcs8.as_ref()).unwrap();
            let public = pair.public_key().as_ref();
            let width = (public.len() - 1) / 2;
            (
                EncodingKey::from_ec_der(pkcs8.as_ref()),
                json!({"keys":[{"kty":"EC","kid":"test-key","alg":label,"crv":if alg==Algorithm::ES256{"P-256"}else{"P-384"},"x":URL_SAFE_NO_PAD.encode(&public[1..1+width]),"y":URL_SAFE_NO_PAD.encode(&public[1+width..])}]}),
            )
        } else {
            let mut k = keys();
            k["keys"][0]["alg"] = json!(label);
            (
                EncodingKey::from_rsa_der(include_bytes!("../fixtures/private.der")),
                k,
            )
        };
        let mut v = claims();
        let digest_alg = match alg {
            Algorithm::RS256 | Algorithm::PS256 | Algorithm::ES256 => &aws_lc_rs::digest::SHA256,
            Algorithm::RS384 | Algorithm::PS384 | Algorithm::ES384 => &aws_lc_rs::digest::SHA384,
            _ => &aws_lc_rs::digest::SHA512,
        };
        for (field, input) in [("at_hash", "access"), ("c_hash", "code")] {
            let digest = aws_lc_rs::digest::digest(digest_alg, input.as_bytes());
            v[field] = json!(URL_SAFE_NO_PAD.encode(&digest.as_ref()[..digest.as_ref().len() / 2]));
        }
        let mut header = Header::new(alg);
        header.kid = Some("test-key".into());
        let token = encode(&header, &v, &encoding).unwrap();
        verify(
            &token,
            &serde_json::from_value(jwks).unwrap(),
            Expected {
                config: &config,
                nonce: "expected",
                code: "code",
                access_token: "access",
                algorithms: &[alg],
                max_age: None,
            },
        )
        .unwrap();
    }
}
