# 01-02 第十三部分：延迟调用闭包

2026-11-11 完成本批验收。Container/Application.factory 与 wrap 已接通，01-02 整体仍未完成。

## 固定参考及旧版复用

- Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：[Container.wrap/factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php)、[ContainerCallTest.testCallWithDependencies](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerCallTest.php)。固定文档 container#method-invocation-and-injection 和 resolving；相同三语正文补延迟闭包小节。
- 编码前检查旧版 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 的 container/{call.rs,method_call.rs,binding_state_tests.rs,directory/raw.rs}。未找到可直接复制的 Laravel factory/wrap 公开入口；复用本主线前批已提取的参数快照、实时绑定替换、缓存及作用域解析机制，补旧 binding_state_tests 的 instance/forget 后原工厂仍有效等回归，不把旧版直接原工厂调用当作 Laravel factory。
- Goravel 固定 v1.18.0 的编译装配不变。本批不改 main.rs、bootstrap/、默认应用路由、配置或依赖清单。

## 正式接口与行为

- Container.factory/factoryNamed 返回同步闭包，factoryAsync/factoryAsyncNamed 为异步重载；Application.factory/factoryNamed 返回建立应用 Facade 上下文的异步闭包。具名形式保存原始拥有所有权的 ServiceKey，每次重新解析别名。
- 创建闭包不检查或构造服务；每次执行调用正常 make/get，不提前固定工厂、实例、缓存或作用域。后续绑定/别名/实例替换及 forget 立即可见，bind 重建、singleton/scoped 复用行为与普通解析一致；错误可重试，异步等待延迟 Provider。
- wrap 保存回调及原始参数快照，每次执行正常 call；参数写时复制，外部修改不影响保存值，依赖与默认值实时选择。回调状态共享，不克隆回调，也不缓存返回值。每次 Future 独立，取消不消耗包装器，已关闭作用域拒绝继续解析。
- Rust 的 Callback::reusable 明确 Fn + Send + Sync，避免将内联异步闭包先按 FnOnce 推断后再要求重复执行的编译限制；Callback::new 的一次性调用能力及既有用法保留。可变共享状态由同步机制表达，不自动克隆捕获。返回闭包借用容器/应用，不能超过其生命周期；可返回外部借用值。
- 返回包装器使用具体 impl Fn，不增加包装器 Box。内部回调共享使用 Arc，异步 Future 沿用现有 Box 适配，不宣称零分配或实测性能收益。

## 实际验收

macOS ARM64、Rust 1.94.0。工作区 263 项常规测试、隔离副本 244 项，另有 14 项真实命令子进程通过。fmt、严格 Clippy、locked/offline 独立 standalone release 通过。[Rust 结果](rust-results.json)。隔离构建关闭增量缓存以控制磁盘占用。

- 新增 8 项单元测试：懒登记、实时别名/实例/forget、普通与 scoped 缓存、异步失败重试、参数快照及实时依赖、共享非 Clone 回调与新鲜默认值、外部借用、消费者上下文、取消释放及已关闭作用域。
- HTTP 应用集成的 5 次 TCP 请求改走 wrap 和 factoryAsync，覆盖延迟 Provider、500 脱敏、重试与两应用隔离；新增应用集成验证 factory/factoryNamed/wrap 并发 Facade 隔离及实时实例替换。
- 新增 lazy:probe 实际 Artisan，通过 Application.factory 和 wrap 调用，并在异步等待后读取同一应用 Facade。原发布 [58 HTTP 与 6 Artisan](http-results.json)、[40 命令进程](command-results.json)通过，包含预期失败。
- [48 个三语 Rust 示例](example-results.json)编译执行通过；三语构建和根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围与复现

[11 文件 tree](changed-tree.txt)：4 个框架实现/契约、4 个测试文件和 3 个三语页面。Application 共享文件仅纳入 factory/factoryNamed/wrap，排除既有 gRPC 与其他 WIP。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

```sh
python3 .agents/project/audits/01-02m/prepare_candidate.py /tmp/openrust-01-02m-review
cd /tmp/openrust-01-02m-review
cargo +1.94.0 fmt --all --check
CARGO_INCREMENTAL=0 cargo +1.94.0 test --workspace --locked --offline
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_INCREMENTAL=0 cargo +1.94.0 build --locked --offline --release --features standalone
```

三个 verify 脚本接收副本路径；prepare_candidate 从 git-commit.txt 指定的不可变提交恢复校验，不读取工作区 WIP。

## 剩余边界

下一批继续核对 getBindings/flush/环境与全局实例等公开 API 的 Rust 对应，再按依赖补 SelfBuilding、新实例和属性、自动接口投影。事件 when 与完整流式/终止作用域按既有计划推进。01-02 整体未完成；未做 Windows 实机验收，未推送。可选业务 JSON 封装仍属于 04-02，按用户授权复用旧 ApiResponse/PageMeta。

本地代码提交 `ea4fd4bffd29ebd03897e7c3d4e541712e3ed1d6`，未推送。11 文件从不可变提交恢复校验通过。清理独占 target 1,216,816 KiB；磁盘可用约 51.97 GB，最近额度剩余 93%。保留候选源码、必要发布产物及共享缓存。
