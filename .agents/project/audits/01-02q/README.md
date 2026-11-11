# 01-02q 容器实例和键访问

对应 [Laravel 13.x 容器源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L1679-L1755) 与 [ArrayAccess 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php#L286-L407)。Laravel 在 [Application 基础绑定](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php#L267-L282)中安装全局实例。参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)和[容器契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/foundation/application.go#L122-L136)，应用仍持有各自的服务注册表。

- `getInstance` 复用现有应用任务作用域。作用域外惰性创建进程默认实例，`setInstance` 只更新该默认值。PHP 在构造应用时设置全局实例，Rust 的应用作用域避免同进程并行应用相互覆盖。旧实例在锁外释放。
- 键访问复用既有类型化解析、工厂和共享实例。Rust 用具名方法返回 `Result<Arc<T>>`，并提供异步工厂和读取接口。值赋值采用瞬态工厂持有同一值。删除按原始键处理，同时清理活动作用域的缓存，保留别名和作用域声明。
- 活动作用域的缓存选择与键删除共用注册表锁。回调前检查缓存是否仍有效，写入解析记录时在锁内复核绑定版本，保留 `forgetInstance` 清缓存时的成功记录。删除前已经开始的调用可以完成自身解析。
- 旧版 `6d1a8192` 的 `vendor/openrust/framework/src/container/directory.rs` 和 `directory_tests.rs` 提供按原始键清实例及别名回归。`forget_instance_key` 保留定义和历史，`offsetUnset` 需要同时移除二者，因此复用行为用例及当前缓存退休机制。

## 验收

基线 `a55d2ebb` 加本批变更的隔离副本，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。275 项测试与 16 个 Artisan 入口子进程探针通过，含 `directory:probe` 的应用实例和键读写。另实跑 `RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire`。
- 通过真实 TCP 共发送 11 个 HTTP 请求。新增的 5 个请求验证两个应用并行解析各自的实例、一个应用删除后的 404 和另一个应用继续解析。新测试覆盖进程默认实例的并发初始化和析构重入、值和工厂、悬空别名、活动作用域清理、回调期间和异步解析期间删除。
- 三语文档的新增 Rust 示例分别编译并运行。`npm run build`、`node tests/check_browser.mjs` 和加入容器页链接与正文检查的浏览器脚本各通过 12 个组合。

01-02 仍需验收 SelfBuilding、类及参数属性和接口投影。先对照固定版自动注入源码，复用现有类描述符及工厂解析，环境属性可使用 01-02p 的默认环境解析器。
