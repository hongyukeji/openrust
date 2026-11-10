# 01-02 第六部分：重绑定、refresh 与配置一致性

2026-11-11 完成重绑定通知、refresh、必要登记错误传播和当前配置读取的本批验收，关闭 CT-CONFIG-REBIND。01-02 仍需延迟 Provider；构造覆盖、属性投影及完整流式/终止作用域依后续单元推进，不标记整个容器或框架完成。

## 固定基线与旧版复用

对照 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：

- [Container.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 bind、instance、extend、rebinding、refresh、rebound。注册监听器先保存，已绑定时解析当前服务但不调用该监听器；回调在变更提交后执行，不回滚已提交值。
- [LoadConfiguration](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bootstrap/LoadConfiguration.php) 先绑定新配置仓储，再加载配置文件。[Application 的核心别名](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php#L1543-L1556) 以 config 为规范名。
- [Facade.swap](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/Facade.php#L171-L177) 先更新门面缓存，再向容器提供实例。固定文档对应 `960eedf0911155ad16326ade49c0ea5d18eda0b8:container.md#rebinding` 和 configuration 章节。Goravel 继续沿用固定 v1.18.0 的编译型启动参考，本批不改 main.rs 或 bootstrap/。

编码前查阅旧版 `6d1a8192:vendor/openrust/framework/src/container/extensions.rs` 的 Rebound::capture/dispatch、rebinding_key、erase_rebinding；核对 binding_event_tests.rs 的 global_extend_after_explicit_only_read_does_not_invent_history，以及已解析服务、别名、快照通知相关回归。提取提交值与有序回调快照、先登记后解析、错误不回滚、显式实例读取不制造历史的机制。旧异步 Resolver、目录版本系统和资源关闭队列未整包迁入。

## 正式接口与语言适配

- rebinding::<T, _>(key, callback) 沿别名登记，返回 Result<Option<Arc<T>>>。已有服务返回当前值，不立即调用监听器；未绑定返回 None。初次解析失败仍保留监听器。refresh 接收明确的 Arc 目标和 setter，代替 PHP 方法名称字符串，初始不调用 setter。
- bind/singleton/scoped 及具名/条件形式返回 Result<()>；instance/instanceNamed 返回 Result<Arc<T>>。这是传播 Laravel 回调异常所必需的 Rust 适配。调用处、测试和三语示例同批处理，不吞错或改成运行时 panic。
- 替换已解析的配方通知监听器；实例只在该原始键此前已经绑定时通知。首次绑定、被拒绝的条件登记和清缓存不通知。实例、缓存装饰与工厂变更使用各自已提交的值/配方快照；并发后继变更不能把新值错交给旧事件，也不能被旧事件写回覆盖。
- 通知前执行 before 查找钩子，需要重建时执行正常构造/装饰/解析回调，再按快照顺序通知。回调错误、panic、类型不匹配都保留已经提交的绑定或缓存，未执行的后续监听器停止。同服务递归通知在嵌套变更提交后返回循环错误。全程不持注册表锁执行用户代码。CT-REBIND-RETIRED-DROP：作用域重绑定替换旧缓存时，把旧值析构延后到注册表读锁和作用域缓存锁释放之后；析构重入登记服务的回归通过。
- 解析过的瞬态/已清缓存共享服务追加未来装饰器时，为监听器重新构造。仅显式实例读取后清缓存，不制造成功历史，也不提前重建。即时扩展回调失败后仍保留已提交缓存。
- Facade.swap 的缓存写入顺序对齐 Laravel；失败时门面与容器仍保留新值，回调中重入门面可看到新实例。原有应用隔离和 scoped 覆盖保持。

## CT-CONFIG-REBIND 闭合

- 删除 Application 内保存配置仓储的旁路字段。Application::config() 及内部装配访问器每次解析规范 config 服务，返回 Result<Arc<dyn contracts::config::Repository>>；错误有明确的 BootstrapError/RoutingError 来源链。公开 Repository::new，供替换仓储时构造实际配置。
- config 是规范名，配置契约是 typed 别名。create 不预绑定空配置；boot 在 Provider 前先绑定新仓储，再运行编译型加载器并填充同一仓储。加载前登记的监听器可观察新空仓储；监听失败则不执行加载器，已提交新仓储保留。预先持有的旧 Arc 不会被暗中改写。
- HTTP 监听、静态磁盘注册与视图路径设置使用 Provider 替换后的仓储。健康页使用弱容器引用，每次 HTML 请求读取当前配置；运行时替换可立即反映应用名称。Artisan 通过同一访问器读取。已装配路由和视图查找器不会因任意配置替换自动重建。
- Rust 的明确契约不隐式发现 PHP 任意类/接口投影；替换时需使用 dyn Repository 契约。多类型投影仍是后续能力，不能把本批 config 的一个 typed 别名算作全部核心别名已经实现。编译型配置加载器返回批量值，绑定仓储后填充，不动态执行 PHP 文件。

## 实际验收

macOS ARM64、Rust 1.94.0。工作区 140 项常规测试及 8 项子进程；排除其他模块 WIP 后，隔离副本 121 项常规测试及 8 项子进程通过。fmt、严格 Clippy、locked/offline standalone release 通过。[Rust 结果](rust-results.json)。

新增 15 项重绑定单元回归，覆盖登记返回/触发时机、顺序、别名、refresh、错误/panic、类型不符、并发值与配方快照、作用域、别名循环修复及装饰链通知。tests/rebinding_lifecycle.rs 的 4 项入口回归验证：

- 两应用共 9 次真实 HTTP：Provider 仓储的有效监听端口（覆盖无效种子端口）、命名视图、/storage/tmp/ 挂载及旧前缀 404；运行时名称变化、隔离、错误类型 500、脱敏及恢复。
- 5 次 Artisan Kernel 调用：配置替换、错误退出 1、恢复，以及 swap 监听器失败后的门面/容器提交顺序。
- 配置在 create 后未绑定；规范 typed 别名正确；先绑定新空仓储再加载，同一被观察 Arc 后续填入配置；回调失败阻止加载器执行并阻止重复 boot。

runApplication 子进程新增 rebind:probe，输出必须为监听器收到的新值 2。[58 次 HTTP + 6 次 Artisan](http-results.json)、[40 次进程](command-results.json)回归通过。[21 个三语 Rust 示例](example-results.json)直接提取编译执行通过；三语站点构建及根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。

## 最小范围与复现

[38 文件 tree](changed-tree.txt)：16 个框架文件、16 个测试文件、6 个三语页面。[SHA-256](candidate-sha256.json)。大量测试改动仅为显式处理新 Result。根应用 config/、main.rs、bootstrap/、artisan 和依赖清单均不改。

共享的 Application、BootstrapError、Routing、HTTP Server、RoutingError、Facade 与 startup 测试共 7 文件，只纳入本批必要差异；[范围说明](scope-notes.json)记录排除项。现有 gRPC/广播/安装器 WIP 保留，未混入候选。工作区广播控制器的一行错误传播及既有 gRPC 分支的配置返回适配保留在其 WIP 内，未引入新模块作为依赖。

```sh
python3 .agents/project/audits/01-02f/prepare_candidate.py /tmp/openrust-01-02f-review
cd /tmp/openrust-01-02f-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在工作区运行本目录 verify_http.py、verify_commands.py、verify_examples.py，参数为该副本。构造器只读取 git-commit.txt 对应不可变提交并校验，不复制工作区 WIP。阶段结束清理独占 target，保留候选源码及 .artifacts/release。

## 尚未完成

继续延迟 Provider，必须解决同步 make 与异步 boot 的真实就绪边界，不用阻塞 Tokio 桥接或后台伪就绪。配置读取和重绑定缺口 CT-CONFIG-REBIND 已通过本批实际入口关闭。动态构造覆盖、属性/多接口投影，以及 HTTP/Console 的完整流式和终止作用域仍依计划推进。Windows 未实机验收。

本地代码提交 `8ee496dc0285bf7bfebff41483d64db543aa09d8`，未推送。38 个文件从不可变提交恢复并校验通过。清理独占 target 1,506,228 KiB；磁盘可用约 63.23 GB，最新实际额度剩余 96%。候选源码、发布产物及共享/工作区缓存保留。

## 后续响应复用输入

用户本次确认统一 JSON 响应可以复用 openrust-v0.x。已核对旧版 `6d1a8192:vendor/openrust/framework/src/core/error.rs` 的 ApiResponse/PageMeta 及 `tests/unit/core/error_i18n.rs` 的消息、请求隔离和序列化脱敏测试；在 plan.md 的 04-02 复用输入中记录。此处仅记录来源，尚未迁入当前响应模块。
