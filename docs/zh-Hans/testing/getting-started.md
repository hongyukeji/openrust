# 测试入门

## 运行测试 {#running-tests}

集成测试放在应用的 `tests/` 或对应 crate 下。使用已提交的锁文件运行工作区测试。

通过 Artisan 运行应用测试，或通过 Cargo 运行所有工作区成员的测试。使用 Artisan 时，可在测试过滤名之前传入 Cargo 选项，并在过滤名之后用 `--` 传入 Rust 测试运行器选项。遇到与 Artisan 同名的 Cargo 选项时，在 `test` 后立即加一个 `--`，例如透传 Cargo 的 `--help`。

```sh
./artisan test --locked --test feature artisan_inspire_prints_an_attributed_quote -- --exact
cargo +1.94.0 test --workspace --locked
```

Artisan 从应用根目录运行 Cargo，实时输出结果并返回 Cargo 的退出状态。优先使用环境中的 `CARGO` 可执行文件，未设置时从可执行文件搜索路径调用 `cargo`。

## 并行测试 {#parallel-tests}

```sh
./artisan test --parallel --processes=4 --locked --test feature
./artisan test --parallel --processes=2 --test feature health -- --exact
./artisan test --parallel --test feature -- --list
```

Cargo 统一构建测试产物，原生运行器列举匹配的 libtest 用例，每个用例在独立子进程执行，`--processes` 限制同时运行的子进程数。每个子进程取得独立字符串 `TEST_TOKEN`。用例失败时命令返回 1，Ctrl+C 返回 130；列举不会执行用例。支持过滤名、`--exact`、`--skip name`、`--ignored`、`--include-ignored`、`--nocapture` 和 `--show-output`，不支持的运行器选项明确报错。普通 `artisan test` 继续透传 Cargo。

在测试 Provider 的 boot 方法内注册钩子，此时已进入所属应用的门面上下文：

```rust
use openrust::{
    contracts::foundation::Application as _,
    support::facades::ParallelTesting,
    testing::ParallelTestError,
};

pub fn register_parallel_hooks() -> Result<(), ParallelTestError> {
    ParallelTesting::set_up_process(|app, token| Box::pin(async move {
        app.config()?.set("testing.token", token)?;
        Ok(())
    }))
}
```

`set_up_test_case`／`tear_down_test_case` 回调接收真实可变 `TestApp`；进程与数据库钩子接收真实应用和 token。`set_up_test_database_before_migrating`、`set_up_test_database` 分别在受管 MySQL 夹具首次迁移准备前后执行；重复重置不重复触发。回调或迁移失败后，重试不会把已有数据库重新当作新建。

生成的功能测试使用 `TestApp::run_parallel_test`。已有功能测试可用它包装测试正文，以执行用例钩子；原生并行子进程还会自动执行进程钩子。正文返回错误或 panic 后仍执行清理。自定义运行器可用 `ParallelTesting::run_process`，也可显式调用六个 `call_*_callbacks` 入口；不要为原生子进程再包一层进程生命周期。

`option`、`token` 保留 false 与字符串的区别。`resolve_options_using(Some(Arc::new(...)))`、`resolve_token_using` 设置所属应用的解析回调，传 None 恢复创建服务时的进程环境快照；解析错误向上传播。原生回调接收具类型应用／夹具参数，其他依赖通过应用解析。回调在注册锁外执行，可重入注册。

当前适配为每个用例独立进程，并非跨用例常驻工作进程；现有受管 MySQL 夹具仍各自创建和清理数据库。可复用数据库命名、重建／删除命令选项、自动缓存前缀与编译视图隔离、取消时的异步钩子／数据库回收及 Linux／Windows 原生进程验收仍单列待补，不能凭生命周期入口认定这些能力已提供。

## 测试环境 {#test-environment}

Artisan 给 Cargo 子进程设置 `APP_ENV=testing`。应用用该环境启动时优先读取 `.env.testing`，缺失时读取 `.env`。进程环境与所选文件均未定义 `CACHE_STORE` 或 `SESSION_DRIVER` 时，引导器分别补上 `array`。数据库测试配置应放在测试环境里。修改环境配置前，可用 `./artisan config:clear` 清除应用的配置缓存。

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

在首次 `boot`、HTTP 请求或 Artisan 命令前，把 `.env.testing` 写入 `test.root()`。配置加载器可以从环境参数中读取当前夹具的值，收到的 `APP_ENV` 为 `testing`。夹具忽略从进程继承的 `APP_CONFIG_CACHE`、`APP_PACKAGES_CACHE`、`APP_ROUTES_CACHE` 和 `APP_EVENTS_CACHE` 路径。需要为夹具指定自定义路由缓存时，在它的 `.env.testing` 中设置 `APP_ROUTES_CACHE`。

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
