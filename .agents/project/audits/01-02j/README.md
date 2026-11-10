# 01-02 第十部分：静态构造描述与自动注入

2026-11-11 完成本批验收。普通具体类构造、默认值与可空参数已接通，01-02 整体未完成；下一部分为变参，再接方法调用、SelfBuilding/属性/投影及剩余公开契约核对。没有把手写绑定工厂当作最终自动注入的替代。

## 固定参考及旧版复用

- 固定 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php)：build、resolveDependencies、resolvePrimitive、resolveClass、resolve/needsContextualBuild、addContextualBinding。对照 [ContainerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php) 的具体类自动构造、嵌套依赖与默认参数。固定文档对应 container 的 resolving、zero-configuration-resolution、binding-interfaces-to-implementations、contextual-binding；仅交付本批对应子节。
- 实现前核对旧版 `6d1a8192:vendor/openrust/framework/src/container/class_descriptor.rs`、class_descriptor_tests.rs、descriptor_resolve.rs、selection_tests.rs、binding_state_tests.rs，提取编译参数描述、类型化槽、元数据不等于绑定、默认值惰性、空值/类型检查、共享配方与历史隔离。旧 ClassDirectory 的 PHP 弱标量转换、属性、接口投影、命名目录与资源系统未整包迁入。
- 固定 Goravel v1.18.0 继续作为编译型应用装配参考；本批不修改入口、bootstrap/、默认配置或部署形式。

## 正式接口及行为

- `openrust::container::{ClassDescriptor, ConstructorParam, ResolvedArgs}`；Container.registerClass、build、buildAsync。registerClass 在普通 Provider.register 中登记不可变元数据，不构造对象，不标为 bound/shared；同描述器克隆幂等，同类型不同定义拒绝。登记和类型错误不在注册表锁内执行或销毁用户捕获。
- PHP 反射替换为类型 ID、静态参数声明与编译构造函数。未绑定但存在元数据的具体类型可通过 make/makeAsync 自动构造；普通成功解析记录历史，参数覆盖不记录。别名、上下文、装饰、回调与现有延迟服务就绪保护共用原容器，不创建第二套服务实例表。
- concrete 使用同步构造器，concreteAsync 使用 Send Future。普通同步构造在异步通道中等待异步/延迟依赖；同步通道不能初始化异步服务时返回类型化错误。trait 依赖通过现有 dyn Trait 绑定注入，保留原 Arc 身份，不冒充自动接口投影。
- 显式同名参数优先。原始值依次选择 `$name` 上下文、默认值、可空值，否则报错。类默认值在无显式绑定和直接上下文时生效；否则解析类。没有默认值的可空类仍尝试解析，不能把缺少构造元数据的类型静默变为 null。
- 参数和默认值按声明顺序求值，完整选参后校验声明类型，再调用构造器。默认函数只在选中时重新求值。空名和重名提前拒绝，null 与缺项分开；原始值不可解析错误包含参数名及正在构造的服务，载荷内容不进入诊断。参数保持严格 Rust 类型，不执行 PHP 弱转换。
- build 直接构造，绕过目标自身绑定/缓存/装饰/解析回调/历史，依赖仍走普通容器解析。已有 singleton/scoped 绑定通过 build 或 buildAsync 指定构造配方，实现共享生命周期；显式带参构造继续隔离普通缓存。
- 构造期间登记的新绑定不被旧构造写入 resolved 历史。已描述类型的装饰器立即校验契约；异步装饰重绑定使用异步描述配方，不回到同步解析。
- 本批核对发现 addContextualBindingAsync 尚未像同步入口及固定 Laravel 一样归一化依赖别名，已修正并加入默认值选择回归；并验证别名在上下文登记之后出现时，直接上下文判断与别名查找的差别。
- Rust 必须显式登记编译元数据，不能反射任意未声明类型；构造函数按验证槽读取参数，服务由 Arc 表达所有权。同步/异步元数据入口共享同一配方身份。没有性能基准，不宣称零开销或数值收益。

## 验收

macOS ARM64、Rust 1.94.0。工作区 214 项常规测试、隔离副本 195 项，另有 11 项子进程探针通过。fmt、严格 Clippy、locked/offline 独立 standalone release 通过。[Rust 结果](rust-results.json)。

- 新增 17 项单元测试：元数据/绑定/历史区分、别名/类型校验、同步与异步嵌套注入、原始值上下文和覆盖、类默认优先级、可空规则、签名拒绝、build 及依赖事件、参数隔离、选参顺序、作用域共享、循环/失败重试、拒绝描述器捕获的锁外销毁、trait Arc、退役历史、默认惰性/新值、异步别名以及装饰/重绑定。
- 新增实际 HTTP 测试的 5 次 TCP 请求，验证未绑定 Worker 的元数据构造、延迟 Client 首次加载、请求间门面重置、三个应用隔离、私有失败 500 脱敏和后续重试。启动时没有构造 Worker/Client。新增 construction:probe 真实 Artisan 子进程自动注入 Greeting，输出 constructed:from provider。
- 原发布入口 [58 次 HTTP 与 6 次 Artisan](http-results.json)、[40 次命令进程](command-results.json)通过，含预期失败。
- [36 个三语 Rust 示例](example-results.json)编译运行通过；三语文档构建与根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围与复现

[16 文件 tree](changed-tree.txt)：6 个框架实现/契约文件、4 个测试文件、6 个三语页面。本批没有共享文件混合暂存；既有 gRPC、配置及其他模块 WIP 保留，不改依赖清单或锁文件。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

```sh
python3 .agents/project/audits/01-02j/prepare_candidate.py /tmp/openrust-01-02j-review
cd /tmp/openrust-01-02j-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

三个 verify 脚本接收副本路径；prepare_candidate 从 git-commit.txt 指定不可变本地提交恢复并校验，不读取工作区 WIP。

## 剩余契约

1. 变参注入：原始值/类/上下文数组、显式覆盖、空列表、失败分类和签名限制。
2. call、bindMethod/hasMethodBinding/callMethodBinding：方法参数、默认方法、静态/实例调用及类型上下文。
3. SelfBuilding/newInstance、类和参数属性、Singleton/Scoped/Bind/BindWhen、属性解析回调与自动接口投影。
4. 容器剩余公开 API：factory、getBindings、flush、环境解析以及全局实例/ArrayAccess 的 Rust 对应；须逐项核对并实现或明确记录语言转换，不能仅凭现有测试认定 01-02 完成。
5. Provider 事件 when 和完整流式/终止作用域仍依既有事件/生命周期单元推进。Windows 实机未验收，未推送。

本地代码提交 `dac781c00346d7b2c99f36be6359e169157750fe`，未推送。16 文件从不可变提交恢复校验通过。清理独占 target 2,403,172 KiB；磁盘可用约 56.73 GB，最近额度剩余 95%。保留候选源码、必要发布产物及共享缓存。
