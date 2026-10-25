# 测试基线

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：当前可复验的测试命令。

## 本地（当前全部通过）

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check            # 本机 cargo-deny 0.20：advisories / bans / licenses / sources 全部 ok
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

当前覆盖：`openrust-core` 16 个单元测试（含配置默认值、部分 TOML 与环境变量数组），`openrust-http` 33 个集成测试（`tests/router.rs` 19 个、`tests/resource.rs` 14 个）。

## ADR-006 旧名扫描（任意改动）

除列出的豁免文件外，命中数应为 0。URL 中的 `https:` 等误报，需逐条确认后忽略。

```bash
rg -n --hidden -g '!target' -g '!.git' -g '!Cargo.lock' \
  -g '!.agents/project/decisions/adr-006-rust-native-naming.md' -g '!.agents/project/conventions.md' \
  -g '!CHANGELOG.md' -g '!docs/**' \
  -e 'Action::(Index|Show|Store|Destroy)' -e '\.(index|show|store|destroy)\(' \
  -e 'ServiceProvider' -e '[A-Z][a-z]+Provider\b' -e 'ServiceNotRegistered' \
  -e '\b[a-z]+:(show|work|seed|list|run|generate|failed|retry|reset|create|status)\b' -e 'make:' \
  -e '\bAPP_ENV\b' -e '\bAPP__' -e 'OPENRUST__' -e 'resources/mail' -e 'Mailable' -e 'Seeder' -e 'Factory' \
  -e '\.group\(' -e 'config/app\.toml' -e '\bGuard\b' -e '\.disk\(' -e 'src/handlers' -e 'src/entities' \
  -i -e 'laravel' -e 'rails'
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
