---
name: implement-crate
description: 按已定 contract 实现或扩展一个 openrust-* crate（新增模块、provider、命令、driver 或测试）。用于 M1-M6 的编码功能簇；不用于修改公共 API 设计或跨 crate 架构决策。
---

# 实现一个 openrust crate

## 读取

1. `.agents/project/core/<crate>.md`（已有）或 crate `README.md`（空壳）的规划 surface。
2. `.agents/project/plans/framework-design.md` 中对应章节，只读本 crate 相关小节。
3. 参考实现：`crates/openrust-core`（契约）与 `crates/openrust-http`（provider、扩展 trait、提取器、测试写法）。

## 实现规则

- 每个能力 crate 的入口是一个 `<Capability>Provider`：`register` 注册服务（`app.provide`）、命令（`app.command`）与扩展钩子（`app.extension_mut::<T>()`）；需要异步初始化放 `boot`。
- 对 `AppBuilder` 的新增方法用扩展 trait `AppBuilder<Capability>Ext`，存入扩展，构建后用 `App::extension::<T>()` 读取。
- 配置用 `app.section::<XConfig>("x")`，`XConfig` 带 `#[serde(default)]` 与手写 `Default`。
- 错误统一进入 `openrust_core::Error`：客户端错误选对应变体，其他用 `Error::internal(source)`；不新增对外错误类型。
- driver 命名 `<Backend><Capability>`，放 `drivers/`；fake driver 放 `testing.rs` 并由 `test-util` feature 暴露。
- 可选后端用 feature（`db-postgres`、`queue-redis` …），feature 只做加法；新依赖写入 `stack.md` 并说明理由。
- 库代码无 `unwrap` / `expect` / `panic` / `todo` / `unimplemented`；公共项有文档，crate 入口类型带 doctest。

## 验证

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <crate>
RUSTDOCFLAGS="-D warnings" cargo doc -p <crate> --no-deps
```

数据相关 crate 另按 `.agents/project/memory/testing.md` 在 PostgreSQL 与 MySQL 各跑一次。集成测试用 `AppBuilder::new(Default::default(), figment::Figment::new())` + `finalize` 构建应用，HTTP 用 `tower::ServiceExt::oneshot`，不开真实端口。

## 收口

更新 `.agents/project/core/<crate>.md`（当前实现 / 后续边界 / 验证）、crate `README.md`、`todo.md`（删除完成项）；按 `conventions.md` 提交。发现 contract 本身有问题：不改设计，写入 `todo.md` 并在交付说明中标注。
