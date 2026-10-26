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

在仓库根目录检查默认构建目录（macOS/Linux；尚未构建时目录可能不存在）：

```bash
du -sh target
du -sh target/debug/incremental target/debug/deps target/debug/build
df -h .
```

使用 `CARGO_TARGET_DIR`、`--target-dir` 或独立 build-dir 时，检查对应实际路径及其所在磁盘。一次任务中的多轮编译复用缓存，整组验证完成后再收尾：

- 清理本任务为验证创建的临时源码副本、临时生成应用及其专属 target；正式业务应用不属于清理对象。失败或取消也须回收临时产物，诊断需暂留时说明原因。
- 无构建占用且取得兼容构建锁后清理可再生的 `incremental/`，保留正常二进制、依赖及关联调试产物；下次修改源码后的编译可能变慢，macOS 源级调试也可能需要重建。
- 保留的依赖产物若持续增长，应检查工具链、feature 与历史包变更，明确清理范围后再处理。需要扩大清理范围时先用 `cargo clean --dry-run` 预览；不把全量 `cargo clean` 设为每次编译后的默认动作，也不按哈希或日期猜测删除 `deps`。

删除前须遵守[构建产物与临时资源规范](.agents/project/conventions.md#构建产物与临时资源)中的路径、归属及并发检查。空间收尾属于人工开发与 Agent 的共同责任；当前没有后台自动清理任务。

## 提交

使用仓库的[提交脚本](.agents/project/scripts/commit)，采用约定的提交类型和中文主题：

```bash
AI_AGENT_SOURCE="<agent> <model>" ./.agents/project/scripts/commit <type> "<中文主题>" "<说明>" -- <pathspec>...
./.agents/project/scripts/commit validate HEAD
```

每次提交只包含一个功能变更。允许的提交类型和署名规则见 [conventions.md](.agents/project/conventions.md) 中的 Git 章节。

## 许可证

Contributions are dual licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option. Unless you explicitly state otherwise, an intentional contribution is submitted under those terms without additional conditions.
