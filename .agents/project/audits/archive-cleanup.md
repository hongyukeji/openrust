# 旧批次验收归档清理

- 2026-11-15：清理 00-01 至 00-03、01-01 和 01-02a 至 01-02p 的 296 份验收材料副本及归档索引。各批次验收 README 保留原位。已跟踪材料的删除见提交 `131d472e`，原件可从提交 `67d28aab` 的原路径查阅。
- 01-01 的构建缓存清理范围和剩余磁盘空间记录见提交 `67d28aab` 中的 `.agents/project/audits/01-01/cleanup-results.json`。

## 已完成计划与旧草稿清理（2026-11-23）

按用户要求删除7份已完成专项计划，将1份仍有效的扩展复用决定并入既有主题记录；同步活跃引用、当前交接和待办。没有另建压缩包或复制计划全文。清理前逐文件确认与提交 `f80096ea63cea8709b1d3548742998b4b56219da` 一致，原文均可通过 `git show <该提交>:<原路径>` 查阅。

| 原plans文件 | 现行主题记录 |
| --- | --- |
| `database-settings.md` | [README](settings/README.md) |
| `model-field-references.md` | [README](model-fields/README.md) |
| `blade-native-rust-blocks.md` | [native-rust-blocks-2026-11-23](03-08/native-rust-blocks-2026-11-23.md) |
| `locale-bcp47.md` | [locale-bcp47-2026-11-22](03-07/locale-bcp47-2026-11-22.md) |
| `locale-query-and-welcome-switching.md` | [welcome-language-2026-11-22](03-07/welcome-language-2026-11-22.md) |
| `operations-ecosystem.md` | [README](08-04/README.md) |
| `payments.md` | [README](pay/README.md) |
| `extension-reuse-decisions.md` | [extension-design-review](extension-design-review.md) |

另删除 `audits/core-drafts-archive/` 的README、manifest和tar.gz共3份旧骨架副本，均已核对与上述提交逐字一致，可按原路径恢复。原22份草稿无需再保留第二份存档；当时“数据库翻译仅骨架”的说明已经过时，现行结果见[翻译验收](localization-database/README.md)。

保留固定基线、官方文档交付对照、全框架对齐的计划和统一数据，以及所有仍有效的主题验收／许可证材料。统一数据中的历史快照路径不改写；其中已删除计划的路径仅代表当时文件，并非活跃链接。实际环境升级、真实供应商、Windows及其他未验条件仍见[待办](../todo.md)，不因清理认定完成。

本次仅整理文档，检查删除范围、引用、JSON解析及Git差异；不改运行代码，不重复执行Rust／数据库测试，用户随后授权本次归档清理的本地提交，不含推送或发布；用户及协作者在途修改保留。
