# 测试基线

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：当前可复验的测试命令。代码尚未开始，以下为 M0 起生效的约定。

## 本地

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
```

## 双库集成（M2 起）

```bash
docker compose -f compose.ci.yaml up -d postgres mysql
OPENRUST_TEST_DATABASE_URL=postgres://openrust:openrust@localhost:5432/openrust_test cargo test -p openrust-db --features db-postgres
OPENRUST_TEST_DATABASE_URL=mysql://openrust:openrust@localhost:3306/openrust_test cargo test -p openrust-db --features db-mysql
```

## 模板快照（M1 起）

```bash
cargo run -p openrust-cli -- new /tmp/openrust-smoke --db postgres && cargo test --manifest-path /tmp/openrust-smoke/Cargo.toml
```

每个里程碑把新增的测试入口写在这里，替换过期命令。
