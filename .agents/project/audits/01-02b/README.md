# 01-02 第二部分：作用域绑定

2026-11-11 完成作用域绑定及 HTTP 处理器/Artisan 执行接线验收。01-02 的上下文绑定、扩展/回调及延迟 Provider 仍在推进，本记录不标记整个单元或页面完成。

## 固定基线与复用

对照 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc:src/Illuminate/Container/Container.php` 的 scoped、scopedIf、forgetScopedInstances、原始键实例操作和作用域名称记录。文档对应 `960eedf0911155ad16326ade49c0ea5d18eda0b8:container.md#binding-scoped`。沿用前序固定 Goravel 编译型应用及容器接口核对。

先查旧版 `6d1a8192:vendor/openrust/framework/src/container.rs` 的 root_scoped_reset_and_child_scope_isolation、dropped_scope_releases_cache_without_lifetime_tombstones、scoped_value_drop_can_reenter_container_after_cache_release，以及作用域取消相关用例。复用生命周期隔离、清理、锁外析构的实现思路和回归意图。旧异步工厂、资源关闭队列与显式 Resolver 类型未整包带入；本部分工厂继续同步，Future 负责执行生命周期。

## 实现与 Rust 适配

- scoped/scopedIf、命名形式及 forgetScopedInstances 接通。无显式生命周期时保留根作用域行为；HTTP 处理器、Artisan handle/call 自动进入执行作用域。普通单例仍由应用共享。
- 每个 Tokio 生命周期持有独立实例缓存；同容器嵌套执行复用，不同应用保留各自状态。容器身份由稳定 Arc 分配标识，不使用可复用的容器地址；悬挂嵌套 Future 与同地址替换回归通过。
- 实例覆盖与别名移除保存在当前作用域，原键清理不跨请求。根实例清理、局部清理及共享服务互不混用。重新绑定仍是应用级注册动作，使用绑定版本区分新配方，避免误用旧缓存。
- 清除缓存、实例替换和被替换服务的析构都在缓存/注册表锁外进行。修正根单例与作用域实例覆盖时残留旧缓存引用的问题，保留原工厂供后续重新构造。
- 生命周期 Future 和调用方 Arc 拥有引用。执行结束、失败或取消并释放所有关联拥有者后释放缓存；显式保留服务或嵌套 Future 可以延长引用生命，不强制销毁调用方持有的 Arc。
- PHP 生命周期边界的全局实例清理，转换为 Rust 并发任务的局部缓存，保留同一生命周期共享、下次重新构造的机制。未声称性能提升，没有性能基准。

流式正文可以捕获已解析 Arc 服务。流式/终止回调本身尚待 HTTP 单元接线，本批验证的是处理器和命令执行作用域，不把空的终止钩子视为已实现。门面缓存与容器缓存分离，显式清理容器实例不会自动清掉门面缓存。

## 验收

macOS ARM64、Rust 1.94.0。工作区 74 项常规测试及 4 项 CLI 子进程检查通过；隔离 55 项常规测试及 4 项子进程通过。fmt、严格 Clippy 与 locked/offline standalone release 通过。

新增 scoped 单元回归 14 项、根单例缓存回归 1 项，以及 tests/scoped_lifecycle.rs 的 2 项入口回归。覆盖并发与嵌套、根清理不影响子生命周期、实例覆盖、别名删除、地址复用、重试/panic、取消释放及析构重入。两应用共 4 次真实 HTTP 请求核对同请求共享、跨请求不同及析构计数；同一 Artisan Kernel 连续 3 次命令核对各次独立及结束释放。实际进程入口包含 scope:probe。

[58 次 HTTP 与 6 次命令](http-results.json)、[40 次进程](command-results.json)重验通过。[9 个 Rust 三语示例](example-results.json)提取编译运行通过。三语站点构建及根路径、/docs/、三语桌面/移动共 12 项浏览器检查通过。

## 范围与复现

[12 文件 tree](changed-tree.txt)和 [SHA-256](candidate-sha256.json)。仅接入必要容器、Facade 接线、测试及三语容器正文，其他模块 WIP 保留。

```sh
python3 .agents/project/audits/01-02b/prepare_candidate.py /tmp/openrust-01-02b-review
cd /tmp/openrust-01-02b-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

在项目根目录执行 verify_examples.py、verify_http.py、verify_commands.py，参数均为上述副本目录。构造工具从 git-commit.txt 对应的不可变提交恢复并校验。验收记录保存后，清理本部分独占 target，候选源码与 .artifacts/release 保留；详见 cleanup-results.json。

## 后续

继续 01-02 的上下文绑定、扩展/解析/重绑定回调与延迟 Provider。重绑定阶段增加 CT-CONFIG-REBIND：验证容器替换配置契约后，Application 配置访问和路由装配引用的一致性，不能仅验证容器直接解析。完整流式与终止回调的作用域延续在 HTTP/Console 对应单元闭合。Windows 仍未实机验收。

## 本地提交与清理

代码提交 `21780469f1d54fe1bc9218bc9ad7573a721e3c5f`，12 个变更文件，未推送。不可变提交恢复与 12 文件 SHA-256 验证通过。已确认无 Rust 编译进程使用本阶段目录，清理独占 target 1,229,060 KiB，约 1.26 GB，清理后磁盘可用约 66.44 GB。最近额度剩余 98%，未触发停止条件。候选源码、必要发布产物及共享下载缓存保留。
