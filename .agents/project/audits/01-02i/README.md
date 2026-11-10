# 01-02 第九部分：工厂参数覆盖

2026-11-11 完成本批验收。完成 makeWith 工厂参数的底层契约；01-02 整体未完成。静态构造描述/自动依赖注入、默认值/变参、call/bindMethod、属性/多接口投影仍待后续，不能以手写工厂替代这些最终能力。

## 固定参考及旧版提取

- 固定 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：[Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 makeWith、resolve、build(Closure)、getLastParameterOverride 和 needsContextualBuild；[上游测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php) 的别名实参数组和共享实例。对应固定文档 container 的 resolving 小节及容器事件。
- 检查旧版 `6d1a8192:vendor/openrust/framework/src/container/parameters.rs`、`class_descriptor_tests.rs`、`selection_tests.rs` 和 `binding_state_tests.rs`。提取精确名称/整数键、异构值/null、顺序覆盖、共享值与快照隔离、参数帧关闭及普通历史隔离的机制和回归思路。旧动态 Value、PHP 标量 Oracle、整个 ClassDirectory/Resolver、资源系统未整包迁入；构造器部分留待同一参数基础上接入。
- 固定 Goravel v1.18.0 的编译型应用装配继续保留。本批不改 main.rs、bootstrap/、默认配置、路由或部署方式。

## 接口与可观察行为

- `openrust::container::{ParameterKey, ParameterValue, ParameterOverrides}`。容器支持 makeWith、makeWithNamed、makeWithAsync、makeWithAsyncNamed；Application 提供 makeWith 和 makeWithNamed 异步入口，沿用本应用门面上下文。
- 空集合沿用普通解析及缓存；非空集合从原工厂构造，绕过现有 instance、singleton/scoped 缓存，仍应用上下文选择、装饰器和回调。结果不替换普通共享值，不写入普通 resolved 历史。不登记工厂的纯 instance 服务收到非空参数明确 NotBound，实例仍可正常读取。
- 当前绑定闭包保留 Fn(&Container) 签名，使用 getLastParameterOverride 读取参数；这是 PHP 工厂第二个数组实参的 Rust 适配。该上游内部名称在 Rust 暴露为工厂读取入口，避免修改既有同步/异步工厂签名。解析前事件可通过 ResolutionEvent::parameters 读取快照。后置回调本身没有额外参数事件载荷，但仍处在本次工厂参数帧内。
- 每次普通嵌套解析使用空帧；需要转交时显式传递当前快照。参数快照身份区分构造：不同参数快照允许有限递归，相同快照对同一服务的环被拒绝；不比较或序列化载荷值来检测递归。
- 克隆参数快照共享不可变集合，后续修改采用写时复制。同键覆盖保留位置并返回旧值；值不要求 Clone/Serialize，from_arc 保留对象身份。Name/Index 精确区分，null 与缺项不同。downcast 不做 PHP 标量隐式转换，类型错误明确 ParameterTypeMismatch，Debug/错误消息不输出载荷内容。
- 每次 poll 恢复当前容器自己的参数，不把参数留在线程本地跨越 await。普通挂起解析移入带参工厂后仍使用自己的空帧。错误/panic/取消恢复前一帧；挂起工厂清理恢复自身参数；退役参数载荷在空帧内释放，不借用外层调用参数。用户主动保留的快照/Arc 仍由用户负责释放。
- 空参数普通读取不增加参数集合或 Future 的堆分配；非空异步覆盖需要一个持有帧的 Future 包装。冷构造既有作用域包装保存参数快照。线程安全载荷要求 Send + Sync + 'static，键和值由 Rust 类型显式表达；没有性能基准，不宣称零开销。

## 验收

macOS ARM64、Rust 1.94.0。工作区 196 项常规测试、隔离副本 177 项，另有 10 项子进程探针通过。fmt、严格 Clippy、locked/offline 独立 standalone release 通过。[Rust 结果](rust-results.json)。

- 新增 13 项单元回归：顺序/快照/脱敏载荷、null 与类型错误、冷/热共享缓存及历史、原工厂与显式实例、别名、嵌套默认空帧/显式转交、before 事件、panic/错误恢复、并发 poll、scoped 隔离、取消清理、不同参数递归、普通挂起解析及参数析构重入隔离。
- 新增实际 HTTP 集成测试，4 次 TCP 请求覆盖两个应用的延迟 Provider/app.makeWith、带参工厂、普通缓存保持、错误参数的 500 脱敏及后续成功请求。新增 parameters:probe 通过真实 Artisan 子进程，输出 12:0，验证应用命名入口与普通缓存分离。
- 原发布入口 [58 次 HTTP 与 6 次 Artisan](http-results.json)、[40 次命令进程](command-results.json)通过，含预期失败。
- [33 个三语 Rust 示例](example-results.json)编译运行通过，文档构建及根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围和复现

[18 文件 tree](changed-tree.txt)：8 个框架实现/契约文件、4 个测试文件和 6 个三语页面。Application 共享文件只纳入两个新入口，既有 gRPC WIP 保留；未改依赖清单或锁文件。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

```sh
python3 .agents/project/audits/01-02i/prepare_candidate.py /tmp/openrust-01-02i-review
cd /tmp/openrust-01-02i-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

三个 verify 脚本接收该副本路径。prepare_candidate 从 git-commit.txt 对应不可变本地提交恢复，不复制工作区 WIP。Windows 实机未验收，未推送。

本地代码提交 `27087b7ce050f464a5e38e6da6d4676e9397647e`，未推送。18 文件从不可变提交恢复校验通过。清理独占 target 1,524,852 KiB；磁盘可用约 58.34 GB，最近额度剩余 95%。保留候选源码、必要发布产物及共享缓存。
