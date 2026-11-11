# 01-02u Singleton 与 Scoped 声明

对应 [Laravel 13.x 容器生命周期](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#binding-basics)、[共享属性判断](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L275-L335)、[绑定属性装配](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L1000-L1028)和[具体类及接口生命周期测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php#L838-L891)。参考固定版 [Goravel 应用入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)，声明继续由应用 Provider 装配。

- 旧版 `6d1a8192` 的 `container/class_descriptor.rs` 与 `container/class_binding.rs` 提供生命周期声明、Singleton 优先级、类工厂与作用域复用用例。本批沿用当前目录及共享缓存，保留隐式具体类在缓存前未绑定、缓存后可见的规则。
- `withLifetime` 声明具体类生命周期，`registerLifetime` 声明服务键生命周期。支持 `Bind` 和 `BindWhen` 选出的实现及已有显式工厂。Rust 用同步和异步工厂共用一个缓存槽，直接构建和显式参数覆盖使用独立实例。
- Scoped 使用已有请求、命令及根作用域。清除实例后可再次解析。自动类的内部共享工厂省略于 `getBindings`，显式登记工厂显示实际共享设置。类属性回调在发布前执行，作用域缓存重置保留属性元数据。

## 验收

基线 `63d0f0ab` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。294 项测试包含 5 项生命周期属性专项测试，覆盖失败重试、并发共享、参数覆盖、清理和自构造重入。
- 8 次真实 HTTP 请求覆盖请求内共用 Scoped、跨请求重建、异步依赖失败及重试和应用隔离。16 个 Artisan 入口探针通过，包含 `environment:probe` 的条件绑定单例身份断言。`./artisan inspire` 正常退出。
- 三语示例独立编译执行。文档构建及根路径和子路径的 12 个浏览器组合通过，覆盖容器页导航与新小节正文。

下一部分核对编译接口投影，依赖现有类描述符、接口绑定与本批生命周期。内置上下文属性按各组件依赖接入。
