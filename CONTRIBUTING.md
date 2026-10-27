# 参与 OpenRust 贡献

OpenRust 目前处于 pre-alpha 开发阶段。修改代码前，请阅读 [AGENTS.md](AGENTS.md) 了解仓库工作流程，阅读 [.agents/project/conventions.md](.agents/project/conventions.md) 了解代码与 Git 约定，并查看相关 crate README 和 `.agents/project/` 当前事实以确认契约。

## 构建与验证

安装与 workspace `rust-version` 兼容的 Rust 工具链，然后运行：

```bash
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
```

五项主要门禁是 `fmt`、`clippy`、`test`、`doc` 和 `deny`；`cargo check` 用于快速检查构建。数据相关变更还必须按照 [.agents/project/memory/testing.md](.agents/project/memory/testing.md) 执行 PostgreSQL 和 MySQL 集成测试。

## 构建空间管理

**开发者与 AI Agent 都须在整组验证结束后检查空间并清理临时产物。** `target/` 同时保存程序、依赖、测试和增量缓存；不同 feature、工具链及历史包名的构建会累积，不能依赖 Cargo 自动回收。

仓库内全部 `target/` 合计软上限 20 GiB、硬上限 40 GiB。所在磁盘可用空间低于总容量的 20% 是软告警，低于 10% 是硬告警。任一软告警触发后，清理任务临时目录和 `incremental/`，并在开始下一功能簇前把仓库 target 总量降至 20 GiB 内；任一硬告警触发后，暂停新增大型构建矩阵，检查并清理本仓库中体积最大的完整 target，直到低于 40 GiB 且磁盘可用空间恢复至 10% 以上，再继续按软告警控制 target 和增量缓存。若仓库构建产物已清空仍未恢复软阈值，不删除仓库外或归属不明的文件；暂停大型构建并报告磁盘状态。不要等磁盘耗尽；满足条件时可清理完整 `target/`，不要求只删增量缓存。

在仓库根目录检查默认构建目录（macOS/Linux；尚未构建时目录可能不存在）：

```bash
find . -type d -name target -prune -exec du -sh {} +
df -h .
```

使用 `CARGO_TARGET_DIR`、`--target-dir` 或独立 build-dir 时，检查对应实际路径及其所在磁盘。一次任务中的多轮编译复用缓存，整组验证完成后再收尾：

- 清理本任务为验证创建的临时源码副本、临时生成应用及其专属 target；正式业务应用不属于清理对象。失败或取消也须回收临时产物，诊断需暂留时说明原因。
- 同一功能簇复用缓存，验证结束后无构建占用并取得兼容构建锁时清理 `incremental/`；达到上文软/硬阈值时，按实际 target 归属使用 `cargo clean --dry-run` 预览后执行完整清理。下一次构建会重建删除的文件，增量缓存清理也可能降低后续编译速度。
- 清理前检查 Cargo、rustc、rustdoc 进程、构建锁和目标实际路径。只对本仓库或本任务明确创建的 target 清理；不要按哈希或日期猜测删除 `deps`，不要更改全局 Cargo 配置或 profile。

删除前须遵守[构建产物与临时资源规范](.agents/project/conventions.md#构建产物与临时资源)中的路径、归属及并发检查。空间收尾属于人工开发与 Agent 的共同责任；当前没有后台自动清理任务，因此每个功能簇必须记录 target 总体积和磁盘剩余空间，不能把“Cargo 会自动回收”作为保留无限增长产物的理由。

## 提交

使用仓库的[提交脚本](.agents/project/scripts/commit)，采用约定的提交类型和中文主题：

```bash
AI_AGENT_SOURCE="<agent> <model>" ./.agents/project/scripts/commit <type> "<中文主题>" "<说明>" -- <pathspec>...
./.agents/project/scripts/commit validate HEAD
```

每次提交只包含一个功能变更。允许的提交类型和署名规则见 [conventions.md](.agents/project/conventions.md) 中的 Git 章节。

## 许可证

Contributions are dual licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option. Unless you explicitly state otherwise, an intentional contribution is submitted under those terms without additional conditions.
