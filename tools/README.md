# 仓库工具包

`tools/` 收纳与框架运行时分离、但仍属于同一 Cargo workspace 的开发和维护工具。收纳目录只整理仓库根结构；Cargo package 名称、库/API 名称、功能边界和常用 `cargo -p` 命令保持不变。

- `cli/`：`openrust` 命令行与项目模板。
- `macros/`：过程宏编译边界。
- `testing/`：应用可选的数据库测试辅助 package。
- `performance/`：维护者基准与性能验证，不进入默认 workspace members。

各 package 仍独立维护 manifest 与测试。目录和 workspace 约定见 [ADR-021](../.agents/project/decisions/adr-021-loco-style-module-layout.md) 与 [架构说明](../.agents/project/architecture.md)。
