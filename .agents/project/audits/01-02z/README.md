# 01-02z 公开属性后置回调派发

对应 [Laravel 13.x Contextual Attributes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#contextual-attributes)、[Container::fireAfterResolvingAttributeCallbacks](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L1594-L1642)、[BoundMethod](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/BoundMethod.php#L203-L207)和[路由参数解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/ResolvesRouteDependencies.php#L51-L63)。核对 [Laravel 属性 after 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContextualAttributeBindingTest.php#L123-L130)。应用装配参考固定 [Goravel Boot/Start 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)。

- 从旧版 `6d1a8192` 的 `container/descriptor_resolve.rs` 和 `container/attribute_resolution_tests.rs` 提取实际对象匹配和同对象接口投影回归。保留属性内置 `after` 先执行、已注册回调分组顺序、回调实例重新构造和失败即时返回。
- `Container::fireAfterResolvingAttributeCallbacks` 接受编译属性、已解析值及可选参数元数据。类构造和参数解析使用同一入口。构造参数显式覆盖仍跳过其回调。Laravel 的动态 `__get`、`__set` 代理到 ArrayAccess，Rust 由已有的 `offsetGet`、`offsetSet` 表达。
- `ParameterValue::service` 保留服务类型，匹配实际类型及编译登记的 trait 投影。类型化类属性回调按对象匹配，包括从命名绑定构造的对象。显式提供具体服务的上下文属性可投影到构造函数或方法所需的 trait。通过 `AttributeEvent::service` 读取同一个 Arc 分配。
- Rust 使用类型元数据和静态声明替代 PHP 反射及 `instanceof`。若调用者已经只传入 trait 对象，回调仅能取得该对象携带的类型及可用投影。旧式类参数继续支持 `ParameterValue::new(Arc<T>)`，按通过类型检查的声明类型观察。路由动作自动依赖解析在路由/控制器批次使用公开入口接入。

## 验收

在 `21a65e98` 加上已验收但未暂存的 01-02y 与本批代码的隔离副本执行，macOS arm64，Rust 1.94.0。工作区其它改动保持原样。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。309 项测试含两项专项，验证失败截断、重试、容器隔离及构造/方法调用中的对象身份。
- 构造生命周期集成套件的 8 次真实 HTTP 请求通过，作用域服务回调在每个新实例上执行一次，并验证 trait 视图身份。16 个 Artisan 入口探针通过，其中 `construction:probe` 验证公开派发入口和投影对象身份。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语示例由 Rust 1.94.0 编译并执行。`npm run build` 成功，含打包体积提示。根路径与子路径共 12 个浏览器组合验证导航及本节内容。

下一批接 01-03 日志默认渠道、默认栈与落盘，依赖已验收的 01-01 容器、Provider 和配置装配。服务型属性及路由动作解析继续按其依赖推进。
