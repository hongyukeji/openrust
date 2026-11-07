//! 原始字节贯通与实际 PHP 8.5 低成本 fixture；不以随机盐文本相等作验收。

use super::{
    Argon2IdHasher, ArgonConfig, ArgonHasher, BcryptConfig, BcryptHasher, HashInfo, HashManager,
    HashOptions, Hasher, HashingConfig,
};
use crate::core::Result;
use crate::support::BoxFuture;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct CaptureHasher {
    seen: Mutex<Vec<Vec<u8>>>,
}
impl CaptureHasher {
    fn capture(&self, value: &[u8]) -> Result<()> {
        self.seen
            .lock()
            .map_err(|_| super::failure("test_lock", "测试记录锁失效"))?
            .push(value.to_vec());
        Ok(())
    }
}
impl Hasher for CaptureHasher {
    fn accepts_exact_input(&self, value: &[u8]) -> Result<bool> {
        self.capture(value)?;
        Ok(true)
    }
    fn info(&self, _encoded: &str) -> Result<HashInfo> {
        Ok(HashInfo {
            algo: None,
            algo_name: "capture".into(),
            options: HashOptions::new(),
        })
    }
    fn make<'a>(
        &'a self,
        value: &'a [u8],
        _options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.capture(value)?;
            Ok("custom-encoded".into())
        })
    }
    fn check<'a>(
        &'a self,
        value: &'a [u8],
        encoded: Option<&'a str>,
        _options: &'a HashOptions,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.capture(value)?;
            Ok(encoded == Some("custom-encoded"))
        })
    }
    fn needs_rehash(&self, _encoded: &str, _options: &HashOptions) -> Result<bool> {
        Ok(false)
    }
}

struct ByteSecret(Vec<u8>);
impl AsRef<[u8]> for ByteSecret {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[tokio::test]
async fn custom_trait_object_and_manager_preserve_every_plaintext_byte() -> Result<()> {
    let capture = Arc::new(CaptureHasher {
        seen: Mutex::new(Vec::new()),
    });
    let driver = Arc::clone(&capture) as Arc<dyn Hasher>;
    let manager = HashManager::new(HashingConfig {
        driver: "capture".into(),
        ..HashingConfig::default()
    })?;
    let factory_driver = Arc::clone(&driver);
    manager.extend("capture", move || Ok(Arc::clone(&factory_driver)))?;
    let options = HashOptions::new();
    let mut long = vec![b'x'; 73];
    long.extend([0, 255]);
    let inputs = [vec![0], vec![255], vec![b'a', 0, b'b'], long];
    let mut expected = Vec::new();
    for value in inputs {
        assert!(driver.accepts_exact_input(&value)?);
        assert_eq!(driver.make(&value, &options).await?, "custom-encoded");
        assert!(
            driver
                .check(&value, Some("custom-encoded"), &options)
                .await?
        );
        assert!(manager.accepts_exact_input(ByteSecret(value.clone()))?);
        assert_eq!(
            manager.make(ByteSecret(value.clone()), &options).await?,
            "custom-encoded"
        );
        assert!(
            manager
                .check(ByteSecret(value.clone()), Some("custom-encoded"), &options)
                .await?
        );
        expected.extend(std::iter::repeat_n(value, 6));
    }
    manager.make("text", &options).await?;
    manager.make(String::from("text"), &options).await?;
    expected.extend([b"text".to_vec(), b"text".to_vec()]);
    assert_eq!(
        *capture
            .seen
            .lock()
            .map_err(|_| super::failure("test_lock", "测试记录锁失效"))?,
        expected
    );
    Ok(())
}

#[derive(Deserialize)]
struct Fixture {
    profiles: BTreeMap<String, Profile>,
    inputs: Vec<Input>,
    make: Vec<Make>,
    check: Vec<Check>,
}
#[derive(Deserialize)]
struct Profile {
    algorithm: String,
    capability: String,
    requested_fixture_options: Value,
    official_defaults_changed: bool,
}
#[derive(Deserialize)]
struct Input {
    id: String,
    bytes_len: usize,
    base64: String,
}
#[derive(Deserialize)]
struct Make {
    profile: String,
    case: String,
    algorithm_capability: String,
    result: Value,
}
#[derive(Deserialize)]
struct Check {
    slot: usize,
    invoke_original_or_rust_checker: bool,
    checker_profile: String,
    hash_profile: Option<String>,
    hash_case: Option<String>,
    hash_fixture: Option<String>,
    group: String,
    candidate_case: String,
    result: Value,
    encoded_hash: Option<String>,
    hash_is_public_synthetic_fixture: bool,
}

fn fixture_driver(profile: &Profile) -> TestResult<Arc<dyn Hasher>> {
    assert_eq!(profile.capability, "available");
    assert!(!profile.official_defaults_changed);
    let options = &profile.requested_fixture_options;
    let verify = options["verify"].as_bool().ok_or("missing verify")?;
    if profile.algorithm == "bcrypt" {
        assert_eq!(options["rounds"], 4);
        let limit = match &options["limit"] {
            Value::Null => None,
            value if value == &Value::from(72) => Some(72),
            _ => return Err("unexpected fixture limit".into()),
        };
        return Ok(Arc::new(BcryptHasher::new(BcryptConfig {
            rounds: 4,
            verify,
            limit,
        })?));
    }
    assert_eq!(options["memory"], 1024);
    assert_eq!(options["time"], 1);
    assert_eq!(options["threads"], 1);
    let config = ArgonConfig {
        memory: 1024,
        time: 1,
        threads: 1,
        verify,
    };
    match profile.algorithm.as_str() {
        "argon2i" => Ok(Arc::new(ArgonHasher::new(config)?)),
        "argon2id" => Ok(Arc::new(Argon2IdHasher::new(config)?)),
        _ => Err("unexpected fixture algorithm".into()),
    }
}

fn fixture_inputs(inputs: Vec<Input>) -> TestResult<BTreeMap<String, Vec<u8>>> {
    assert_eq!(inputs.len(), 29);
    let mut decoded = BTreeMap::new();
    for input in inputs {
        let bytes = STANDARD.decode(input.base64)?;
        assert_eq!(bytes.len(), input.bytes_len);
        assert!(decoded.insert(input.id, bytes).is_none());
    }
    Ok(decoded)
}

async fn compare_make(fixture: &Make, driver: &dyn Hasher, value: &[u8]) -> TestResult<usize> {
    assert_eq!(fixture.algorithm_capability, "available");
    assert_eq!(fixture.result["error_levels"], serde_json::json!([]));
    let options = HashOptions::new();
    let result = driver.make(value, &options).await;
    match fixture.result["status"].as_str() {
        Some("returned") => {
            let encoded = result?;
            assert_eq!(
                serde_json::to_value(driver.info(&encoded)?)?,
                fixture.result["actual_hash_info"]
            );
            // 这182次Rust生成值roundtrip另计，不冒充PHP的408次fixture校验。
            assert!(driver.check(value, Some(&encoded), &options).await?);
            Ok(0)
        }
        Some("exception") => {
            let expected = match fixture.result["exception_class"].as_str() {
                Some("RuntimeException") => "hashing_failed",
                Some("InvalidArgumentException") => "hashing_value_too_long",
                _ => return Err("unknown make exception".into()),
            };
            assert!(matches!(result, Err(error) if error.code() == expected));
            Ok(if expected == "hashing_failed" { 1 } else { 2 })
        }
        _ => Err("unknown make status".into()),
    }
}

// 每条来源只可为成功make、失败make未调用、或三个固定hash之一；不以缺字段假装None hash。
fn check_source(check: &Check, makes: &BTreeMap<(&str, &str), &Value>) -> TestResult<usize> {
    assert!(check.hash_is_public_synthetic_fixture);
    match (
        check.hash_profile.as_deref(),
        check.hash_case.as_deref(),
        check.hash_fixture.as_deref(),
    ) {
        (Some(profile), Some(case), None) => {
            assert!(matches!(
                check.group.as_str(),
                "self-check" | "byte-boundary-pair" | "cross-algorithm-verify-policy"
            ));
            let made = makes.get(&(profile, case)).ok_or("unknown source make")?;
            match made["status"].as_str() {
                Some("returned") => {
                    assert!(check.invoke_original_or_rust_checker);
                    assert_eq!(
                        check.encoded_hash.as_deref(),
                        Some(
                            made["public_fixture_hash"]
                                .as_str()
                                .ok_or("source make hash missing")?
                        )
                    );
                    Ok(0)
                }
                Some("exception") => {
                    assert!(!check.invoke_original_or_rust_checker);
                    assert_eq!(check.result["status"], "fixture-unavailable");
                    assert_eq!(check.result["reason"], "actual-make-did-not-return-hash");
                    assert!(check.encoded_hash.is_none());
                    Ok(1)
                }
                _ => Err("unknown source make status".into()),
            }
        }
        (None, None, Some(fixed)) => {
            assert_eq!(check.group, "null-empty-malformed-hash");
            assert!(check.invoke_original_or_rust_checker);
            let (expected, category) = match fixed {
                "null" => (None, 2),
                "empty" => (Some(""), 3),
                "malformed" => (Some("not-a-password-hash"), 4),
                _ => return Err("unknown fixed hash fixture".into()),
            };
            assert_eq!(check.encoded_hash.as_deref(), expected);
            Ok(category)
        }
        _ => Err("ambiguous or incomplete hash fixture source".into()),
    }
}

async fn compare_check(fixture: &Check, driver: &dyn Hasher, value: &[u8]) -> TestResult<usize> {
    assert!(fixture.hash_is_public_synthetic_fixture);
    assert_eq!(fixture.result["error_levels"], serde_json::json!([]));
    let options = HashOptions::new();
    let result = driver
        .check(value, fixture.encoded_hash.as_deref(), &options)
        .await;
    match fixture.result["status"].as_str() {
        Some("returned") => {
            let expected = fixture.result["value"]
                .as_bool()
                .ok_or("non-boolean fixture")?;
            assert_eq!(result?, expected);
            Ok(usize::from(!expected))
        }
        Some("exception") => {
            assert_eq!(fixture.result["exception_class"], "RuntimeException");
            assert!(matches!(result, Err(error) if error.code() == "hashing_algorithm_mismatch"));
            Ok(2)
        }
        _ => Err("unknown check status".into()),
    }
}

#[tokio::test]
async fn actual_php85_binary_fixture_matches_232_make_and_408_called_checks() -> TestResult {
    // Root companion提供此固定公共合成fixture；56个未调用槽位不能记为false或通过。
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../tests/fixtures/hashing_bytes_php85.json"
    ))?;
    assert_eq!(fixture.profiles.len(), 8);
    assert_eq!(fixture.make.len(), 232);
    assert_eq!(fixture.check.len(), 464);
    let inputs = fixture_inputs(fixture.inputs)?;
    let drivers = fixture
        .profiles
        .iter()
        .map(|(name, profile)| Ok((name.clone(), fixture_driver(profile)?)))
        .collect::<std::result::Result<BTreeMap<_, _>, Box<dyn std::error::Error>>>()?;
    let mut makes = BTreeMap::new();
    let mut make_counts = [0; 3];
    for make in &fixture.make {
        let category = compare_make(
            make,
            drivers
                .get(&make.profile)
                .ok_or("unknown make profile")?
                .as_ref(),
            inputs.get(&make.case).ok_or("unknown make input")?,
        )
        .await?;
        make_counts[category] += 1;
        assert!(
            makes
                .insert((make.profile.as_str(), make.case.as_str()), &make.result)
                .is_none()
        );
    }
    assert_eq!(make_counts, [182, 26, 24]);
    let mut counts = [0; 3];
    let mut sources = [0; 5];
    for (slot, check) in fixture.check.iter().enumerate() {
        assert_eq!(check.slot, slot);
        assert!(drivers.contains_key(&check.checker_profile));
        assert!(inputs.contains_key(&check.candidate_case));
        let source = check_source(check, &makes)?;
        sources[source] += 1;
        if source == 1 {
            continue;
        }
        let category = compare_check(
            check,
            drivers
                .get(&check.checker_profile)
                .ok_or("unknown checker")?
                .as_ref(),
            inputs
                .get(&check.candidate_case)
                .ok_or("unknown check input")?,
        )
        .await?;
        counts[category] += 1;
    }
    assert_eq!(counts, [248, 136, 24]);
    assert_eq!(counts.iter().sum::<usize>(), 408);
    assert_eq!(sources, [312, 56, 32, 32, 32]);
    Ok(())
}
