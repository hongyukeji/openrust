# 01-02 第四部分：解析回调与共享对象发布

2026-11-11 完成 beforeResolving、resolving、afterResolving 的当前显式工厂入口、实际 HTTP/Artisan 与三语正文验收。同时纠正 01-02c 的 tagged 计数时机。extend、重绑定及延迟 Provider 继续留在 01-02，不标记整个交付单元完成。

## 基线和旧版复用

对照固定 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Container.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Container/Container.php)：resolve 的事件、缓存和 resolved 顺序，beforeResolving/resolving/afterResolving 登记及各派发阶段，getCallbacksForType 与 tagged。文档对应固定 `960eedf0911155ad16326ade49c0ea5d18eda0b8:container.md#container-events`。继续沿用固定 Goravel v1.18.0 的编译型应用方式，不改应用入口。

编码前核对旧版 `6d1a8192:vendor/openrust/framework/src/container/resolving_hooks.rs` 和 extensions.rs。提取 ResolutionEvent、全局/指定选择器的有序分组、分阶段快照、锁外执行及失败后共享值保留的机制。特别复用 hooks_preserve_selector_groups_and_read_registrations_after_global_callbacks、failed_completed_hook_leaves_published_singleton_without_success_history 的回归意图。旧异步 Resolver、ClassRegistry 投影和关闭资源队列未整包引入。

## 实现与差异

- 三个公开方法保留 Laravel 名称，None 为全局，Some(ServiceKey) 为指定选择器，登记时规范化别名。回调接收 ResolutionEvent 和当前容器，返回 Result。event.key 是查找开始确定的规范身份，event.value::<T>() 取得原有 Arc；构造前或契约不匹配时为 None。
- 各阶段依次全局后指定；全局先快照，执行后再取指定回调快照，保留阶段内后登记指定回调的行为。before 在缺失查找及缓存命中前执行，允许登记缺失服务；显式实例和共享缓存命中不重复执行构造后回调。
- 共享对象在构造后回调前发布，当前线程回调可重入读取同一实例；其他线程等待当前构造后回调结束。复用现有等待图检测跨线程循环，避免把配置到一半的对象交给并发调用方。同步工厂与回调不跨 await。
- 回调失败或 panic 时，共享对象已发布缓存保留，但不记成功解析历史；缓存命中不重跑失败回调。清掉缓存后没有旧的成功记录，下次重新构造。瞬态失败不缓存，before 失败不执行构造。作用域缓存的发布标记与根缓存独立，保持原配方身份和跨请求隔离。
- 回调执行和捕获对象释放不持注册表锁。没有配置某阶段回调时，用原子阶段标记跳过回调表读取；无构造后回调时不创建事件对象。不宣称测得性能收益。
- Rust 无 PHP 动态重载、任意继承关系查询或构造反射。使用显式服务键及已登记输出类型匹配，trait 对象须按实际契约登记；对象配置使用内部可变性。显式参数覆盖、属性/多接口投影仍需其对应实现，不用空参数集合伪装已完成。无终止条件的同服务同阶段回调递归返回循环依赖错误，防止进程栈溢出。

## tagged 纠正

上一批把数量固定时机误写成首次 len；固定 Laravel 的 tagged 实际将创建时的 count 直接传给 RewindableGenerator。本批改为创建序列时固定数量。强化原回归：创建序列后、首次 len 前追加标签，len 保持创建时的数量；迭代仍读取追加的服务。三语正文同步纠正。01-02c 的旧测试记录保留，不能用旧记录证明这个边界正确。

## 验收

macOS ARM64、Rust 1.94.0。工作区 102 项常规测试和 6 项进程检查通过；隔离副本 83 项常规测试和 6 项进程检查通过。fmt、严格 Clippy 和 locked/offline standalone release 构建通过。[Rust 结果](rust-results.json)。

新增 12 项回调单元回归，覆盖阶段顺序、别名、缺失服务、共享/显式实例缓存、共享回调重入、失败与 panic、重试、递归拒绝、阶段登记快照、输出类型匹配、并发等待和作用域隔离。tests/resolving_lifecycle.rs 通过两应用共 4 次实际 TCP 请求，验证正确输出、500 回退与下一请求重试；3 次 Artisan Kernel 调用验证失败退出 1、后续成功和新作用域。实际 runApplication 子进程新增 hooks:probe，输出必须由回调登记的 hook-result 服务取得。

[58 次 HTTP + 6 次 Artisan](http-results.json)、[40 次进程](command-results.json)重验通过。[15 个三语 Rust 示例](example-results.json)直接提取编译执行通过，三语站点构建和 `/`、`/docs/` 的三语桌面/移动共 12 项浏览器检查通过。

## 范围与复现

[13 文件 tree](changed-tree.txt)、[SHA-256](candidate-sha256.json)。仅容器、作用域发布元数据、测试和三语容器正文，无共享依赖文件或应用模块混入。其他工作区改动保留。

```sh
python3 .agents/project/audits/01-02d/prepare_candidate.py /tmp/openrust-01-02d-review
cd /tmp/openrust-01-02d-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在工作区运行本目录 verify_http.py、verify_commands.py、verify_examples.py，参数均为该副本。提交后从 git-commit.txt 对应不可变提交重建；阶段结束清理独占 target，保留候选源码和 .artifacts/release。

## 后续

下一部分继续 extend 和重绑定/refresh，再推进延迟 Provider。CT-CONFIG-REBIND 仍未解决：Application.config() 返回直接字段借用，而动态容器解析返回 Arc，需按 Rust 所有权明确公开返回与错误传播方式，并验证路由装配同步取当前契约。不能以本批回调或 giveConfig 的局部结果代替这个验收。流式与终止作用域、反射属性及调用参数按对应单元推进；Windows 未实机验收。

## 本地提交与清理

代码提交 `e6cdc944f86df5408a68f56aceb2c9448caef065`，13 文件，未推送。不可变 Git 恢复及 SHA-256 校验通过。确认无编译进程后，清理独占 target 1,108,028 KiB，约 1.13 GB；候选源码及发布产物保留。清理后磁盘可用约 64.46 GB，最近额度剩余 97%。[清理结果](cleanup-results.json)。
