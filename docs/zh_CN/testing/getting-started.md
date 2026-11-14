# 测试入门

## 运行测试 {#running-tests}

集成测试放在应用的 `tests/` 或对应 crate 下。使用已提交的锁文件运行工作区测试。

通过 Artisan 运行应用测试，或通过 Cargo 运行所有工作区成员的测试。使用 Artisan 时，可在测试过滤名之前传入 Cargo 选项，并在过滤名之后用 `--` 传入 Rust 测试运行器选项。遇到与 Artisan 同名的 Cargo 选项时，在 `test` 后立即加一个 `--`，例如透传 Cargo 的 `--help`。

```sh
./artisan test --locked --test feature artisan_inspire_prints_an_attributed_quote -- --exact
cargo +1.94.0 test --workspace --locked
```

Artisan 从应用根目录运行 Cargo，实时输出结果并返回 Cargo 的退出状态。优先使用环境中的 `CARGO` 可执行文件，未设置时从可执行文件搜索路径调用 `cargo`。

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

## 创建测试 {#creating-tests}

Artisan 把功能测试写入 `tests/feature/`，把单元测试写入 `tests/unit/`。生成的功能测试引导临时应用并检查健康路由，单元测试使用 Rust 标准测试入口。

```sh
./artisan make:test HealthTest
./artisan make:test GreetingTest --unit
```

在 `tests/feature/main.rs` 声明 `mod health_test;`，在 `tests/unit/main.rs` 声明 `mod greeting_test;`，让 Cargo 发现测试文件。嵌套名称也需要声明各级模块。然后运行指定测试。

```sh
cargo +1.94.0 test --locked --test feature test_health_test
cargo +1.94.0 test --locked --test unit test_greeting_test
```

使用 `-f` 或 `--force` 覆盖已有测试文件。在应用根目录添加 `stubs/test.rs.stub` 或 `stubs/test.unit.rs.stub` 可自定义模板。生成器把 `{{test}}` 替换为 Rust 测试函数名。
