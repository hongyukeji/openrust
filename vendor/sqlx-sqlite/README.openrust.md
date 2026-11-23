# OpenRust dependency patch

Source: published sqlx-sqlite 0.9.0, under MIT OR Apache-2.0. Cargo.toml changes libsqlite3-sys from >=0.30.1,<0.38.0 to =0.39.1 (SQLite 3.54.1), addressing RUSTSEC-2026-0459. SQLx 0.9.1 also widens this bound to include the fixed version; this minimal backport preserves the currently accepted SQLx core, PostgreSQL and patched MySQL driver behavior. Remove this patch when the complete SQLx 0.9.1 driver update is accepted. Both upstream licenses are retained.

Cargo.toml.orig and the original lockfile are upstream references; root Cargo.lock defines the application build.

2026-11-23：将日期兼容解析器迁移到 time 0.3.55 的 StringLiteral、CalendarYearFullExtendedRange、MonthNumerical、Hour24 API；保留旧 Year 的扩展年份范围、零填充、可选符号及原有可选分隔符。最低 time 版本同步为 0.3.55，与根锁文件一致，不升级其他驱动或屏蔽警告。time 0.3.55 声明 Rust 1.88.0，低于项目验收版本 1.94.0。

依据：time v0.3.55 `format_description/component.rs` 中旧组件到新组件的官方转换，及 SQLx v0.9.0 `sqlx-sqlite/src/types/time.rs` 的兼容格式。验收位于框架 tests/sqlite_time_compatibility.rs，覆盖真实 SQLite 类型编解码及失败输入。上游完整升级时同时核对安全补丁与日期兼容性，再移除本地接线。
