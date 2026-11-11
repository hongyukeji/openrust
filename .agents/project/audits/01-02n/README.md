# 01-02 第十四部分：绑定目录与清理

2026-11-11 完成本批验收。Container.getBindings/flush 与原工厂调用已接通；01-02 整体仍未完成。

## 固定参考及旧版复用

- Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc`：[Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的 bind/getBindings/flush、独立的 concrete/shared 定义、instance 与 scoped 状态。基础 Container.flush 不清除上下文、标签、装饰器、解析和重绑回调、方法绑定及环境解析器。
- 对应固定文档 container#binding、container#resolving 及扩展绑定小节，在同一三语容器页面补绑定目录与清理，不另造文档主题。
- 编码前检查旧版 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 的 container.rs flush/detach_for_flush、container/directory/raw.rs 的 get_bindings/FactoryHandle，以及 directory_tests.rs 的 raw_snapshot、supplied_container、raw_call_rebind/closed 回归。提取登记顺序、原工厂保活、shared 语义、传入容器与参数帧、绕过目标状态、锁外析构和旧构造不回填机制；不搬旧资源所有权系统。
- Goravel 固定 v1.18.0 的编译装配不变。本批不改 main.rs、bootstrap/、Application、默认路由、配置或依赖。

## 正式接口与行为

- getBindings 返回有所有权、按登记顺序排列的 Bindings；按 ServiceKey 精确查键，支持 iter/keys/所有权迭代及修改快照中既有定义。BindingDefinition.concrete/shared 对应 PHP 绑定数组字段；原 shared 不因显式实例覆盖而改变。替换绑定保持顺序，条件绑定不插入重复项。实例单独键、别名单独键、上下文与自动编译元数据不列入目录。
- FactoryHandle.call/callAsync 使用保留的原工厂与调用者传入的容器/参数。类型错误先于用户执行；异步原工厂在同步入口拒绝，即使普通解析已经预热。绕过目标缓存、装饰器、事件与历史，不添加目标消费者帧；工厂内部依赖仍走普通解析并屏蔽顶层参数。sameFactory 比较来源身份。
- 原工厂快照不持有已预热 singleton 的缓存对象，后续绑定替换、实例覆盖、forget 或 flush 不改变快照。直接执行共享原工厂也每次构造新结果，不将它伪装为正常 make 或 factory 延迟解析入口。
- flush 原子解除绑定、别名/反向别名、显式/共享/scoped 实例、scoped 登记和历史；保留上下文、标签、装饰/解析/重绑回调、方法声明/绑定、编译类描述。用户值在状态提交后及锁外释放，可重入；单个析构 panic 不回滚状态或污染注册表锁。
- 所有活动请求缓存同时解除，但生命周期保持开放。以 Weak 活动记录与既有注册表→缓存锁序实现；登记/注销为短 Mutex 操作，不对每次新请求扫描其他请求。已准入旧构造可返回原值，但不回填新目录或历史；外部持有的 Arc 继续有效。flush 不重新启动 Provider，也不清空独立 Facade 缓存，不代替 Application 终止。
- 审计发现 scoped 缓存析构可能继承清理者的参数与类上下文。本批统一 flush 与正常 scope 结束的中立清理：屏蔽 Facade、参数和消费者帧；显式生命周期 Guard 负责关闭，不依赖最后一个保留 Arc 的释放。原有保留 Future 关闭/取消回归继续通过。
- Rust 用 Bindings/BindingDefinition/FactoryHandle 替代 PHP 动态数组与闭包。快照读取线性构造，精确查找线性；这是显式检查，不加入普通解析路径。Weak 生命周期记录维护及异步 Future/参数载体有成本，未做性能基准，不宣称零成本。

## 实际验收

macOS ARM64、Rust 1.94.0。工作区 277 项常规测试、隔离副本 258 项，另有 15 项真实命令子进程通过。fmt、严格 Clippy、locked/offline 独立 standalone release 通过。[Rust 结果](rust-results.json)。隔离构建关闭增量缓存以控制空间。

- 新增 13 项单元测试：目录顺序/过滤/shared、快照独立及来源身份、原工厂绕过缓存/装饰/事件/历史、输出预检、缓存对象不被快照保留、传入容器与参数屏蔽、异步原工厂、flush 保留规则/标签/方法/元数据、重入和 panic、多个活动 scope 同时清理后继续执行、进行中 singleton 不能回填、原工厂替换后续跑及关闭 scope 拒绝、flush/正常 scope 结束屏蔽清理者的参数与类上下文。
- 新增实际 HTTP 集成，6 次 TCP 请求验证子容器目录的原工厂失败 500 脱敏、重试、清理后重绑与旧快照保活、两应用隔离。真实请求的 scoped 析构检查当前 Facade 已被屏蔽。使用应用中的独立服务目录，不拿 Container.flush 冒充 Application 重启。
- 新增 directory:probe 真实 Artisan，读取应用容器原工厂并调用，验证独立目录清理/重绑。原发布 [58 HTTP 与 6 Artisan](http-results.json)、[40 命令进程](command-results.json)通过，包含预期失败。
- [51 个三语 Rust 示例](example-results.json)编译执行通过；三语构建和根路径、/docs/ 的三语桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。

## 最小范围与复现

[12 文件 tree](changed-tree.txt)：5 个框架实现/契约、4 个测试文件和 3 个三语页面。排除既有 Application/gRPC、依赖清单与其他模块 WIP。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

```sh
python3 .agents/project/audits/01-02n/prepare_candidate.py /tmp/openrust-01-02n-review
cd /tmp/openrust-01-02n-review
cargo +1.94.0 fmt --all --check
CARGO_INCREMENTAL=0 cargo +1.94.0 test --workspace --locked --offline
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_INCREMENTAL=0 cargo +1.94.0 build --locked --offline --release --features standalone
```

三个 verify 脚本接收副本路径；prepare_candidate 从 git-commit.txt 指定的不可变提交恢复校验，不读取工作区 WIP。

## 剩余边界

下批核对 resolveEnvironmentUsing/currentEnvironmentIs、全局实例和 ArrayAccess 的 Rust 对应；环境绑定随后用于类属性。SelfBuilding、新实例/类与参数属性、自动接口投影仍待实现。后续增加 offsetUnset 时需同时维护绑定顺序。事件 when 与完整流式/终止作用域依原计划推进。01-02 未整体完成；Windows 实机未验收，未推送。JSON 可选业务封装依 04-02 从旧版提取。

本地代码提交 `d4b348ce4283851e405626580f4ee8272d2043e6`，未推送。12 文件从不可变提交恢复校验通过。清理独占 target 1,348,996 KiB；磁盘可用约 51.86 GB，最近额度剩余 93%。保留候选源码、必要发布产物及共享缓存。
