# 01-02v 编译接口投影

对应 [Laravel 13.x 接口绑定](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#binding-interfaces-to-implementations)及 [Container::getClosure](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L404-L415)。Laravel 依赖 PHP 对象的接口类型关系。Rust 通过显式编译元数据登记对象的 trait 视图。应用仍由 Provider 装配，参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)。

- 复用旧版 `6d1a8192` 的 `container/class_projection.rs`、`class_descriptor_tests.rs` 与 `descriptor_resolve_tests.rs` 中编译器检查的 Arc 转换、同对象验证、非法元数据及命名解析回归。
- `withProjection(class_projection!(Concrete => dyn Contract))` 在类描述符中登记视图。容器先选择服务来源，再按实际绑定输出类型应用视图。别名、已有显式绑定及作用域仍按原有入口选择。缺少兼容视图时在构造服务前失败。
- `Bind::implementation` 可复用描述符的投影来绑定接口，支持 `BindWhen`。投影不新增服务实例或缓存，检查底层 Arc 地址。内部类型擦除仍持有 Rust 类型信息。
- 解析回调和扩展器继续使用被选服务的输出契约。PHP 的接口实例匹配扩展至回调选择，以及固定版绑定闭包内部的 `raiseEvents: false`，仍属于容器剩余契约核对项。

## 验收

基线 `9bab1f7f` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。298 项测试包含 4 项投影专项测试，覆盖同一 Arc、并发单例、显式绑定替换、构造注入、无效投影及缺少描述符后的重试。
- 8 次真实 HTTP 请求通过，Worker 路由验证别名投影与原 Scoped 实例是同一对象。16 个 Artisan 入口探针通过，`construction:probe` 验证通过命名别名取得自构造服务的 trait 视图。`./artisan inspire` 正常退出。
- 三语示例分别编译运行。文档构建通过，根路径和子路径共 12 个浏览器组合覆盖容器页导航与新小节正文。

下一部分核对容器事件的接口匹配与绑定内部事件传播，依赖本批投影元数据和现有解析回调。内置上下文属性按各组件依赖接入。
