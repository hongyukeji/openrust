# 测试入门

## 运行测试

集成测试放在应用的 `tests/` 或对应 crate 下。使用已提交的锁文件运行工作区测试。

```sh
cargo +1.94.0 test --workspace --locked
```

## 创建测试应用

用 `TestApp` 创建具有独立临时根目录和服务容器的应用。启动前可在 `test.root()` 下准备测试文件。回调接收常规应用 Builder，可以注册本次测试所需的 Provider、路由和配置。

```rust
use openrust::{contracts::foundation::Application as _, testing::TestApp};

#[tokio::test]
async fn boots_a_test_application() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| builder)?;
    std::fs::write(test.root().join("fixture.txt"), "test data")?;
    test.boot().await?;
    assert_eq!(test.app().config()?.get("app.env"), Some("testing".into()));
    Ok(())
}
```

`boot` 执行应用引导，无需绑定端口。默认配置使用 `testing` 环境，自定义配置加载器提供自己的设置。夹具离开作用域时清理其临时目录。测试辅助工具不修改进程环境和工作目录。
