# 01-02 第十二部分：闭包与方法调用

2026-11-11 完成本批验收。Container/Application.call、编译方法声明和方法替代绑定已接通；01-02 整体仍未完成。

## 固定参考与旧版提取

- Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：[BoundMethod](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/BoundMethod.php)、[Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 call/getClassForCallable/bindMethod/hasMethodBinding/callMethodBinding、[ContainerCallTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerCallTest.php)。对应固定文档 container#method-invocation-and-injection，及参数、上下文与变参小节。
- 编码前检查旧版 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 的 vendor/openrust/framework/src/container/{call.rs,method_call.rs,method_call_tests.rs}。提取命名/类型键、剩余实参、接收者先构造、绑定跳过注入、替换中回调快照、参数与对象保活、嵌套上下文及取消清理回归。未搬入旧生命周期资源系统；旧测试中无真实声明的对象绑定建立类上下文与固定 PHP is_callable 判断不符，已按上游修正。
- 固定 Goravel v1.18.0 的编译装配保持不变。本批不改 main.rs、bootstrap/、默认路由和配置。

## 正式接口与行为

- 统一入口 `call(target, parameters).await`；Callback 支持借用闭包及借用返回值，MethodCall 表达对象数组、Class@method、默认方法和 __invoke，StaticCall 表达静态字符串/类数组。编译声明 registerMethod/registerStaticMethod 与绑定表分开，不注册或构造对象。
- bindMethod/hasMethodBinding/callMethodBinding 保留 Laravel 名称，静态形式采用类型化重载 bindStaticMethod/callStaticMethodBinding。声明 ASCII 忽略大小写，替代绑定精确匹配调用拼写。直接调用绑定不额外建立类上下文。
- Class@method 先构造对象再读取替代绑定，构造函数可登记新绑定；替代绑定跳过原声明选参与类型校验。进行中的调用保留原绑定快照、接收者及完整输入所有权。用户回调与替换捕获均在锁外执行/释放。
- 方法选参独立于构造：同名、类型原名键、类解析；类默认依据 bound 而非 contextual。原始值不读取 `$name`，nullable 不补缺失参数。剩余输入去键、按插入顺序交给 remaining 或末尾变参；显式数组是一项。计算类集合展开，构造 giveServices 数组不用于方法注入，缺失类依赖不回退空列表。
- 可调用对象或静态目标仅在栈中尚无该类时加帧。Class@method 保留父上下文，符合 BoundMethod.callClass 直接递归 BoundMethod 而非 Container.call 的路径。类上下文与依赖环检测栈分离，允许递归方法调用；逐 poll 和 Drop 恢复原上下文，关闭作用域拒绝后续解析，Application 入口同时建立 Facade 上下文。
- Rust 无 PHP 运行时反射，使用 ConstructorParam、显式编译回调及类型 C；不自动投影 trait/具体类型的方法。存储方法结果要求 Send + 'static，不要求 Sync。Result<R, BindingResolutionError> 表示注入错误，R 可为业务 Result。Rust 1.94 在 Send 异步工厂内组合借用闭包存在 opaque Future 限制，调用分派 Future 使用 Box，保留闭包普通 Future 返回写法。未测性能，不宣称零分配。

## 实际验收

macOS ARM64、Rust 1.94.0。工作区 254 项常规测试、隔离副本 235 项，另有 13 项真实命令子进程通过。fmt、严格 Clippy、locked/offline 独立 standalone release 通过。[Rust 结果](rust-results.json)。隔离构建关闭增量缓存以控制磁盘占用。

- 新增 20 项单元测试：命名/类型键消费、剩余顺序、标量/default/nullable/变参差别、借用返回值、对象身份、构造后绑定、直接绑定上下文、__invoke 声明与默认名、静态两形式、错误返回契约、非 Sync 返回值、绑定/输入/接收者保活、嵌套递归、同容器并发、取消/关闭清理、业务失败重试及 panic 后恢复。
- 新增 2 项应用集成测试，其中 5 次实际 TCP 请求验证延迟依赖、500 脱敏与重试、两应用绑定/Facade 隔离；并发 Application.call 和直接绑定入口通过。
- 新增 method:probe 实际 Artisan 子进程，通过 Application.call 自动构造接收者并注入依赖。原发布 [58 HTTP 与 6 Artisan](http-results.json)、[40 命令进程](command-results.json)通过，包含预期失败。
- [45 个三语 Rust 示例](example-results.json)编译执行通过，三语构建和根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围与复现

[19 文件 tree](changed-tree.txt)：11 个框架实现/契约、5 个测试文件和 3 个三语页面。Application 共享文件只纳入 call 与直接绑定入口；排除既有 gRPC 接线、依赖清单及其他 WIP。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

```sh
python3 .agents/project/audits/01-02l/prepare_candidate.py /tmp/openrust-01-02l-review
cd /tmp/openrust-01-02l-review
cargo +1.94.0 fmt --all --check
CARGO_INCREMENTAL=0 cargo +1.94.0 test --workspace --locked --offline
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_INCREMENTAL=0 cargo +1.94.0 build --locked --offline --release --features standalone
```

三个 verify 脚本接收副本路径；prepare_candidate 从 git-commit.txt 指定的不可变提交恢复并校验，不读取工作区 WIP。

## 剩余边界

SelfBuilding、参数/类属性、投影及剩余公开 API 仍依 01-02j 清单推进。wrap 等便捷入口尚待下一批核对，不声称整个方法/容器 API 齐全。事件 when 与完整流式/终止作用域依原计划推进。未做 Windows 实机验收，未推送。JSON 可选业务封装已按用户指示定位旧版 ApiResponse/PageMeta，仍属 04-02，不在本批引入。

本地代码提交 `eead45deb42e939f6c701aadaf7b3bd492be80cb`，未推送。19 文件从不可变提交恢复校验通过。清理独占 target 1,216,496 KiB；磁盘可用约 52.15 GB，最近额度剩余 94%。保留候选源码、必要发布产物及共享缓存。
