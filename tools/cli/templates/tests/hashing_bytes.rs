//! 生成应用的统一Hashing保留原始字节；测试显式低成本不改变部署默认。

use openrust::{
    Result,
    hashing::{ArgonConfig, HashManager, HashOptions, HashingConfig},
};

#[tokio::test]
async fn generated_hashing_preserves_binary_input_and_text_callers() -> Result<()> {
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
    let options = HashOptions::new();
    let input = [0xff, 0, b'a', 0xfe];
    let encoded = hashing.make(input, &options).await?;
    assert!(
        hashing
            .check(input.to_vec(), Some(&encoded), &options)
            .await?
    );
    assert!(
        !hashing
            .check([0xff, b'a', 0xfe], Some(&encoded), &options)
            .await?
    );
    assert!(!hashing.check("�\0a�", Some(&encoded), &options).await?);
    let text = hashing.make("正常文本", &options).await?;
    let driver = hashing.driver(None)?;
    assert!(
        driver
            .check("正常文本".as_bytes(), Some(&text), &options)
            .await?
    );
    assert!(driver.accepts_exact_input(&input)?);
    Ok(())
}
