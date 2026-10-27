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

**开发者与 AI Agent 必须在每次构建前、每条构建/测试命令后、长验证阶段边界和功能簇结束时检查空间。** `target/` 同时保存程序、依赖、测试和增量缓存；不同 feature、工具链及历史包名的构建会累积，不能依赖 Cargo 自动回收。

每个功能簇结束且确认没有构建进程或目录占用后，必须对本仓库 target 执行 `cargo clean --dry-run` 复核，再执行 `cargo clean`，清空编译产物；同时删除该簇临时验证目录和临时生成项目/示例的 target。target 只在同一功能簇的多轮验证间复用，不能带入下一簇。每次清理后记录回收量、仓库所有 target 总量和所在磁盘可用空间。`cargo clean` 不清理 Cargo 下载缓存。

仓库内全部 `target/` 合计软上限 20 GiB、硬上限 30 GiB。所在磁盘可用空间低于总容量的 10% 是软告警，低于 5% 是硬告警。软告警时停止扩展验证矩阵，结束当前命令后清理本任务临时产物和本仓库 target，再评估能否运行下一条必要的定向门禁；硬告警时不启动任何新构建，先停止构建、确认无并发使用者并清理本仓库 target，只有磁盘余量恢复到 5% 以上才继续。若清理本仓库产物后仍在告警区间，暂停构建并报告，不删除仓库外或归属不明的文件。功能簇结束后仍必须清理本仓库 target，不能把 20/30 GiB 当作可长期占用额度。

在仓库根目录检查默认构建目录（macOS/Linux；尚未构建时目录可能不存在）：

```bash
find . -type d -name target -prune -exec du -sh {} +
df -h .
```

使用 `CARGO_TARGET_DIR`、`--target-dir` 或独立 build-dir 时，检查对应实际路径及其所在磁盘。一次任务中的多轮编译复用缓存，整组验证完成后再收尾：

- 清理本任务为验证创建的临时源码副本、临时生成应用及其专属 target；正式业务应用不属于清理对象。失败或取消也须回收临时产物，诊断需暂留时说明原因。
- 同一功能簇内复用缓存。验证结束后无构建占用并取得兼容构建锁时，默认用 `cargo clean --dry-run` 预览后对本仓库 target 执行完整清理；软告警时不得暂留任何仓库编译产物，硬告警时暂停大型矩阵。下一次构建会重建删除的文件；清理操作不影响 Cargo 下载缓存。
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
