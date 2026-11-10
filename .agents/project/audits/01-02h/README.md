# 01-02 第八部分：异步工厂组合

2026-11-11 完成本批验收，关闭 CT-ASYNC-FACTORY-DEFERRED。01-02 整体仍未完成；下一部分核对静态构造描述、构造参数覆盖与方法调用，之后闭合属性/多接口投影等缺口。事件 when 及完整流式/终止作用域随既有依赖单元推进。

## 固定参考与旧版复用

- Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Container](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 的绑定、生命周期、resolve、扩展和重绑定机制；[Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php) 的延迟服务装载；文档基线的 container、providers、facades 章节。固定 Goravel v1.18.0 继续用于编译型应用装配参考，本批不改入口、bootstrap 或默认配置。
- 实现前检查旧版 `6d1a8192:vendor/openrust/framework/src/container.rs` 的异步 Factory、Attempt::wait、Construction::publish、关闭作用域，以及 `tests/unit/container/binding_state_tests.rs`。提取先登记通知再查状态、同次失败共享、取消后重试、退役配方不写入新历史和关闭后拒绝解析的回归思路。旧动态 Value、错误格式、资源系统与整个 Resolver 不整包迁入。

## 正式接口及 Rust 适配

- `openrust::container::FactoryFuture`；typed/named 的 bindAsync、singletonAsync、scopedAsync 和各自 If 变体（If 位于 Async 前）。注册需要 await，含必要的重绑定解析。保留所有原同步接口，共用绑定表、别名、条件判断、上下文、类型校验、缓存、装饰器与解析事件。
- 工厂通过当前 Container 的 makeAsync/getAsync 自动装载下一层延迟 Provider，无需手动预热。Application.make/get 为完整查找绑定原应用门面上下文，不另造请求作用域。Facade::getFacadeRootAsync 直接走异步通道，避免重复 before 回调，缓存保留较新 swap 或先完成的根对象。
- 接通 giveAsync/giveServiceAsync/giveConfigAsync/giveTaggedAsync、标签 collectAsync/nextAsync，以及 rebindingAsync/refreshAsync/extendAsync。标签逐项解析被取消时保留当前项。回调、setter 与装饰器本体仍同步；await 初始化放在工厂中，异步回调族未纳入本批。
- PHP 顺序闭包转换为显式 Send Future，Rust 加 Async 名称与 Box::pin 以保持稳定工具链的闭包生命周期及类型擦除。就绪共享缓存快速返回，不创建构造所有者或上下文包装；冷构造和等待需要上下文持有与通知。门面上下文快照可跨 Future 安全持有，缓存从 RefCell 改为 Mutex，用户代码与退役值析构在锁外执行。没有基准数据，不宣称零成本或数值收益。
- 缓存状态机共用同步/异步构造，异步等待不用阻塞 Condvar。每次 poll 恢复消费者和所有者上下文；等待图处理跨分支依赖环。异步构造失败、取消和 panic 唤醒本次等待者并共享错误，后续显式解析可重试。发布后回调失败仍保留共享值，沿用已有行为。同步首访异步工厂明确返回 AsyncResolutionRequired，缓存就绪后可同步读取。
- 解析历史在构造完成、调用后置回调前核对配方快照。构造期间退役的缓存不会污染新配方的历史；后置回调主动 forget 后仍保留本次成功解析历史，遵循 Laravel 的顺序语义。
- 请求/命令退出显式关闭作用域、清空缓存并唤醒挂起解析，避免 cache → service → future → context 保留环。已关闭作用域返回 ScopeClosed；门面 Future 跨应用执行返回 FacadeNotSet。销毁挂起工作恢复仍存活的原应用/作用域，原上下文关闭后采用无门面的清理上下文，不借用新请求。调用者明确保留的 Future、Arc 及其资源仍需调用者 drop，不承诺强制回收用户所有权对象；完整流式/终止作用域仍待后续单元。

## 实际验收

macOS ARM64，Rust 1.94.0，locked/offline。工作区 182 项常规测试，隔离副本 163 项，另有 9 项子进程探针通过；fmt、严格 Clippy、独立 standalone release 通过。[Rust 结果](rust-results.json)。

- 新增 18 项容器单元测试和 5 项应用生命周期测试。覆盖冷/热解析、同步与异步共享、失败原因、取消/panic 唤醒与重试、依赖环、逐 poll 上下文、条件/命名/类型检查、上下文/标签/配置、装饰和重绑定、退役历史、关闭缓存保留环及多应用隔离。
- 本批 8 次真实 TCP HTTP 请求验证嵌套延迟依赖自动加载、三个应用隔离、每请求作用域、500 脱敏与重试、跨请求 Future 拒绝、关闭及存活上下文的清理；3 次 Console Kernel 命令验证错误退出与后续重试/作用域重置。新增 async:probe 实际子进程验证组合工厂输出 7。
- 原发布入口 [58 次 HTTP 与 6 次 Artisan](http-results.json)、[40 次命令进程](command-results.json)通过，含预期失败。完整路径迁移部署、静态文件、JSON/Web 异常和 inspire 保持原行为。
- [30 个三语 Rust 示例](example-results.json)全部提取编译并执行。三语站点构建和根路径、/docs/ 的桌面/移动共 12 项浏览器检查通过。[文档结果](docs-results.json)。首次工作区示例检查发现共享缓存混有另一工具链的 rlib，改用独立 Rust 1.94.0 构建副本验证；未清理共享缓存。示例中的 Result 传播错误已修正后全量通过。

## 最小范围与复现

[25 文件 tree](changed-tree.txt)：12 个框架实现/契约文件、4 个测试文件及 9 个三语页面。两处共享文件只纳入本批接线，已有 gRPC/广播/其他门面 WIP 留在工作区；不改 Cargo.toml、Cargo.lock、config/、main.rs 或 bootstrap/。[范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)。

```sh
python3 .agents/project/audits/01-02h/prepare_candidate.py /tmp/openrust-01-02h-review
cd /tmp/openrust-01-02h-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

验收脚本 verify_http.py、verify_commands.py、verify_examples.py 接收副本目录。构造器从 git-commit.txt 指向的不可变本地提交恢复并校验，不读取未提交工作区。Windows 实机未验收，未推送。

本地代码提交 `a6b0edfb271d07f20aea40a02270a6f98a1b41e0`，未推送。25 文件从不可变提交恢复并校验通过。清理本阶段独占 target 1,468,000 KiB；磁盘可用约 59.08 GB，额度剩余 95%。保留候选源码、必要发布产物及共享缓存。
