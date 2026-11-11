# 01-02r SelfBuilding 与类构建

对应 [Laravel 容器自动解析](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md#zero-configuration-resolution)、[SelfBuilding 契约](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Container/SelfBuilding.php)及 [build 和 newInstance 分支](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php#L1050-L1145)。固定版 [ContainerTest::testWithFactoryHasDependency](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php#L1002-L1013)验证方法依赖注入。参考固定 [Goravel 启动入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go)，类元数据由应用服务提供者登记。

- 从旧版 `6d1a8192` 的 `container/self_building.rs`、`container/class_descriptor.rs` 和 `container/attribute_resolution_tests.rs` 复用独立方法签名、同类重入降级、外层共享缓存及方法绑定的回归场景。当前实现用已有 `Container::call` 和 `bindStaticMethod` 调用并替换 `newInstance`。
- Rust 用 trait 要求实现 `newInstance`，用类描述符登记普通构造和方法参数。首次构建经异步方法注入，构造参数覆盖不作为方法实参。显式绑定仍优先于自动构建。自构造中重入同一类使用普通构造，并避开同一条正在执行的共享工厂。普通构造仍检查循环依赖。
- `makeAsync` 和 `buildAsync` 是自构造入口，同步首次构建报告需要异步解析。Laravel 的类属性解析回调留待属性单元统一实现。

## 验收

基线 `be478bf9` 加本批文件的隔离副本，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 均通过。280 项测试包含 5 项自构造专项测试，另有 16 个 Artisan 入口子进程探针，包含 `construction:probe`。实际 `./artisan inspire` 正常退出。
- 8 次真实 HTTP 请求验证普通构造、延迟 Provider、自构造依赖失败 500、后续重试、同类普通构造回退和并行应用隔离。
- 三语自构造示例分别编译并执行。文档 `npm run build` 通过，浏览器在根路径和子路径验证容器页导航、正文及本地化，共 12 个组合。

下一部分核对类与参数属性及解析回调，依赖本批的两套参数声明和 01-02p 的环境检测。自动接口投影随后推进。
