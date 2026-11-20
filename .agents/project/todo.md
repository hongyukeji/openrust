# 项目待办

- [x] **数据库翻译扩展**。沿用 Spatie `language_lines` 多语言 JSON 结构，实现显式 DatabaseLoader、文件覆盖和 Lang 原子发布；Rust1.94＋PostgreSQL18 的 8 项行为／HTTP 测试、4 示例、严格全feature Clippy／fmt与三语12＋6浏览器通过。精确语言回退和缺表错误适配见 [验收与调研](audits/translation-database/README.md)。

- [x] **06-08 Artisan 交互本地化**。命令列表、帮助、参数错误与生产确认复用应用翻译器。默认使用 `APP_LOCALE`，应用翻译缺失时按 `APP_FALLBACK_LOCALE` 回退。命令名、选项、退出码与机器可读输出结构沿用现有契约。 [基础验收](audits/06-08/README.md) · [生产确认验收](audits/06-08/production-confirmation.md)
