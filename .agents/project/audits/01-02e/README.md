# 01-02 第五部分：扩展绑定与缓存替换

2026-11-11 完成 extend/forgetExtenders 的当前同步工厂、共享缓存和作用域入口验收。重绑定监听、refresh、配置一致性和延迟 Provider 仍在 01-02 后续，不标记整个容器单元完成。

## 固定基线与复用

对照 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Container.extend/resolve/forgetExtenders](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php) 和 [ContainerTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container/ContainerTest.php)。文档对应固定 `960eedf0911155ad16326ade49c0ea5d18eda0b8:container.md#extending-bindings`。沿用固定 Goravel v1.18.0 的编译型启动，不改应用装配或运行入口。

编码前核对旧版 `6d1a8192:vendor/openrust/framework/src/container/extensions.rs`，检查 directory_tests_v2.rs 的别名装饰/清理/循环，binding_state_tests.rs 的原作用域工厂与持久装饰链恢复，descriptor_race_tests.rs 的构造期间追加装饰一次，以及旧 callback_disposal/selection 相关回归位置。提取缓存槽及等待图复用、注册与执行分离、锁外执行、失败重试和退役捕获先释放的机制；不移植旧异步 Resolver、资源关闭队列或未纳入当前契约的全套目录实现。

## 实现与 Rust 适配

- extend::<T, _>(key, closure) 规范化别名并检查输出契约。尚无缓存实例时按顺序保存装饰器，不提前构造；工厂完成后、解析回调前装饰，适用于瞬态、共享和上下文构造。显式 instance 跳过未来装饰链，清除实例后恢复原工厂与原链。
- 已有缓存时立即替换其值，该闭包不加入未来链。旧调用方保留原 Arc，下一查找取得新值。forgetExtenders 跟随别名，仅清除未来链，不反向撤销当前缓存。构造后回调中的即时替换不会被构造收尾覆盖，本次 make 仍返回其原局部对象，后续读取返回缓存替换值。
- 缓存槽复用现有单例状态机，显式实例也通过槽同步，以支持并发即时替换。同槽更新串行、其他线程读取等待，更新线程可读取旧值；跨线程循环使用原等待图拒绝。已在退役槽上开始的更新不能覆盖后来绑定的新实例，等待中的操作重新选择当前槽。
- 构造期间登记的装饰器在缓存发布前纳入一次。注册表和缓存槽采用一致的锁顺序；执行用户代码不持注册表锁或缓存槽锁。已执行装饰器使用独立稳定身份记录，防止释放后地址复用，同时避免把捕获对象延迟释放到发布之后。
- 发布前失败或 panic 的共享构造不留下部分缓存，可重新构造；即时替换失败保留旧指针，不回滚共享对象内部已经发生的可变操作。清理装饰链、退役装饰器捕获和旧对象的析构均在内部锁外。
- 未来链是应用级登记；对当前已缓存 scoped 实例的即时替换只作用于当前生命周期，后续生命周期继续原工厂与未来链。Facade 缓存独立，需清理或使用 uncached facade 才能重新取得替换值。
- Rust 用明确的 Arc<T> 输入/输出代替 PHP 动态对象，trait 包装必须保持登记契约；错误通过 Result 传播。递归扩展返回循环错误。显式实例增加缓存槽同步以支持即时替换，不宣称零开销或已测得吞吐提升。没有装饰器时使用原子标记跳过瞬态装饰查表。

## 验收

macOS ARM64、Rust 1.94.0。工作区 121 项常规测试及 7 项进程检查通过；隔离副本 102 项常规测试及 7 项进程检查通过。fmt、严格 Clippy、locked/offline standalone release 通过。[Rust 结果](rust-results.json)。

新增 17 项单元回归，覆盖别名、顺序、不提前构造、即时/未来链区别、显式实例、类型检查、失败/panic 重试、回调顺序、上下文、构造期间追加、并发读写/循环、作用域、退役捕获析构、trait 包装和重绑不覆盖。tests/extension_lifecycle.rs 的两应用共 4 次真实 TCP 请求验证装饰链、回调中的缓存替换、500 回退和下一请求重试；3 次 Artisan Kernel 调用验证失败退出 1 与后续命令恢复。runApplication 实际子进程增加 extend:probe，输出必须为装饰后的 11。

[58 次 HTTP 和 6 次 Artisan 调用](http-results.json)、[40 次进程](command-results.json)回归通过。[18 个三语 Rust 示例](example-results.json)直接提取编译执行通过；三语站点构建、根路径及 /docs/ 的三语桌面/移动共 12 项浏览器检查通过。

## 范围与复现

[11 文件 tree](changed-tree.txt)、[SHA-256](candidate-sha256.json)。仅必要容器、契约、测试和三语容器正文，无启动、配置或其他模块混入。共享依赖 WIP 保留。

```sh
python3 .agents/project/audits/01-02e/prepare_candidate.py /tmp/openrust-01-02e-review
cd /tmp/openrust-01-02e-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在工作区运行本目录 verify_http.py、verify_commands.py、verify_examples.py，参数为该副本。提交后通过 git-commit.txt 对应不可变 Git 版本恢复；结束时清理独占 target，保留候选源码及 .artifacts/release。

## 后续必做

下一部分接重绑定监听与 refresh，核对绑定变更触发时机、回调错误传播及已经提交的绑定状态，并闭合 CT-CONFIG-REBIND：Application 的直接配置字段不能继续与容器替换的配置契约分离。现有 bind/instance 等登记接口的返回类型须随必要错误传播一起核定，不能用 panic 或悄悄吞错代替 Laravel 的异常行为。随后完成延迟 Provider 与同步解析/异步 boot 的衔接；不使用阻塞运行时桥接。属性投影、构造覆盖及流式/终止作用域依原交付计划推进。Windows 未实机验收。

## 本地提交与清理

代码提交 `daf4a917e6107956fe9de19f11cdb543ce33706a`，11 文件，未推送。不可变 Git 恢复及 SHA-256 校验通过。确认无编译进程后清理独占 target 1,148,320 KiB，约 1.18 GB；候选源码和发布产物保留。清理后磁盘可用约 64.02 GB，最近额度剩余 97%。[清理结果](cleanup-results.json)。
