# 主控台測試

## 執行命令

使用 `TestApp::artisan` 執行 Artisan，傳入 `artisan` 程式名稱之後的引數。它使用一般主控台核心，回傳結束代碼、標準輸出與標準錯誤。

```rust
use openrust::testing::TestApp;

#[tokio::test]
async fn reports_command_success_and_failure() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| builder)?;
    let help = test.artisan(["help", "env"]).await;
    help.assertSuccessful();
    assert!(help.stdout.contains("env"));
    assert!(help.stderr.is_empty());

    let missing = test.artisan(["example:missing"]).await;
    missing.assertExitCode(1);
    assert!(missing.stdout.is_empty());
    assert!(missing.stderr.contains("example:missing"));
    Ok(())
}
```

## 檢查命令輸出

使用 `assertSuccessful` 驗證結束代碼 0，使用 `assertExitCode` 驗證指定代碼，使用 `expectsOutput` 檢查標準輸出中的完整一列。需要檢查命令直接寫入程序的輸出時，透過子程序執行。
