//! 实际PHP8.5.12固定方法体的Basic oracle；最小header夹具不代表HTTP接线验收。

use super::{basic_credentials, decode_basic_payload};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct Fixture {
    php_version: String,
    actual_stdout_sha256: String,
    input_sha256: String,
    script_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    has_authorization: bool,
    authorization_values_base64: Vec<String>,
    decoder: BytesValue,
    credentials: Credentials,
}

#[derive(Deserialize)]
struct BytesValue {
    kind: String,
    base64: Option<String>,
}

#[derive(Deserialize)]
struct Credentials {
    kind: String,
    username: Option<BytesValue>,
    password: Option<BytesValue>,
}

fn fixture() -> TestResult<Fixture> {
    let fixture: Fixture = serde_json::from_str(include_str!("basic_credentials_oracle.json"))?;
    assert_eq!(fixture.php_version, "8.5.12");
    assert_eq!(
        fixture.actual_stdout_sha256,
        "f2ed1ab60c66ba699eaf933ec49f7a34a12a37b13011319fba5022bd4f312910"
    );
    assert_eq!(
        fixture.input_sha256,
        "b4472e01d801e837e6befba3a6ca321111e51f1e57da8326a6b41d936040a473"
    );
    assert_eq!(
        fixture.script_sha256,
        "c7884eeaa2ac57af9cf18d27d161c234c6b05ac6f4b5d2ba5b3c4aac0927f645"
    );
    assert_eq!(fixture.cases.len(), 56);
    Ok(fixture)
}

fn invalid_fixture() -> openrust::Error {
    openrust::Error::custom(
        "basic_oracle_fixture_invalid",
        "Basic oracle字段或覆盖分类无效",
    )
}

fn first_header(case: &Case) -> TestResult<Option<Vec<u8>>> {
    if !case.has_authorization {
        return Ok(None);
    }
    let first = case
        .authorization_values_base64
        .first()
        .ok_or_else(invalid_fixture)?;
    Ok(Some(STANDARD.decode(first)?))
}

fn expected_bytes(value: &BytesValue) -> TestResult<Option<Vec<u8>>> {
    match value.kind.as_str() {
        "null" => Ok(None),
        "bytes" => Ok(Some(
            STANDARD.decode(value.base64.as_deref().ok_or_else(invalid_fixture)?)?,
        )),
        _ => Err(invalid_fixture().into()),
    }
}

#[test]
fn actual_php_decoder_matches_all_46_attempted_byte_and_rejection_cases() -> TestResult {
    let fixture = fixture()?;
    let mut accepted = 0;
    let mut rejected = 0;
    let mut not_attempted = 0;
    for case in &fixture.cases {
        if case.decoder.kind == "null" {
            not_attempted += 1;
            continue;
        }
        let header = first_header(case)?.ok_or_else(invalid_fixture)?;
        let payload = header.get(6..).ok_or_else(invalid_fixture)?;
        let decoded = decode_basic_payload(payload);
        match case.decoder.kind.as_str() {
            "bytes" => {
                let expected = expected_bytes(&case.decoder)?.ok_or_else(invalid_fixture)?;
                assert!(
                    decoded.is_ok_and(|bytes| bytes == expected),
                    "decoder字节结果不匹配：{}",
                    case.id
                );
                accepted += 1;
            }
            "false" => {
                assert!(decoded.is_err(), "decoder拒绝边界不匹配：{}", case.id);
                rejected += 1;
            }
            _ => return Err(invalid_fixture().into()),
        }
    }
    assert_eq!((accepted, rejected, not_attempted), (30, 16, 10));
    Ok(())
}

#[test]
fn actual_php_basic_matches_all_55_byte_cases_and_preserves_old_boundary_evidence() -> TestResult {
    let fixture = fixture()?;
    let mut comparable = 0;
    let mut binary_boundary = 0;
    let mut synthetic_non_psr = 0;
    for case in &fixture.cases {
        if case.credentials.kind == "exception" {
            assert_eq!(case.id, "synthetic_present_empty_values");
            synthetic_non_psr += 1;
            continue;
        }
        assert_eq!(case.credentials.kind, "pair");
        let header = first_header(case)?;
        let actual = basic_credentials(header.as_deref());
        let username = expected_bytes(
            case.credentials
                .username
                .as_ref()
                .ok_or_else(invalid_fixture)?,
        )?;
        let password = expected_bytes(
            case.credentials
                .password
                .as_ref()
                .ok_or_else(invalid_fixture)?,
        )?;
        let non_utf8 = [&username, &password].into_iter().any(|value| {
            value
                .as_deref()
                .is_some_and(|bytes| std::str::from_utf8(bytes).is_err())
        });
        if non_utf8 {
            assert!(matches!(
                case.id.as_str(),
                "binary_username" | "binary_password"
            ));
            binary_boundary += 1;
        }
        let actual = (
            actual.0.as_ref().map(|value| value.as_slice()),
            actual.1.as_ref().map(|value| value.as_slice()),
        );
        let expected = (username.as_deref(), password.as_deref());
        // 不打印凭据；旧整对UTF8拒绝断言完整保存在provenance前像，当前授权扩域为bytes。
        assert!(actual == expected, "Basic解析不匹配：{}", case.id);
        comparable += 1;
    }
    assert_eq!((comparable, binary_boundary, synthetic_non_psr), (55, 2, 1));
    Ok(())
}
