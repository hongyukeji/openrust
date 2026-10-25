# openrust-cli

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：全局 `openrust` 命令提供项目生成、开发启动与运行期命令代理。

## 当前实现

- `openrust new <name-or-path>` 生成无数据库应用骨架；`--db postgres|mysql` 增加 backend feature、数据库插件、migration crate、UTC/JSON 列 helper 初始迁移、seed 入口与 `.env.example`。
- `openrust dev` 以 `OPENRUST_ENV=development` 启动当前项目的 `serve` 命令；`openrust serve` 不改环境变量。
- 其他命令转发为当前项目的 `cargo run --quiet -- <args>`，并保留子进程退出码。
- 默认模板快照覆盖 manifest、主入口、应用、路由、基础配置、测试与 `AGENTS.md`；双库模板单测覆盖 backend feature、migration crate、插件和 seed 文件。

## 后续边界

- `generate model` / `generate migration` 属于 M2/M6 交界的后续功能簇。
- `generate` / `g` 和其余生成器属于 M6。
- M1 模板依赖已发布的 `openrust` 版本；仓库内验收未发布源码时通过 Cargo 临时 patch 到本地门面 crate。
