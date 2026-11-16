# OpenRust 项目指引

- 以 Laravel 为准、参考 Goravel 实现。按[固定 Laravel 基线](plans/laravel-rebuild-baseline.md)核对目录、公开接口和可观察行为。应用入口、装配及运行方式参考[固定 Goravel 版本](conventions.md#上游参考)，并用实际测试验证 Rust 实现的等价行为。
- 实现每个功能簇前，先检查 `openrust-v0.x` 的对应实现和测试。按[旧版实现复用规范](conventions.md#旧版实现复用)提取符合当前契约的代码和回归用例。
- 先按正式版定目录、命名空间、公开名称、Artisan 契约和文档入口，再按[交付计划](plan.md)的依赖顺序逐个填充功能簇。按[骨架与开发规范](conventions.md#骨架与开发顺序)分别记录骨架定稿、实现和验收状态。
- 应用代码放在根应用的对应目录，框架源码放在 `vendor/openrust/framework/`。测试代码放在所属应用或 crate 的 `tests/` 目录。
- 按[文档规范](conventions.md#文档规范)维护三语文档，示例随功能验收。
- 按[验收清单](conventions.md#验收清单)运行与改动相符的检查并记录结果。按验收收尾规则直接删除本批确认闲置、可重建的临时产物，跳过系统废纸篓或回收站。
