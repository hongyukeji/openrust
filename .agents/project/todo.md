# 项目待办

- [ ] **数据库翻译扩展**。`vendor/openrust/translation-database/` 仅有可编译骨架；提交 `c9791297c` 是为满足 workspace 通配符而补齐包清单，不代表功能交付。后续须核对现行异步 Loader 契约与旧版可复用实现，再实现数据库词条加载、显式装配和行为验收；覆盖／回退、来源故障、语言与命名空间及应用隔离均须明确设计并验证。旧设计仅供参考，见 [骨架草稿归档](audits/core-drafts-archive/README.md)；现有通用 Loader 能力见 [03-10 验收](audits/03-10/README.md#可插拔异步翻译源)，不能据此将具体数据库扩展标记完成。

- [x] **06-08 Artisan 交互本地化**。命令列表、帮助、参数错误与生产确认复用应用翻译器。默认使用 `APP_LOCALE`，应用翻译缺失时按 `APP_FALLBACK_LOCALE` 回退。命令名、选项、退出码与机器可读输出结构沿用现有契约。 [基础验收](audits/06-08/README.md) · [生产确认验收](audits/06-08/production-confirmation.md)
