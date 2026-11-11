# 01-02x 基础内置上下文属性

对照 [Laravel 13.x 上下文属性](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#contextual-attributes)、[Config、Give、Tag 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Attributes)、[ContextualAttributeBindingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContextualAttributeBindingTest.php)及 [Container::currentlyResolving](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L1660-L1663)。应用装配参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)。

- 从旧版 `6d1a8192` 的 `container/attributes/builtin.rs`、`builtin_tests.rs` 和 `container/contextual.rs` 提取配置空值与缺失区分、显式解析参数和标签惰性迭代回归。
- `Config` 按 Rust 目标类型反序列化，键缺失才使用默认值。类型错误包含配置键和目标类型，隐藏配置值及反序列化错误详情。
- `Give` 通过容器解析指定服务，可登记独立参数和输出投影。异步入口可等待异步工厂和延迟 Provider。
- `Tag` 延迟到迭代时解析，保留登记顺序和重复项，可重新遍历。`TaggedServices` 校验容器身份，迭代时借用原容器并使用活动作用域。
- `currentlyResolving()` 读取当前解析键，`CurrentResolving` 将其注入为 `Option<ServiceKey>`。Laravel 测试用自定义属性访问此方法，Rust 额外提供内置属性。

## 验收

基线 `2876f604` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- 格式检查、离线锁定工作区测试和全目标严格 Clippy 通过。305 项测试含 4 项内置属性专项以及现有统一 JSON 响应回归。
- 8 次真实 HTTP 请求覆盖配置注入、异步服务解析、惰性标签、延迟 Provider 失败重试和应用隔离。16 个 Artisan 入口探针通过，包含 `construction:probe`。实际执行 `./artisan inspire` 正常退出。
- 三语示例分别编译执行。文档构建通过，根路径和子路径共 12 个浏览器组合通过。

下一批核对 `RequestAttribute` 与 `RouteParameter` 的请求和匹配路由作用域，再按依赖接入其余内置属性。01-02 尚有公开接口待核对。
