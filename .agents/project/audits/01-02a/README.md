# 01-02 第一部分：条件绑定与实例状态

2026-11-11 完成本部分实现与验收。01-02 整体仍在推进，作用域、上下文、扩展/解析回调及延迟 Provider 尚未关闭。本部分不代表整个容器单元或页面全部完成。

## 对照与复用

固定 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc:src/Illuminate/Container/Container.php` 的 bindIf、singletonIf、isShared、resolved、forgetInstance、forgetInstances，以及实例与工厂分别保存的行为。文档对应固定 `960eedf0911155ad16326ade49c0ea5d18eda0b8:container.md#binding` 和 resolving 相关段落。

先核对旧版 `6d1a8192:vendor/openrust/framework/src/container/directory_tests.rs` 的 conditional_registration_noop_preserves_epoch_cache_callbacks_and_drops_outside_state、concurrent_conditionals_install_exactly_one_named_factory、raw_shared_and_forget_do_not_follow_alias_but_make_and_history_do。提取原键操作、晚绑定别名、条件登记原子性、保留工厂与锁外析构的回归意图，未搬入旧异步目录及跨模块依赖。沿用 01-01 对固定 Goravel 编译型容器接口的核对。

## 实现与适配

- 修正 instance 覆盖工厂的存储问题。实例现在单独保存，临时覆盖不会丢掉工厂。forgetInstance 清除原始键的实例；单例工厂随后可重新构造，不影响已返回 Arc。
- bindIf/singletonIf 及 Rust 命名形式 bindIfNamed/singletonIfNamed 接通。同一个注册表写锁内完成存在性检查与登记，原键的绑定、实例或别名均算存在；不执行被拒工厂，拒绝的捕获对象在锁外释放。
- isShared 检查原键，不跟随别名。resolved 跟随别名，保留工厂成功解析的历史；只有显式实例而无工厂的条目，在清除后不再显示已绑定或已解析。
- forgetInstances 清除现存实例与单例缓存，保留工厂、别名和解析历史。重置采用新的单例状态，仍在使用旧实例的调用保持有效。门面缓存与容器缓存分开管理。
- Rust 方法名保持 Laravel 拼写；命名形式与 Result 适配沿用前序规则。没有以当前同步 make 对异步延迟 Provider 做阻塞桥接；其衔接仍需在后续部分完成设计与验收。

## 实际验收

macOS ARM64，Rust 1.94.0。工作区 57 项常规测试及 3 项 CLI 子进程检查通过；隔离副本 38 项常规测试及 3 项子进程通过。两者 fmt 与严格 Clippy 通过，隔离 locked/offline standalone release 构建通过。

framework/tests/unit/container.rs 现共 13 项回归，本部分新增 5 项，覆盖工厂恢复、别名原键、实例状态、8 线程条件登记竞争，以及拒绝工厂析构重入。Provider 的真实 HTTP/Artisan 回归改用条件绑定并核对 resolved/isShared；包含两应用 5 次真实 TCP 请求及真实进程入口。另有 [58 次 HTTP 和 6 次命令](http-results.json)、[40 次进程](command-results.json)通过预期结果。

三语 container 正文同步。[6 个 Rust 示例](example-results.json)直接从三语页面提取、编译并执行通过。三语站点构建与根路径、/docs/、三语桌面/移动共 12 项浏览器检查通过。

## 范围与复现

[8 个变更文件 tree](changed-tree.txt)，[SHA-256](candidate-sha256.json)。以已提交的 01-01 配套为基础，无需再次夹带其他模块 WIP。git-commit.txt 保存本部分代码提交，构造工具优先从不可变提交恢复，源工作区后续变化不影响已提交阶段的重现。

```sh
python3 .agents/project/audits/01-02a/prepare_candidate.py /tmp/openrust-01-02a-review
cd /tmp/openrust-01-02a-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在项目根目录执行：

```sh
python3 .agents/project/audits/01-02a/verify_examples.py /tmp/openrust-01-02a-review
python3 .agents/project/audits/01-02a/verify_http.py /tmp/openrust-01-02a-review
python3 .agents/project/audits/01-02a/verify_commands.py /tmp/openrust-01-02a-review
```

验收记录保存后，清理本部分独占 target，保留候选源码及 .artifacts/release。范围与清理后空间见 cleanup-results.json。根工作区历史共享缓存和下载缓存保留。下一步仍为 01-02 的 scoped 生命周期与其余待办，不进入 01-03。

## 本地提交与清理

代码提交 `3c3612642e383e340184982d2bd157359f25d8fc`，8 个变更文件，未推送。前序 4 个阶段及本部分的构造工具已统一优先恢复不可变提交，5 组 SHA-256 核对通过。清理独占 target 882,216 KiB，约 0.90 GB；清理后磁盘可用约 66.80 GB。额度最近实测剩余 98%，未触发停止条件。必要发布产物及候选源码已保留。
