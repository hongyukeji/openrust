# install:full 第一批（开发中，非完整安装交付）

> 历史记录：用户随后确认统一入口为 `artisan init`，此处命令及阻断状态仅描述当时的实现，不代表当前入口；现状和后续验收见 [init 验收记录](init.md)。不保留 `install:full` 命令或别名。

2026-11-19 用户授权开发，计划见 [完整安装入口](../../plan.md#install-full)。本批不暂存、不提交、不推送，保留 Eloquent、翻译驱动等并行改动。

## 本批实现

- 注册真正的 Artisan list/help、`install:full --dry-run`、`--force`、`--without-migration`，复用通用无交互选项。
- 只读计划检查源码项目入口，展示当前模块边界、缺失的前置、覆盖与迁移策略及默认 Sanctum 守卫；不打印环境变量值、不执行构建或迁移。
- 默认执行和强制执行都在缺少完整安装依赖时返回非零；不写路由、配置、迁移或安装锁，不把部分安装报告为成功。
- 共享源码修改锁使用 `try_lock`，并发冲突立即失败；进程退出／锁释放后可重试。此变更适用于现有单项安装入口，避免仅完整安装有锁而单项操作可穿透。
- 三语 Artisan 文档明确开发中范围与前置阻断。旧版仅作为复用核对来源，未找到可直接迁移的统一完整安装编排器，不搬入旧行为。

## 本机验收

macOS ARM64，Rust 1.94.0，独立 `CARGO_TARGET_DIR=/private/tmp/openrust-install-full-build`，禁用 incremental，dev/test debug=0。

- `cargo +1.94.0 test --locked --test install_full`：4 项通过，真实子进程验证只读、缺失前置非零退出、命令帮助、坏清单、源码锁冲突与释放后单项重试。
- `cargo +1.94.0 test --locked --test startup artisan_installs_optional_routes_without_overwriting_or_duplication -- --exact`：1 项通过。
- `cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings`：通过。
- `cargo +1.94.0 fmt --all --check`：通过。
- `cd docs && npm run build`：三语构建通过；`docs/tests/check_browser.mjs` 在系统 Chrome 验证根路径与 `/docs/`、三语、桌面／手机共 12 项通过。
- 原始 `./artisan` 脚本验证与清理结果见本记录后续补充。

验收日志保存在 `/private/tmp/openrust-install-full-{test,regression,clippy,fmt,docs,browser,script}.log`。未执行数据库迁移：当前命令只读／前置阻断，没有要验证的数据库写入。未运行 Windows 或 Linux，本批不能用 macOS 结果代替跨系统验收。

## 尚未完成

完整执行仍待固定版本安装清单、Passport 完整安装服务、Reverb 完整接线及选定认证扩展前置。随后才接通共享安装步骤、单次重建、原迁移入口、配置合并、强制覆盖、版本升级补全与失败恢复。不能通过检测到包目录／Cargo feature 就认定安装器完成。

当前 `--force` 和 `--without-migration` 仅决定只读计划的策略展示，完整执行被前置检查阻断，尚未验收文件覆盖或生产迁移。默认 `/user` 双认证共存同样仍待集成验收。本批不是 v0.1.0 完整安装能力闭合，后续不能省略真实 MySQL 8.4、根／生成应用及三系统入口验收。

## 脚本及清理结果

原始 `./artisan install:full --dry-run --force` 退出 0；实际二进制 `artisan help install:full` 退出 0，`artisan install:full --force --no-interaction` 退出 1，报告缺失前置。脚本使用本机 stable 工具链，前述测试与 Clippy 使用 1.94.0。

确认无编译或文档预览进程占用后，直接清理本批独占 target、浏览器依赖及文档 dist；按 du 统计移除 5.981 GB，文件系统可用空间实际增加 5.638 GB，清理后可用 82.425 GB（76.764 GiB）。验收日志及记录保留，未操作数据库、Docker 容器、数据卷或共享缓存。未暂存、未提交、未推送。
