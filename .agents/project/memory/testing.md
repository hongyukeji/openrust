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

测试数：`openrust-core` 31 个单元测试（含分页 `PageMeta` 保留、配置目录覆盖、`.env` 选择生产时跳过其余变量、无 HTTP 插件的命令边界），`openrust-http` 49 个集成测试（`tests/router.rs` 33 个、`tests/resource.rs` 16 个），`openrust-server` 5 个集成测试；`openrust` 1 个单元测试。

## 双库集成（M2 起）

```bash
docker compose -f compose.ci.yaml up -d postgres mysql
OPENRUST_TEST_DATABASE_URL=postgres://openrust:openrust@localhost:5432/openrust_test cargo test -p openrust-db --features db-postgres
OPENRUST_TEST_DATABASE_URL=mysql://openrust:openrust@localhost:3306/openrust_test cargo test -p openrust-db --features db-mysql
```

## 模板快照（M1 起）

```bash
cargo run -p openrust-cli -- new /tmp/openrust-smoke
OPENRUST_PATH="$PWD/crates/openrust" cargo test --manifest-path /tmp/openrust-smoke/Cargo.toml --config "patch.crates-io.openrust.path=\"$OPENRUST_PATH\""
```

生成模板使用 crates.io 版本依赖；在发布前进行本地模板集成测试时，以上 Cargo patch 将 `openrust` 指向当前仓库门面 crate。CLI 自身的模板内容由快照测试覆盖。
