# 01-02s 上下文属性与解析回调

对应 [Laravel 13.x 上下文属性](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#contextual-attributes)、[构造参数解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L1243-L1282)、[属性回调](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L1601-L1639)及 [BoundMethod](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/BoundMethod.php)。对照固定版 [ContextualAttributeBindingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContextualAttributeBindingTest.php) 和 [AfterResolvingAttributeCallbackTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/AfterResolvingAttributeCallbackTest.php)。应用继续通过 Provider 装配，与固定版 [Goravel 启动入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)职责相符。

- 核对旧版 `6d1a8192` 的 `vendor/openrust/framework/src/container/attributes.rs` 与 `attribute_resolution_tests.rs`。复用属性工厂、注册解析器优先、构造空值回退、整组变参与回调次序，沿用当前构造和方法调用的参数表示。
- 提供 `ContextualAttribute`、`whenHasAttribute`、`resolveFromAttribute`、`afterResolvingAttribute`，接到构造函数、方法、闭包及 `SelfBuilding::newInstance`。显式构造参数跳过属性，方法显式命名参数继续触发回调。方法按命名值、属性、类名值的顺序选择，属性空值保留。
- 编译描述符替代 PHP 反射。属性通过类型身份匹配，服务回调使用声明的 Rust 服务类型。同步回调也用于异步构建，用户代码在注册表锁外执行。属性实例在各次解析和回调中独立创建。构造后类回调先于扩展器和常规服务解析回调。
- 框架内置的上下文属性、类绑定与生命周期属性和接口投影另按依赖接入。本批验收自定义属性和标记回调的通用机制。

## 验收

基线 `0b315181` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 均通过。284 项测试包含 4 项属性专项测试，覆盖参数优先级、空值、变参、缺少处理器后重试、容器隔离和回调顺序。
- 8 次真实 HTTP 请求覆盖属性注入、延迟 Provider 失败后重试及并行应用隔离。16 个 Artisan 子进程探针通过，其中 `construction:probe` 和 `method:probe` 断言属性注入结果。实际 `./artisan inspire` 正常退出。
- 三语示例分别编译并执行。文档 `npm run build` 通过，浏览器在根路径与子路径验证导航和容器页正文，共 12 个组合。

下一批接入类的 `Singleton`、`Scoped`、`Bind` 与 `BindWhen` 元数据，依赖现有作用域、环境解析器与本批属性声明。需要组件服务的内置属性随各自依赖验收。
