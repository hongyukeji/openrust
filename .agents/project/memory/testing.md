# 测试基线

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：当前可复验的测试命令。

## 本地（当前全部通过）

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p openrust-server
cargo doc --workspace --no-deps
cargo deny check            # 本机 cargo-deny 0.20：advisories / bans / licenses / sources 全部 ok
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

当前覆盖：默认 `always_ok` 与 `semantic` 状态码、成功/错误包装、`request_id` 头正文一致、415/422/413/408/500 兜底、`Location`/`Allow`/`Retry-After`、非法配置及统一包装的 `/health`。

M2：`openrust-db` 有迁移参数与连接配置单测；`tests/connection.rs` 覆盖连接、事务提交/回滚、`SKIP LOCKED`，`tests/migration.rs` 覆盖运行期 migrate up 重复执行、status、down/重放、`db seed` 命令调用，并查询 `information_schema` 验证时间与 JSON 列类型。`openrust-test/tests/database.rs` 在 PostgreSQL 14 / MySQL 8.0 验证每用例独立库、迁移和数据隔离；MySQL 隔离测试用 root admin URL。MySQL 强制 TLS，测试断言非空 TLS cipher 与 `+00:00` 会话时区。MSRV `cargo +1.97.0 check --workspace --all-targets --all-features` 通过。测试服务通过 `compose.ci.yaml` 启动。

`openrust new --db postgres` 与 `--db mysql` 生成项目均通过 `cargo check` 和 `cargo test`；仓库内验收用 Cargo `patch.crates-io.openrust.path` 指向本地 facade。`openrust-cli` 单测覆盖两种模板文件与插件注册。

生成器验收：两种数据库模板均生成 model 与多份 migration 后通过 `cargo check` / `cargo test`。运行生成项目的迁移命令在 PostgreSQL 14 / MySQL 8.0 空数据库验证 up、重复 up、status、down、up，包含 UTC 时间、JSON 与外键列。`openrust-db/tests/migration.rs` 需要专用空数据库 URL；不要复用已有其他迁移历史的数据库。CLI 单测、默认/all-features workspace clippy 与 test、all-features rustdoc、Rust 1.97 MSRV 检查和 `cargo deny check` 均通过。

M3 邮件：`cargo test -p openrust-mail --all-features` 覆盖 fake 发送、SMTP 强制 STARTTLS transport 配置、凭据配对校验和密码 Debug 脱敏；`cargo clippy -p openrust-mail --all-targets --all-features -- -D warnings` 通过。`cargo test -p openrust-server --test server` 验证聚合应用注册 Mailer。修改后的 CLI 无数据库、PostgreSQL、MySQL 模板生成项目均通过 `cargo check` / `cargo test`。

测试数：`openrust-core` 31 个单元测试（含分页 `PageMeta` 保留、配置目录覆盖、`.env` 选择生产时跳过其余变量、无 HTTP 插件的命令边界），`openrust-http` 49 个集成测试（`tests/router.rs` 33 个、`tests/resource.rs` 16 个），`openrust-server` 6 个集成测试（含资源宏全部动作路由映射）；`openrust` 1 个单元测试。

## 双库集成（M2 起）

```bash
docker compose -f compose.ci.yaml up -d --wait postgres mysql
OPENRUST_TEST_DATABASE_URL=postgres://openrust:openrust@localhost:5432/openrust_test cargo test -p openrust-db --features db-postgres
OPENRUST_TEST_DATABASE_URL=mysql://openrust:openrust@localhost:3306/openrust_test cargo test -p openrust-db --features db-mysql
```

`compose.ci.yaml` uses the minimum supported PostgreSQL 14 and MySQL 8.0 images. If those ports are already occupied, set `OPENRUST_TEST_POSTGRES_PORT` / `OPENRUST_TEST_MYSQL_PORT` when starting Compose and use the mapped ports in the URLs. The CI database job uses equivalent service containers.

## 模板快照（M1 起）

```bash
cargo run -p openrust-cli -- new /tmp/openrust-smoke
OPENRUST_PATH="$PWD/crates/openrust" cargo test --manifest-path /tmp/openrust-smoke/Cargo.toml --config "patch.crates-io.openrust.path=\"$OPENRUST_PATH\""
```

生成模板使用 crates.io 版本依赖；在发布前进行本地模板集成测试时，以上 Cargo patch 将 `openrust` 指向当前仓库门面 crate。CLI 自身的模板内容由快照测试覆盖。
