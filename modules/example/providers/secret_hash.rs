//! 从本地文件读取集成秘密，复用应用统一哈希配置；命令只输出已编码哈希。

use std::path::Path;

use openrust::console::{Command, print};
use openrust::foundation::Application;
use openrust::hashing::{HashManager, HashOptions};
use openrust::support::BoxFuture;
use openrust::{Error, Result};
use tokio::io::AsyncReadExt;

const MAX_SECRET_BYTES: u32 = 1_048_576;

pub(super) struct HashSecretFile;

impl Command for HashSecretFile {
    fn name(&self) -> &'static str {
        "example secret hash"
    }

    fn about(&self) -> &'static str {
        "按应用统一哈希配置处理秘密文件，只输出已编码哈希"
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let [path] = args.as_slice() else {
                return Err(Error::custom(
                    "example_secret_file_argument",
                    "请提供一个秘密文件路径",
                ));
            };
            let hashing = app.state().resolve::<HashManager>().await?;
            let encoded = digest_file(&hashing, Path::new(path)).await?;
            print(&encoded);
            Ok(())
        })
    }
}

async fn digest_file(hashing: &HashManager, path: &Path) -> Result<String> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| Error::custom("example_secret_file_read_failed", "无法读取秘密文件"))?;
    // 示例业务限制文件尺寸，不改变通用Hasher的输入域或配置。
    let mut bytes = Vec::new();
    file.take(u64::from(MAX_SECRET_BYTES) + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Error::custom("example_secret_file_read_failed", "无法读取秘密文件"))?;
    if u64::try_from(bytes.len()).map_err(Error::internal)? > u64::from(MAX_SECRET_BYTES) {
        return Err(Error::custom(
            "example_secret_file_too_large",
            "秘密文件不能超过1MiB",
        ));
    }
    // 集成秘密要求完整输入身份；这是示例业务策略，不改变通用make/check。
    // 默认链只选择一次最终叶；能力回调更新嵌套配置只影响下一次命令。
    let driver = hashing.driver_for_operation()?;
    if !driver.accepts_exact_input(bytes.as_slice())? {
        return Err(Error::custom(
            "example_secret_input_ambiguous",
            "所选驱动不能保留完整秘密字节，请显式选择完整字节算法",
        ));
    }
    driver.make(bytes.as_slice(), &HashOptions::new()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrust::hashing::{ArgonConfig, HashingConfig};

    #[tokio::test]
    async fn file_bytes_reach_shared_hasher_without_text_decoding() -> Result<()> {
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join("public-byte-fixture");
        let original = [0xff, 0, b'a', 0xfe];
        tokio::fs::write(&path, original)
            .await
            .map_err(Error::internal)?;
        let hashing = HashManager::new(HashingConfig {
            driver: "argon2id".into(),
            argon: ArgonConfig {
                memory: 1024,
                time: 1,
                threads: 1,
                verify: true,
            },
            ..Default::default()
        })?;
        let encoded = digest_file(&hashing, &path).await?;
        assert!(
            hashing
                .check(original, Some(&encoded), &HashOptions::new())
                .await?
        );
        assert!(
            !hashing
                .check([0xff, b'a', 0xfe], Some(&encoded), &HashOptions::new())
                .await?
        );
        assert!(
            !hashing
                .check(
                    [0xef, 0xbf, 0xbd, 0, b'a', 0xef, 0xbf, 0xbd],
                    Some(&encoded),
                    &HashOptions::new()
                )
                .await?
        );
        let bcrypt = HashManager::new(HashingConfig::default())?;
        assert!(
            matches!(digest_file(&bcrypt, &path).await, Err(error) if error.code() == "example_secret_input_ambiguous")
        );
        tokio::fs::write(&path, vec![b'x'; 73])
            .await
            .map_err(Error::internal)?;
        assert!(
            matches!(digest_file(&bcrypt, &path).await, Err(error) if error.code() == "example_secret_input_ambiguous")
        );
        tokio::fs::write(&path, vec![b'x'; 1_048_577])
            .await
            .map_err(Error::internal)?;
        assert!(
            matches!(digest_file(&hashing, &path).await, Err(error) if error.code() == "example_secret_file_too_large")
        );
        assert!(
            matches!(digest_file(&hashing, &directory.path().join("missing")).await, Err(error) if error.code() == "example_secret_file_read_failed")
        );
        Ok(())
    }
}

#[cfg(test)]
mod live_driver_tests {
    use super::*;
    use openrust::config::repository::ConfigRepository;
    use openrust::hashing::{Argon2IdHasher, ArgonConfig, HashInfo, Hasher};
    use std::sync::Arc;

    struct SwitchingPolicy {
        repository: ConfigRepository,
        inner: Argon2IdHasher,
    }
    impl Hasher for SwitchingPolicy {
        fn accepts_exact_input(&self, value: &[u8]) -> Result<bool> {
            let accepted = self.inner.accepts_exact_input(value)?;
            // 同步改变live默认即可确定性击中旧实现的两次driver解析，无定时race。
            self.repository
                .set("hashing.driver", serde_json::json!("bcrypt"))?;
            Ok(accepted)
        }
        fn info(&self, encoded: &str) -> Result<HashInfo> {
            self.inner.info(encoded)
        }
        fn make<'a>(
            &'a self,
            value: &'a [u8],
            options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<String>> {
            self.inner.make(value, options)
        }
        fn check<'a>(
            &'a self,
            value: &'a [u8],
            encoded: Option<&'a str>,
            options: &'a HashOptions,
        ) -> BoxFuture<'a, Result<bool>> {
            self.inner.check(value, encoded, options)
        }
        fn needs_rehash(&self, encoded: &str, options: &HashOptions) -> Result<bool> {
            self.inner.needs_rehash(encoded, options)
        }
    }

    #[tokio::test]
    async fn exact_input_check_and_make_pin_one_live_driver() -> Result<()> {
        let repository = ConfigRepository::new(figment::Figment::from(
            figment::providers::Serialized::defaults(
                serde_json::json!({"hashing":{"driver":"switch-on-check","bcrypt":{"rounds":4,"verify":true}}}),
            ),
        ));
        let hashing = HashManager::from_repository(repository.clone(), 2)?;
        let selected = Arc::new(SwitchingPolicy {
            repository: repository.clone(),
            inner: Argon2IdHasher::new(ArgonConfig {
                memory: 1024,
                time: 1,
                threads: 1,
                verify: true,
            })?,
        });
        let captured = Arc::clone(&selected);
        hashing.extend("switch-on-check", move || {
            Ok(Arc::clone(&captured) as Arc<dyn Hasher>)
        })?;
        let directory = tempfile::tempdir().map_err(Error::internal)?;
        let path = directory.path().join("public-long-secret");
        let input = [b'a'; 73];
        tokio::fs::write(&path, input)
            .await
            .map_err(Error::internal)?;
        let encoded = digest_file(&hashing, &path).await?;
        assert_eq!(
            repository.get("hashing.driver")?,
            Some(serde_json::json!("bcrypt"))
        );
        assert!(encoded.starts_with("$argon2id$"));
        assert!(
            selected
                .inner
                .check(&input, Some(&encoded), &HashOptions::new())
                .await?
        );
        assert!(
            !selected
                .inner
                .check(&input[..72], Some(&encoded), &HashOptions::new())
                .await?
        );
        Ok(())
    }
}
