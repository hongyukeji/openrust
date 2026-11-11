# 01-02w 接口解析回调与内部事件

对应 [Laravel 13.x 容器事件](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#container-events)、[Container::getClosure](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L404-L415)及 [ResolvingCallbackTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ResolvingCallbackTest.php)。应用仍由 Provider 装配，参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)。

- 复用旧版 `6d1a8192` 的 `container/resolving_hooks.rs` 与 `container.rs` 中的阶段匹配、对象视图和每次解析的事件开关。解析前按规范键和类声明匹配，解析中和解析后按实际对象与已登记的 trait 视图匹配。全局回调先执行，再读取类型回调。
- 类绑定内部解析具体实现时只抑制该次解析的事件。构造依赖、类属性回调和扩展器照常执行，具体类型的缓存与解析记录仍更新。外层事件使用外层服务的规范键，并保留同对象的具体类型和 trait 视图。
- Rust 用编译描述符匹配接口，内部工厂携带类型化对象视图。显式转换或扩展器返回不同分配时，旧具体类型视图失效。仅有 trait 类型信息的替换值可按该接口匹配。普通异步工厂仍直接等待原有 future。

## 验收

基线 `7de7877c` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- 格式检查、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。301 项测试含 3 项事件专项，覆盖事件次序、接口与具体类匹配、依赖失败重试、扩展器替换和 Arc 身份。
- 环境测试通过 11 次真实 HTTP 请求，覆盖失败后重试、应用隔离和外层回调的规范键及对象身份。16 个 Artisan 入口探针通过，`construction:probe` 检查接口回调值与解析结果为同一对象。`./artisan inspire` 正常退出。
- 三语示例分别编译执行。文档构建通过，根路径和子路径共 12 个浏览器组合覆盖容器页导航及新小节正文。

下一部分接基础内置上下文属性，优先核对 `Config`、`Give`、`Tag` 和 `CurrentResolving`，依赖已验收的属性派发、配置、标签和投影。
