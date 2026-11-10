# 01-01 容器、Provider 与 Facade 基础

2026-11-11 本单元指定的基础绑定、接口解析、别名、Provider 顺序、应用隔离及循环依赖验收通过。Git 基线 `197e7a0e`，承接 00-03 累计候选。未暂存、提交或推送。

## 固定对照与旧版复用

Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：

- [Container.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 bind、singleton、instance、alias、getAlias、isAlias、has、get、make。
- [ContainerTest.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php) 的绑定、共享实例、接口解析及别名行为。
- [Facade.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/Facade.php) 的 getFacadeAccessor、getFacadeRoot、缓存、clearResolvedInstance、clearResolvedInstances、swap。
- Application 的 Provider 注册后引导顺序，ServiceProvider 的可选钩子。保留前序已验收的去重和启动错误传播。
- 文档快照 `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 container#binding、container#resolving、providers#writing-service-providers、facades#how-facades-work、contracts。

Goravel framework `a554ced468cada50a269780692a0491e5f9e3a11:contracts/foundation/application.go` 的 Bind、Singleton、Instance、Make 作为编译型接口参考；Laravel 仍是行为基线。

先检查 `openrust-v0.x` 的 `6d1a8192`。提取 `src/container/service_key.rs` 的 TypeId/命名键独立身份与哈希实现；复用 `src/container/directory.rs` 的原键别名、晚绑定和链末端解析思路，以及 `directory_tests.rs` 的 dangling_and_cyclic_aliases_are_bound_without_eager_resolution、raw_shared_and_forget_do_not_follow_alias_but_make_and_history_do 回归意图。旧版反射描述、异步资源目录及其他生命周期能力没有整包迁移；按后续容器单元逐簇接入。

## 本批实现与 Rust 适配

- 类型和命名项共用 ServiceKey 目录；alias/getAlias/isAlias/has/get 接通。别名与绑定在同一读锁内选定，工厂在锁外执行；返回 Arc 保持已取得实例有效。
- Rust 不能按参数数量重载 bind 等方法，因此保留既有类型形式，增加 bindNamed、singletonNamed、instanceNamed 命名形式。make 按类型，get 按显式键；命名输出类型在执行工厂前校验。接口以 dyn Trait 和显式工厂表达，不假装存在 PHP 构造反射。
- 单例继续共享首次成功结果。失败和 panic 后可重试，同线程与跨线程工厂循环会返回错误。别名允许晚绑定及循环登记；真正跟随循环时明确报错，避免递归溢出。
- Application 通过 Arc 持有容器，默认配置契约保留 typed 绑定并增加 config 别名。
- `support::facades::Facade` 以 trait 表达基础类职责，保留 Laravel 方法名及 cached 属性意图。显式静态转发替代 PHP __callStatic，Rust 编译时检查服务方法。门面缓存属于执行作用域，不使用进程级全局应用指针。
- bootstrap、HTTP 请求、Artisan handle/call 自动设置门面作用域；同应用嵌套执行共享，不同应用隔离。每个新请求和命令建立新缓存。自行 spawn 的任务不自动继承作用域，应显式传入服务。
- Provider register/boot 可省略，默认成功且无操作。保留前序启动的 fail-stop 规则：部分引导失败后，同一应用不重试，需创建新应用，以避免长运行 Rust 进程中重复初始化副作用。错误保留原始 source，公开阶段信息不直接输出私有 source 文本。

没有据此声称性能更快。类型检查和显式工厂是语言机制适配，本批没有性能基准。

## 需求与证据

| 条件 | 当前证据 |
| --- | --- |
| 类型/命名绑定、实例、共享和接口解析 | framework/tests/unit/container.rs 共 8 项回归，含原有 3 项 |
| 别名晚绑定、链、覆盖、原键状态与循环错误 | 同上 named_bindings、aliases_allow_late_bindings、trait_contracts 用例 |
| 错误输出类型不执行工厂 | named_output_type_is_checked_before_the_factory_runs |
| 失败/异常单例可重试、跨线程循环不死锁 | singleton_retries、singleton_panic、cross_thread_singleton_cycles |
| Provider 全部注册后再引导、去重及幂等 | tests/startup.rs 既有生命周期用例 |
| 可选钩子、注册失败、source 保留 | tests/container_lifecycle.rs |
| 两应用 HTTP、门面缓存及清除/替换 | 同上，5 次真实 TCP 请求，包含并发交错及清理时析构重入 |
| Artisan 作用域与生命周期隔离 | 同上，两个 Kernel 和重复命令作用域 |
| 真实 Artisan 进程入口 | tests/container_process.rs，实际 runApplication 及 Provider，成功/未知命令/错误模式共 3 个子进程 |
| 三语文档 Rust 示例 | verify_examples.py 直接提取 6 个代码块，Rust 1.94 编译并运行通过 |

## 验收与文件范围

macOS ARM64、Rust 1.94.0。工作区 52 项常规测试和 3 项子进程检查通过；隔离候选 33 项常规测试和相同 3 项子进程检查通过。两者格式及严格 Clippy 通过。隔离 locked/offline standalone release 通过。

在新候选上重新运行前序真实请求脚本，[58 次 HTTP 与 6 次命令结果](http-results.json)及 [40 次进程结果](command-results.json)通过。三语站点构建及根路径、/docs/、三语桌面/移动共 12 项浏览器检查通过。验收结果只关闭本批正文段落。

[增量 26 文件 tree](changed-tree.txt)；[累计 428 个候选路径](commit-tree.txt)，另有 [1 个前序必要删除路径](deleted-files.txt)。[SHA-256](candidate-sha256.json)和[排除补丁](scope-exclusions.patch)保留工作区其他模块 WIP。独立重新构造，428 个选定文件校验一致。项目规范、计划、进度及本目录的审计记录作为项目管理文件单独审阅，不计入运行配套数。

```sh
python3 .agents/project/audits/01-01/prepare_candidate.py /tmp/openrust-01-01-review
cd /tmp/openrust-01-01-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在项目根目录验证文档示例：

```sh
python3 .agents/project/audits/01-01/verify_examples.py /tmp/openrust-01-01-review
```

## 未关闭的依赖

01-02 继续条件绑定、状态查询、scoped 生命周期、上下文绑定、扩展/解析回调及延迟 Provider。Facade 的 resolved 回调依赖容器回调，Mocking 及 fake API 在 T-04 随替身模块验收。其他门面类型的服务实现随所属单元交付。PHP 运行时反射与动态静态调用采用上述 Rust 映射，不能据此声称完整 PHP 反射 API 已实现。Windows 实机仍待验收。

## 资源门槛与清理

用户新增额度剩余 ≤25% 或磁盘可用 ≤50 GB 时收尾停工。本批检查额度剩余 99%、磁盘可用约 63.59 GB，未触发。每个新单元、大型构建前及持续开发过程中复查，无法读取额度时不假设充足。

完成验收记录后清理本阶段独占且无运行任务使用的 target 和隔离构建缓存，保留候选源码与 `.artifacts/release/` 的必要二进制及欢迎页。目录和清理后空间见 cleanup-results.json。共享下载缓存不清理；根工作区 target 为共享历史缓存，不能准确区分归属，先保留。恢复时按上面的构建命令重新生成编译产物。

## 本地提交

按用户 2026-11-11 新授权，已提交本地 Git：`cca9680d2a8ebca1d9031c8937e7709b2d92024a`。本地提交范围为 26 个变更路径；前述“未提交”描述为验收完成时的历史状态。未推送。工作区中候选排除的其他模块内容保留。
