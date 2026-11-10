# 01-02 第七部分：异步延迟 Provider 装载

2026-11-11 完成按服务键装载的本批验收。01-02 整体仍未完成；下一部分关闭 CT-ASYNC-FACTORY-DEFERRED。事件触发装载和其他 Provider 生命周期回调随对应事件/生命周期单元推进。

## 固定参考与复用

- Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php) 中 loadDeferredProvider、registerDeferredProvider、make、bound、loadDeferredProviders 和 boot；[ProviderRepository](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/ProviderRepository.php) 的清单与逐键装载；[DeferrableProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Support/DeferrableProvider.php) 的 provides。
- 固定文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 providers#deferred-providers、container 及 facades。Goravel 继续使用固定 v1.18.0 的编译型装配参考，本批不改 main.rs、bootstrap/、artisan 或配置目录。
- 编码前核对旧版 `6d1a8192:vendor/openrust/framework/src/foundation/provider/deferred.rs` 及 `tests/unit/foundation/deferred_providers.rs`。提取静态 provides/编译工厂、就绪等待、跨 Provider 等待图、取消通知、别名/实例、逐键消费与新实例重试、启动中双 boot/单 boot 的回归思路。旧持久化清单、事件分发、资源关闭和整个旧 Resolver 未整包迁入。

## 本批接口与行为

- `openrust::contracts::support::DeferrableProvider` 声明静态 provides；`openrust::support::DeferredProvider::new(factory)` 放入现有 withProviders 列表，工厂到首次装载才创建对象。按实际 Provider 类型去重，保留首次登记。直接登记现有对象并覆盖 isDeferred/provides 时重用该对象；需要 Laravel 按类重新构造的行为时使用编译工厂。
- Application 的 make/get 保留 Laravel 名称，返回可 await 的结果；Container 的 makeAsync/getAsync 与 Facade::getFacadeRootAsync 提供对应异步通道。原同步解析不阻塞 Tokio：未就绪时返回 DeferredServiceRequiresAsync。register 与已有 boot 一样返回 Future，普通 Provider 注册阶段可 await 另一 Provider。
- 先归一化别名，已有显式实例跳过未使用的装载器。pending 服务参与 bound 和条件绑定判断；拒绝条件覆盖不执行其工厂。
- HTTP 保持按需装载。Console Kernel 在 boot 之后、命令登记之前 loadDeferredProviders。加载消耗当前触发键；同 Provider 的其他键保持清单可见性，load-all 成功才清空剩余清单。注册失败保留贡献，尚未消费的其他键能通过新工厂实例重试。
- boot 阶段前已加载的 Provider 进入 booting 回调和 Provider 列表，两次 boot 是固定 Laravel 的行为。普通 Provider boot 期间首次加载的延迟项只进入剩余启动列表一次；启动后的首次解析等待注册与 boot。
- 同一应用共享真实 ApplicationState、容器、当前配置、路径和引导状态。装载器持弱应用状态引用，不建立第二个应用容器或强引用循环。HTTP Kernel 接收已装配路由；中间件装配失败后再次 bootstrap 返回错误，不因已取走路由而 panic。
- 每个 Provider 使用异步互斥与明确阶段，不在标准锁内 await 用户代码。并发首访共享一次装载，等待图拒绝跨 Provider 的等待环；boot 可以解析本 Provider 已登记服务。
- 错误、取消与 panic 后不把部分初始化对象当作就绪服务。boot 失败后创建新应用重试，已贡献容器值保留。门面的 boot-owner 临时缓存与显式 swap 也参与就绪保护，不能绕过等待屏障。这个长驻并发应用的失败屏障是区别于 PHP 顺序请求执行的 Rust 适配；不声称错误后的所有并发行为逐字相同。

## 验收

macOS ARM64、Rust 1.94.0。工作区 159 项常规测试及 8 项子进程；排除其他模块 WIP 后，隔离副本 140 项常规测试及 8 项子进程通过。fmt、严格 Clippy、locked/offline standalone release 通过。[Rust 结果](rust-results.json)。

- 新增 4 项容器异步门控测试，15 项实际应用/Provider/Kernel 回归。覆盖别名、显式实例、首访并发、注册/启动顺序、条件绑定、逐键消费、新实例重试、类型去重、失败、取消、panic、等待环、重复 Kernel 失败及门面缓存/swap。
- 7 次真实 TCP HTTP 验证懒加载、三个应用隔离、500 脱敏和同请求并发门面保护；1 次 Artisan Kernel 命令读取就绪服务。原入口回归 [58 次 HTTP + 6 次 Artisan](http-results.json) 与 [40 次进程调用](command-results.json)通过。
- [24 个三语 Rust 示例](example-results.json)提取编译执行通过。三语文档站构建、根路径及 /docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围与复现

[38 文件 tree](changed-tree.txt)：1 个应用 Provider、16 个框架/框架测试文件、9 个根测试及 12 个三语页面。[SHA-256](candidate-sha256.json)。必要的现有 Provider 测试/命令子进程示例同步改为异步 register。没有引入新依赖。

6 个共享文件按候选内容纳入必要差异；排除已有 gRPC、广播/安装器及集合骨架。[范围说明](scope-notes.json)。工作区 gRPC 的 ApplicationBuilder、HTTP Server 共享状态适配保留在其 WIP，未混入候选。

```sh
python3 .agents/project/audits/01-02g/prepare_candidate.py /tmp/openrust-01-02g-review
cd /tmp/openrust-01-02g-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在工作区运行本目录 verify_http.py、verify_commands.py、verify_examples.py，参数为该副本。构造器只读取 git-commit.txt 对应不可变提交并校验，不复制工作区 WIP。

## 剩余边界

CT-ASYNC-FACTORY-DEFERRED：同步绑定工厂/解析回调不能 await 尚未装配的 Provider；当前须在异步注册阶段或调用入口先初始化依赖。下一部分提供正式异步工厂组合、同一容器上下文和就绪/失败传播，再关闭这个差异；不以显式预热替代最终自动解析。属性/多接口投影、参数覆盖及完整流式/终止作用域按既有计划推进。事件 when 装载等待事件单元。Windows 尚未实机验收，未推送。

本地代码提交 `ee7b97bf185e3655f1f5edb995850a4952da289a`，未推送。38 文件从不可变提交恢复并校验通过。清理独占 target 1,646,640 KiB；磁盘可用约 61.61 GB，最新额度剩余 96%。候选源码、必要发布产物和共享缓存保留。
