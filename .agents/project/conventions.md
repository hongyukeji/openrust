# 工程规范

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 框架代码、测试、文档与 Git 提交规范。

## 代码

- Edition 2024；MSRV 写入 workspace `rust-version`，为当前 stable 减两个小版本，CI 单独验证。
- 所有 crate `#![forbid(unsafe_code)]`；库 crate `#![deny(missing_docs)]`，公共入口类型附带可运行 doctest。
- `[workspace.lints]` 启用 `clippy::all` + `clippy::pedantic`；显式 `#[allow]` 必须注明原因。库 crate 拒绝 `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented`；测试与 `openrust-cli` 二进制可放宽 `unwrap_used` / `expect_used`。
- 错误：库 crate 用 `thiserror`，`anyhow` 只出现在二进制边界；所有面向 HTTP 的错误最终进入 `openrust::Error`。
- 异步统一 tokio；trait 中使用原生 `async fn`，需要跨任务时加 `Send` 约束。
- 宏只做派生与属性标注，不改变控制流；宏支持代码放 `__private` 模块并 `#[doc(hidden)]`。
- feature 只做加法；命名 `db-postgres` / `db-mysql` / `queue-redis` / `cache-redis` / `storage-s3` / `mail-smtp` / `templates` / `otel` / `metrics` / `test-util`。
- 不写 `TODO` 注释；活跃任务进 `todo.md`。注释只解释非显然的 WHY、触发边界、失败语义或安全约束。

## 命名

| 对象 | 规则 | 示例 |
| --- | --- | --- |
| crate | `openrust-<capability>` | `openrust-queue` |
| driver 类型 | `<Backend><Capability>` | `RedisCache`、`DatabaseQueue` |
| 配置结构 | `<Capability>Config` | `QueueConfig` |
| 错误类型 | `<Capability>Error` | `QueueError` |
| Provider | `<Capability>Provider` | `DatabaseProvider` |
| 提取器 | 名词或形容词 | `CurrentUser`、`Validated<T>` |
| crate 内部布局 | `lib.rs` / `config.rs` / `error.rs` / `provider.rs` / `drivers/` / `testing.rs` | — |

## 验证

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
cargo semver-checks            # 发布前
```

| 改动范围 | 验证 |
| --- | --- |
| 任意 crate 源码 | 上述命令 + 该 crate 单元测试 |
| `openrust-db` / `openrust-queue` / `openrust-auth` | 另在 PostgreSQL 与 MySQL 各跑集成测试，见 `memory/testing.md` |
| 模板与生成器 | 双库模板快照测试：生成项目后 `cargo check` + `cargo test` |
| 文档-only | `git diff --check` + 相对链接检查 + `rg` 旧路径扫描 |

测试原则：框架是底层通用机制，每个公共行为都应有单元或集成测试；新建测试文件前必须能一句话说清它能发现哪个缺陷。fake driver 的断言放在 `openrust-test`。

## 文档

- 每个 crate 的 README 即 `lib.rs` 文档（`#![doc = include_str!("../README.md")]`）。
- 正式文档在 `docs/`（mdBook，M6 建立），信息架构对齐 Laravel：入门 → 基础 → 深入 → 数据库 → 安全 → 测试 → 部署。
- `.agents/project/` 写法见 [style.md](style.md)。

## Git 提交

提交使用项目脚本：

```bash
AI_AGENT_SOURCE="<Agent 名称> <模型>[ (<档位>)]" ./.agents/project/scripts/commit <type>[(scope)] "<中文主题>" "<说明>" -- <pathspec>...
./.agents/project/scripts/commit validate HEAD
```

- type：`feat` / `fix` / `docs` / `style` / `refactor` / `perf` / `test` / `build` / `ci` / `chore` / `revert`；scope 可选，建议用 crate 短名（`core` / `http` / `db` / `cli`）。
- 主题用中文描述实际变化；正文最后一个非空行必须是 `AI-Agent: <Agent 名称> <模型>[ (<档位>)]`。
- 脚本优先读 `AI_AGENT_SOURCE`，其次 `AI_AGENT`，再次 `AI_AGENT_NAME` + `AI_AGENT_MODEL`（可附 `AI_REASONING_EFFORT` / `CLAUDE_CODE_EFFORT_LEVEL`）。Claude Code 会话中 `AI_AGENT` 可能被运行环境预置为非规范值，提交时显式传 `AI_AGENT_SOURCE`。
- 新增文件先 `git add` 再用 pathspec 提交；一个功能簇一个 commit，不夹带无关改动。
- 不 force push；推送 `main` 前确认工作区干净、验证通过。
