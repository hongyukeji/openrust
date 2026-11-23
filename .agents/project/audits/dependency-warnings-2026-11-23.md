# Artisan 依赖警告修复验收

日期：2026-11-23。范围：用户在 RustRover 终端执行 `./artisan model:fields User` 时出现的依赖编译警告。用户随后授权暂存并本地提交本批修复及验收记录，未授权推送。

## 实际复现

通过 computer-use 在用户已打开的 RustRover「本地」终端执行原命令，未替换成代理终端。目录为 `/Users/Home/Codes/Rust/openrust`，终端实际 `rustc`、`cargo` 均为 1.99.1。修复前重现 rav1e 与 sqlx-sqlite 警告，最终输出“模型字段已同步”，`MODEL_FIELDS_EXIT=0`。因此原问题是构建诊断噪声，不能将其报告为命令失败。

修复后在同一终端执行 `TMPDIR="$PWD/target/tmp/model-fields-sync-20261123/tmp" ./artisan model:fields User`，输出只有正常 INFO，`MODEL_FIELDS_FIXED_EXIT=0`，未输出 warning/error。这一步是用户明确要求的 IDE 原环境复测，沿用用户默认 1.99.1 与根应用缓存；没有把它计为 Rust 1.94.0 隔离验收。其他 Cargo 验收经 build_guard、既有独占缓存与隔离源码运行。

## 根因与修改

- rav1e 0.8.1：`std::*` 经上层模块导入旧 `u16` 模块，`u16::MAX` 命中弃用常量；改为 `<u16>::MAX`。两个借用返回类型显式标注 `'_`，不改变生命周期绑定。
- sqlx-sqlite 0.9.0：日期兼容解析器仍手工构造 time 旧组件。改为 StringLiteral、CalendarYearFullExtendedRange、MonthNumerical、Hour24，保留扩展年份、符号、零填充和可选分隔符。最低 time 调整为 0.3.55，与既有根 Cargo.lock 一致；未升级锁文件或其他驱动。time 0.3.55 声明最低 Rust 1.88.0，实际验收使用 1.94.0。
- 隔离重编译进一步暴露 imgalifier-avif 0.1.0 五处遗留 cfg_attr 引用未声明的 asm feature；删除这些无用的条件 dead_code 豁免。没有添加空 feature、禁用 unexpected_cfgs 或启用缺少链接实现的汇编分支，纯 Rust 解码路径保持不变。
- 三个已有依赖补丁的 README.openrust.md 同步修改范围和维护说明，原版权、许可证与包版本保留。未修改 Artisan 启动脚本、IDE 设置或全局诊断配置。

## 官方依据

- SQLx v0.9.0 原解析器：https://github.com/launchbadge/sqlx/blob/v0.9.0/sqlx-sqlite/src/types/time.rs
- time v0.3.55 旧组件向新组件的官方转换：https://github.com/time-rs/time/blob/v0.3.55/time/src/format_description/component.rs

两份固定版本源码已联网下载到工作流 research 目录，并与本地 registry 的 time 0.3.55 实现核对。旧 Year 默认范围为 Extended，不能换成 Standard。没有新增依赖或平行解析器。本次不改变模型字段生成、查询或 Laravel 对齐契约。

## 验证命令与范围

工作流：`target/tmp/model-fields-sync-20261123/`；隔离源码 `source/`，独占缓存 `cargo-target/`。以下 Cargo 命令均以 `python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 --` 为前缀，清单指向上述隔离源码。

| 命令 | 结果 |
| --- | --- |
| `cargo +1.94.0 fmt --manifest-path <source>/Cargo.toml --all --check` | 通过 |
| `rustfmt +1.94.0 --edition 2021 --check <source>/vendor/sqlx-sqlite/src/types/time.rs` | 通过 |
| `cargo +1.94.0 clippy --manifest-path <source>/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 通过，无 warning/error |
| `cargo +1.94.0 test --manifest-path <source>/Cargo.toml --locked -p openrust-framework --test sqlite_time_compatibility --test sqlite_security --test images` | 31 项通过，无失败／忽略，无 warning/error |
| `cargo +1.94.0 build --manifest-path <source>/Cargo.toml --bin main --locked` | 通过，无 warning/error |
| `env APP_BASE_PATH=/Users/Home/Codes/Rust/openrust <target>/debug/main artisan model:fields User --check` | Rust 1.94 构建产物检查真实 User，通过、退出0 |
| `git diff --check` | 通过 |

新增 SQLite 实际消费者用例覆盖空格／T、可选秒／Z、纳秒、闰日、扩展年份／负年份、正负时区、无时区、绑定往返、NULL、Unix 秒与非法日期拒绝。现有 SQLite 安全测试覆盖实际引擎、WAL 和完整性；28 项图片回归覆盖真实 AVIF 编解码及其他已支持图片格式。

终端证据：`rustrover-before.txt`、`rustrover-after.txt`；隔离验收：`warnings-fmt.log`、`warnings-vendor-fmt.log`、`warnings-clippy.log`、`warnings-regression.log`、`warnings-build.log`、`warnings-artisan-194-check.log`，均位于上述工作流目录。

限制：实际硬件为 macOS aarch64，未重新验收 Windows／Linux／x86_64；未重跑全工作区全部测试或三库完整矩阵。数据库运行时修改仅涉及 SQLite，PostgreSQL 原开发库上的真实 User 生成已在 RustRover 复测通过，无数据库重建或清库。既有开发库操作记录及其他协作者的18语言计划保留。
