# 控制台测试

## 运行命令

用 `TestApp::artisan` 运行 Artisan，传入 `artisan` 程序名之后的参数。它走正常的控制台内核，返回退出码、标准输出和标准错误。

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

## 检查命令输出

使用 `assertSuccessful` 验证退出码 0，使用 `assertExitCode` 验证指定退出码，使用 `expectsOutput` 检查标准输出中的完整一行。需要检查命令直接写入进程的输出时，通过子进程运行。
