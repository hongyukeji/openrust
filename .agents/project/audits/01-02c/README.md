# 01-02 第三部分：上下文绑定与标签服务

2026-11-11 完成上下文工厂、服务名实现、配置输入和标签集合的本批验收。01-02 的扩展/解析/重绑定回调及延迟 Provider 尚未完成，不标记整个容器章节或交付单元关闭。

## 固定基线与复用

- Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：[ContextualBindingBuilder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/ContextualBindingBuilder.php)、[Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 when、addContextualBinding、resolve、getContextualConcrete、tag/tagged、原键别名索引，以及 [ContextualBindingTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContextualBindingTest.php)。文档对应固定 `960eedf0911155ad16326ade49c0ea5d18eda0b8:container.md` 的 contextual-binding、binding-primitives、binding-typed-variadics、tagging。
- Goravel 继续参考固定 v1.18.0 的编译型应用装配；公开行为以 Laravel 为准，本批不改应用入口或运行方式。
- 编码前核对旧版 `6d1a8192:vendor/openrust/framework/src/container/contextual.rs`、descriptor_source.rs；检查 attribute_resolution_tests.rs 的显式输入以及 call_tests.rs 中 StaticOwner/StaticChild 的独立上下文回归。复用消费者到依赖的类型化查表、构造前类型检查、配置反序列化与原键有序标签思路；旧异步 Resolver、构造属性反射和目录系统未整包迁入。

## 实现与语言适配

- 接通 when/needs/give、whenNamed/whenMany、needsNamed、giveService、giveConfig、tag/tagged 和 giveTagged。上下文仅选择最近的消费者，优先于根实例和作用域缓存；结果不写入默认缓存或默认 resolved 历史。
- 同一依赖的上下文工厂可调用默认实现。循环检测同时核对服务键和当前配方身份，允许上下文到默认实现的委托，仍拒绝同配方递归；失败和 panic 自动恢复构造栈。
- when 固定消费者的当前别名，give 固定依赖的当前别名。后续直接别名通过目标服务的反向索引查找，保留登记顺序；别名改指向或按原键重新绑定后不使用旧索引。无需扫描整个别名表，未宣称测得性能收益。
- Rust 没有 PHP 构造反射或动态参数重载，使用已登记的同步工厂身份作为消费者，显式解析构造输入。typed 服务名用泛型，原字符串用 Named 形式；give 接收工厂，服务名形式为 giveService。标量值在工厂内返回 Arc，变长输入显式使用 Vec<Arc<T>>。不隐式转换 Arc<Concrete> 为任意 dyn Trait。
- giveConfig 在实际解析时读取当前容器 config 契约。None 表示省略默认值；缺失使用默认，显式 null 保持 null。使用 Serde 检查目标 Rust 类型，不默默套用 PHP 标量转换。唯一新增直接依赖为已锁定的 serde；gRPC 等工作区依赖变化排除在本批外。
- tag 保留原键、顺序和重复项。tagged 不提前构造服务，每轮迭代重新解析；首次 len 计数缓存，与 Laravel 可重复生成器一致。未知标签的空序列不受之后新建同名标签影响。giveTagged 收集集合并传播单项错误。替换上下文工厂的捕获对象在注册表锁外析构。

## 实际验收

macOS ARM64、Rust 1.94.0。工作区 88 项常规测试与 5 项真实 CLI 子进程检查；隔离副本 69 项常规测试与 5 项子进程检查通过。两处 fmt、严格 Clippy 及隔离 locked/offline standalone release 构建通过。最终别名索引调整后再次通过 Rust 检查和实际入口回归。

新增 12 项 contextual 单元回归，覆盖缓存优先级、同依赖默认委托、最近消费者、多个消费者、别名增改、类型前置拒绝、失败/panic 重试、作用域覆盖、配置默认/null/类型、标签惰性与重复迭代、析构重入及跨线程/嵌套应用隔离。tests/contextual_lifecycle.rs 通过两应用共 4 次并发真实 TCP 请求及 3 次 Artisan Kernel 调用；container_process 实际 runApplication 子进程新增 context:probe。

[58 次 HTTP 和 6 次 Artisan 调用](http-results.json)、[40 次进程检查](command-results.json)通过；涵盖原首页、健康检查、静态优先级与安全边界、异常以及命令成功/失败契约。[三语 12 个完整 Rust 示例](example-results.json)提取编译运行通过；三语站点构建及 `/`、`/docs/` 的三语桌面/移动共 12 项浏览器检查通过。

## 最小范围与复现

[13 文件 tree](changed-tree.txt)、[文件表](changed-files.txt)、[SHA-256](candidate-sha256.json)。共享 Cargo.toml 和 Cargo.lock 只纳入 serde 的必要差异；从不可变提交恢复候选，不读取共享依赖文件里的其他 WIP。其他模块和根应用的改动保留。

```sh
python3 .agents/project/audits/01-02c/prepare_candidate.py /tmp/openrust-01-02c-review
cd /tmp/openrust-01-02c-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在工作区运行本目录 verify_http.py、verify_commands.py、verify_examples.py，参数为上述副本。提交后构造器从 git-commit.txt 对应的不可变提交恢复并校验，阶段完成清理独占 target；候选源码和 .artifacts/release 保留。

## 剩余边界

01-02 后续继续扩展、解析/重绑定回调和延迟 Provider。CT-CONFIG-REBIND 仍待验证 Application 配置字段与重绑定的契约一致，不能用本批 giveConfig 读取当前契约的结果代替它。反射属性与方法调用按计划各自交付；本批没有加入 PHP 运行时反射。HTTP/Console 的流式与终止回调作用域仍归对应单元闭合。Windows 未实机验收。

## 本地提交与清理

代码提交 `66937849eb46f5483627b6aba538cc44da954491`，13 文件，未推送。不可变提交重建和 SHA-256 校验通过。确认没有编译进程后清理本阶段独占 target 1,333,160 KiB，约 1.37 GB；源码及必要发布产物保留。清理后磁盘可用约 65.11 GB。最近额度剩余 98%，未触发停止条件。[清理记录](cleanup-results.json)。

## 后续复核纠正

01-02d 复核固定源码后确认：tagged 的数量在创建序列时固定，原文“首次 len 计数缓存”不准确。已修正实现、三语正文并补强首次计数前追加标签的回归；详见 [01-02d](../01-02d/README.md#tagged-纠正)。本页旧验收记录不证明该边界正确。
