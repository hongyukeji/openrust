# 01-02 第十一部分：变参注入

2026-11-11 完成本批验收。原始值、类、上下文数组及标签集合的变参已接通；下一部分为 call/bindMethod 等方法调用，01-02 整体仍未完成。

## 固定参考及旧版复用

- 固定 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：[Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 resolveDependencies、resolvePrimitive、resolveClass、resolveVariadicClass；[ContextualBindingBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/ContextualBindingBuilder.php) 的 give(array)、giveTagged；[ContextualBindingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContextualBindingTest.php)。对应固定文档 container#binding-typed-variadics、container#variadic-tag-dependencies 和上下文绑定小节。
- 编码前检查旧版 `6d1a8192:vendor/openrust/framework/src/container/class_descriptor_tests.rs` 的单项/多项/空列表/槽形状/非法签名，descriptor_resolve.rs 的 Services 与 Tagged 消费者上下文、错误分类和部分资源释放，以及 container.rs 的 is_unresolvable_dependency。提取选择和回归机制，未整包搬入旧资源所有权系统或 PHP 标量转换。
- 固定 Goravel v1.18.0 的编译型装配保持不变，本批不改 main.rs、bootstrap/、默认路由或配置文件。

## 正式接口与行为

- ConstructorParam.variadic；ResolvedArgs.values、services。len 统计声明槽位，普通单槽 getter 拒绝变参形状。变参必须最后且不能有默认值，空名/重复名及多变参提前拒绝。
- 同名覆盖是单项，值是向量也不展开；整数键不替代同名槽。原始值缺项返回空列表，类变参缺少绑定或依赖中的原始值不可解析时，整个参数回退为空列表。可构造的类无列表时解析一项；nullable 只允许实际空值项，不把缺项变成 null。
- `needs::<T>().giveServices(keys)` 是 PHP give(array) 的 Rust 类型化重载；底层 addContextualServices 将列表元数据放在同一上下文绑定表，保留原消费者、顺序、重复项与进行中构造的键快照。登记不初始化服务，异步通道自然等待延迟/异步依赖。普通非变参请求同一列表契约仍进行类型校验，不假装它返回单个 T。
- 现有 giveTagged/giveTaggedAsync 使用 `Vec<Arc<T>>` 输出契约，构造变参可消费该集合；T 的直接上下文优先。集合作为独立解析执行，条目不继承原消费者类的上下文；直接服务数组则保留。Rust 集合回调键为 Vec<Arc<T>>，与 PHP 将数组作为同一个 abstract 的动态输出有所区别；这是明确的静态类型适配，不声称回调键逐字一致。
- 原始值上下文接受 Vec<Arc<T>> 或异构 Vec<ParameterValue>，保留无 Clone 值、显式 null 与 Arc 身份；类型校验仍在构造函数执行前完成。ParameterValue 本身作为声明类型时保留对象层级，不误拆载荷。业务失败、类型错误、依赖/别名环、异步要求、Provider 失败、ScopeClosed 不回退为空；原始值变参错误也传播。
- 挂起数组持有已解析部分，取消时在原应用/作用域及参数帧中释放。共享实例保留正常缓存，用户主动保留的 Arc 仍由用户负责。
- 同步/异步原始值上下文改为在构造帧内直接求值，符合 Laravel resolvePrimitive 的闭包调用时机，保留构造参数和消费者，不额外触发 `$name` 的服务事件/缓存/历史。闭包内普通依赖仍使用正常容器解析。这修正上一批通过 get 产生多余解析帧的差异，已加入回归。
- 只为现有 Serde 启用 rc 特性，使 giveConfig 的 Vec<Arc<T>> 配置数组可直接反序列化；没有增加依赖版本。Rust 仍要求显式类型和 Arc 所有权表示；有包装与引用计数成本，未做性能基准，不宣称零成本。

## 实际验收

macOS ARM64、Rust 1.94.0。工作区 232 项常规测试、隔离副本 213 项，另有 12 项子进程探针通过。fmt、严格 Clippy、locked/offline 独立 standalone release 通过。[Rust 结果](rust-results.json)。

- 新增 17 项单元回归：空列表/单项/多项、槽形状、顺序/重复、数组消费者与标签闭包差别、Arc/null/无 Clone 值/异构载体、显式数组不展开、非法签名、缺失与业务/类型/循环/关闭错误、异步等待与键快照、原始值直接求值、取消清理、动态配置数组。
- 新增实际 HTTP 集成的 5 次 TCP 请求，验证变参数组自动加载延迟服务、业务错误 500 脱敏、重试、两个应用隔离和缺失依赖空列表响应。新增 variadic:probe 真实 Artisan 子进程验证有序服务列表，输出 first,second。
- 原发布 [58 次 HTTP 与 6 次 Artisan](http-results.json)、[40 次命令进程](command-results.json)通过，包含预期失败。
- [39 个三语 Rust 示例](example-results.json)编译执行通过，三语构建和根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围和复现

[15 文件 tree](changed-tree.txt)：7 个框架实现/契约文件、1 个依赖清单特性开关、4 个测试文件和 3 个三语页面。清单共享文件仅纳入 Serde rc，排除既有 gRPC/网络依赖改动；根 Cargo.lock 与其他 WIP 保留。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

隔离构建关闭增量缓存以减少本机磁盘占用，测试、优化和默认调试配置不变；工作区同时按普通命令验收。

```sh
python3 .agents/project/audits/01-02k/prepare_candidate.py /tmp/openrust-01-02k-review
cd /tmp/openrust-01-02k-review
cargo +1.94.0 fmt --all --check
CARGO_INCREMENTAL=0 cargo +1.94.0 test --workspace --locked --offline
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_INCREMENTAL=0 cargo +1.94.0 build --locked --offline --release --features standalone
```

三个 verify 脚本接收副本路径，prepare_candidate 从 git-commit.txt 对应不可变提交恢复，不读取工作区 WIP。

## 剩余边界

下一部分为 call、bindMethod/hasMethodBinding/callMethodBinding；后续 SelfBuilding、新实例/参数/类属性、自动投影及剩余公开 API 核对仍按 01-02j 的清单推进。Provider 事件与完整流式/终止作用域依既有单元推进。未做 Windows 实机验证，未推送。

本地代码提交 `b048602f2643e2216aa3e4015441b1d072354fc2`，未推送。15 文件从不可变提交恢复校验通过。清理独占 target 1,277,672 KiB；磁盘可用约 53.51 GB，额度剩余 94%。保留候选源码、必要发布产物及共享缓存。
