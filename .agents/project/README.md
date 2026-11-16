# OpenRust 项目指引

## 开发要点

- 以 Laravel 为准、参考 Goravel 实现。按[固定 Laravel 基线](plans/laravel-rebuild-baseline.md)核对目录、公开接口和可观察行为。应用入口、装配及运行方式参考[固定 Goravel 版本](conventions.md#上游参考)，用实际测试验证 Rust 实现的等价行为。见[移植原则](conventions.md#laravel-到-rust-的移植原则)。
- 每个功能簇实现前，先检查 `openrust-v0.x` 的对应实现和测试，提取符合当前契约的代码和回归用例。见[旧版实现复用](conventions.md#旧版实现复用)。
- 先按正式版确定目录、命名空间、公开名称、Artisan 契约和文档入口，再按交付计划的依赖顺序实现。分别记录骨架定稿、实现完成和验收通过。见[骨架与开发顺序](conventions.md#骨架与开发顺序)。
- 应用代码放在根应用的对应目录，框架源码放在 `vendor/openrust/framework/`。测试代码单独放在所属应用或 crate 的 `tests/` 目录。见[代码与测试目录](conventions.md#代码与测试目录)。

## 阅读路径

- [项目规范](conventions.md)提供完整的开发、文档与验收规则。
- [交付计划](plan.md)列出功能簇、依赖顺序和验收条件。
- [当前交接](current.md)记录验收结果、开发停点和恢复条件。
