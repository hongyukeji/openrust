# 01-02t Bind 与 BindWhen

对应 [Laravel 容器接口绑定](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#binding-interfaces-to-implementations)、[属性选择源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L977-L1066)和 [ContainerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php#L892-L1067)。注册仍由应用服务提供者负责，参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)。

- 旧版 `6d1a8192` 的 `container/attributes/binding.rs` 和 `container/class_binding.rs` 提供声明顺序、通配回退、条件失败重试和同键条件写入语义。本批复用上述规则，选定的实现通过已有绑定目录登记，同步及异步解析走对应工厂入口。
- Rust 以 `registerBindingAttributes` 登记编译元数据。接口与实现通过显式类型转换关联，`BindWhen` 条件为同步 Rust 函数。环境回调和条件在注册表锁外执行。已有显式绑定优先，命中的条件可覆盖它自己登记的临时工厂，独立并发的显式工厂保留优先级。
- 无环境解析器时保留重试机会。条件未命中可在后续解析重新检查。成功选择继续使用到绑定删除或 `flush`。`flush` 清理属性选择缓存，保留声明与环境解析器。
- 本批接入瞬态绑定属性。`Singleton`、`Scoped` 与属性绑定联动以及自动接口投影仍需后续验收。

## 验收

基线 `21ba7bf9` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- 格式检查、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。289 项测试包含 5 项绑定属性专项测试，覆盖同步、异步、回退、条件重试、并发显式覆盖和工厂快照。
- 环境用例通过 11 次真实 HTTP 请求，新增请求覆盖环境解析失败后的重试及同一进程的应用隔离。16 个 Artisan 入口子进程通过，包含 `environment:probe` 的 `BindWhen` 断言。`./artisan inspire` 正常退出。
- 三语示例独立编译运行。文档构建通过，根路径与子路径共 12 项浏览器检查覆盖容器页导航和新小节正文。

下一批接 `Singleton`、`Scoped` 声明及二者与绑定属性的组合。前置的属性绑定工厂、同步和异步解析路径及请求作用域均已验收。
