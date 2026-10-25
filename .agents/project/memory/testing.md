# 测试基线

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：根 package 模块迁移、双库与生成项目当前可复验入口。

## 本地 workspace 门禁

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
```

按 feature 独立检查框架包，避免只依赖 `--all-features`：

```bash
cargo check -p openrust --no-default-features
cargo check -p openrust --no-default-features --features db-postgres
cargo check -p openrust --no-default-features --features db-mysql
cargo check -p openrust --no-default-features --features auth
cargo check -p openrust --no-default-features --features mail-smtp
cargo check -p openrust --no-default-features --features test-util
cargo check --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

根 package 示例：`cargo run -p openrust --example hello`；应用配置可用 `APP_SERVER__PORT` 指定隔离端口，HTTP smoke 访问 `/` 应返回统一 JSON 200。

## PostgreSQL / MySQL

日常 CI 基线为 PostgreSQL 16、MySQL 8.4 LTS。先检查并复用合适的服务；否则使用项目 Compose。需要隔离端口时：

```bash
OPENRUST_TEST_POSTGRES_PORT=55432 OPENRUST_TEST_MYSQL_PORT=53306 docker compose -p openrust-ci -f compose.ci.yaml up -d --wait postgres mysql
OPENRUST_TEST_DATABASE_URL=postgres://openrust:openrust@localhost:55432/openrust_test cargo test -p openrust-server --features auth,db-postgres --test auth_database --test db_connection --test db_migration
OPENRUST_TEST_DATABASE_URL=mysql://root:root@localhost:53306/openrust_test cargo test -p openrust-server --features auth,db-mysql --test auth_database --test db_connection --test db_migration
```

MySQL `openrust-test` 隔离测试需具备建库/删库权限的管理 URL；一般应用集成测试使用最低权限 `openrust` 用户。MySQL driver 强制 Rustls TLS，并在新连接设置 `+00:00` 时区。两个独立数据库不能替代真实复制拓扑。

2026-10-26 根模块迁移后，上述 DB/auth tests 在 PostgreSQL 16 和 MySQL 8.4 均实际通过（分别 4 项 auth、3 项连接/事务、1 项迁移）。`openrust-test` 数据库隔离测试也分别通过。

## 生成项目

CLI 模板快照测试：`cargo test -p openrust-cli`。生成 PostgreSQL、MySQL `--auth` 项目后运行 `cargo fmt --all --check`、`cargo check`、`cargo test`。仓库当前未发布，集成验证可用 Cargo patch 指向本地根 `openrust` 与 `crates/openrust-test`；这不作为无 patch 的发布安装验收。

2026-10-26 根模块迁移后，两种生成项目都以真实 PostgreSQL 16 / MySQL 8.4 服务运行 `generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 并通过。测试覆盖实际 HTTP、JWT/PAT 认证、数据库流程、可信代理 client IP 与 fake mail。

## 迁移验收记录

- `cargo metadata --no-deps --format-version 1` 确认根 package 为 `openrust`，workspace 共五个成员：`openrust`、`openrust-cli`、`openrust-macros`、`openrust-test`、`openrust-server`。
- 无默认、默认、关键单独 feature、all-features 编译已检查；workspace default/all-features clippy 与 test、warnings-denied rustdoc、`cargo deny check` 均通过。`cargo deny` 仍会输出重复版本依赖提示，但无 advisory、license、ban 或 source 失败；JWT 使用 `aws_lc_rs`，不再解析到受 RUSTSEC-2023-0071 覆盖的 `rsa 0.9`。
- 已实现并验证的范围不包含数据库读写分离复制拓扑、Redis 集群、队列多 worker、跨节点实时通信、S3 或完整集群验收；它们仍按 `todo.md` 与各专题计划推进。
- PostgreSQL 14 / MySQL 8.0 结果只保留为历史兼容性证据，不代表当前日常 CI 基线或最终发布最低版本。v1 发布前确定并验证最低支持版本。
