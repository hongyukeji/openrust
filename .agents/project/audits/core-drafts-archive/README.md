# 核心骨架设计草稿归档

2026-11-20，按用户“检查，没有用的就归档删除”的要求核对。原 `.agents/project/core/` 的 21 份 Markdown 和 1 份启动文件清单完整保存在 [压缩归档](core-drafts.tar.gz) 中，原目录移除。逐文件大小与 SHA-256 见 [manifest.json](manifest.json)。删除前已逐字节比对压缩包与原文件，共 22 份一致。

## 核对结论

这些资料来自骨架阶段，含“仅声明类型”“命令仍占位”等过时状态。逐份检查了标题、设计范围、目录／接口及待办，并核对下表的现行计划锚点、验收记录、正文和源码路径。现行资料已按功能单元组织，无需继续将旧草稿作为开发入口；此核对不等于重新验收各模块全部行为，也不把旧设计的所有目标认定为已实现。

数据库翻译扩展仍仅为骨架，保留为 [未完成待办](../../todo.md)；旧 localization 草稿中的数据库覆盖和故障降级设计仅供后续核对，未经当前契约验收，不直接升级为正式要求。已实现的通用异步 Loader 接入不能代替具体数据库扩展，见 [03-10 可插拔异步翻译源](../03-10/README.md)。

原目录没有被外部 Markdown 链接直接引用；清点记录中的旧路径是历史盘点文字，保留并补充本次归档说明。归档内部的相对链接保留原文，可能已过时，不作为现行导航。旧版复用路径和其他设计细节完整留在压缩包中，需要时先核对当前规范与实现。

## 现行入口对照

表中源码路径相对 `vendor/openrust/framework/src/`。计划及验收链接仅用于定位对应单元，实际完成范围以各自记录为准。

| 原文件 | 现行计划／验收 | 正文示例 | 源码入口 |
| --- | --- | --- | --- |
| `artisan.md` | [00-03](../../plan.md#unit-00-03)（[验收](../00-03/README.md)）、[06-08](../../plan.md#unit-06-08)（[验收](../06-08/README.md)） | [digging-deeper/artisan.md](../../../../docs/zh_CN/digging-deeper/artisan.md) | `console` |
| `auth.md` | [05-08](../../plan.md#unit-05-08)（[验收](../05-08/README.md)）、[05-09](../../plan.md#unit-05-09)（[验收](../05-09/README.md)） | [security/authentication.md](../../../../docs/zh_CN/security/authentication.md) | `auth` |
| `broadcasting-websocket.md` | [08-01](../../plan.md#unit-08-01)（[验收](../08-01/README.md)）、[08-02](../../plan.md#unit-08-02)（[验收](../08-02/README.md)） | [digging-deeper/broadcasting.md](../../../../docs/zh_CN/digging-deeper/broadcasting.md) | `broadcasting` |
| `cache-redis.md` | [03-06](../../plan.md#unit-03-06)（[验收](../03-06/README.md)）、[03-09](../../plan.md#unit-03-09)（[验收](../03-09/README.md)） | [digging-deeper/cache.md](../../../../docs/zh_CN/digging-deeper/cache.md) | `cache` |
| `collections.md` | [02-08](../../plan.md#unit-02-08)（[验收](../02-08/README.md)）、[02-09](../../plan.md#unit-02-09)（[验收](../02-09/README.md)） | [digging-deeper/collections.md](../../../../docs/zh_CN/digging-deeper/collections.md) | `support/collection.rs` |
| `database.md` | [02-01](../../plan.md#unit-02-01)（[验收](../02-01/README.md)）、[02-02](../../plan.md#unit-02-02)（[验收](../02-02/README.md)）、[02-03](../../plan.md#unit-02-03)（[验收](../02-03/README.md)）、[02-04](../../plan.md#unit-02-04)（[验收](../02-04/README.md)）、[02-05](../../plan.md#unit-02-05)（[验收](../02-05/README.md)）、[02-06](../../plan.md#unit-02-06)（[验收](../02-06/README.md)）、[02-07](../../plan.md#unit-02-07)（[验收](../02-07/README.md)） | [database/getting-started.md](../../../../docs/zh_CN/database/getting-started.md) | `database` |
| `eloquent.md` | [05-01](../../plan.md#unit-05-01)（[验收](../05-01/README.md)）、[05-02](../../plan.md#unit-05-02)（[验收](../05-02/README.md)）、[05-03](../../plan.md#unit-05-03)（[验收](../05-03/README.md)）、[05-04](../../plan.md#unit-05-04)（[验收](../05-04/README.md)）、[05-05](../../plan.md#unit-05-05)（[验收](../05-05/README.md)） | [eloquent/getting-started.md](../../../../docs/zh_CN/eloquent/getting-started.md) | `database/eloquent` |
| `encryption-hashing.md` | [03-04](../../plan.md#unit-03-04)（[验收](../03-04/README.md)）、[03-05](../../plan.md#unit-03-05)（[验收](../03-05/README.md)） | [security/encryption.md](../../../../docs/zh_CN/security/encryption.md) | `encryption` |
| `events.md` | [06-02](../../plan.md#unit-06-02)（[验收](../06-02/README.md)） | [digging-deeper/events.md](../../../../docs/zh_CN/digging-deeper/events.md) | `events` |
| `filesystem.md` | [03-01](../../plan.md#unit-03-01)（[验收](../03-01/README.md)）、[03-03](../../plan.md#unit-03-03)（[验收](../03-03/README.md)）、[04-09](../../plan.md#unit-04-09)（[验收](../04-09/README.md)） | [digging-deeper/filesystem.md](../../../../docs/zh_CN/digging-deeper/filesystem.md) | `filesystem` |
| `localization.md` | [03-10](../../plan.md#unit-03-10)（[验收](../03-10/README.md)）、[06-08](../../plan.md#unit-06-08)（[验收](../06-08/README.md)） | [digging-deeper/localization.md](../../../../docs/zh_CN/digging-deeper/localization.md) | `translation` |
| `logging-exceptions.md` | [01-03](../../plan.md#unit-01-03)（[验收](../01-03/README.md)）、[01-04](../../plan.md#unit-01-04)（[验收](../01-04/README.md)）、[01-05](../../plan.md#unit-01-05)（[验收](../01-05/README.md)）、[01-06](../../plan.md#unit-01-06)（[验收](../01-06/README.md)）、[01-07](../../plan.md#unit-01-07)（[验收](../01-07/README.md)） | [the-basics/errors.md](../../../../docs/zh_CN/the-basics/errors.md) | `foundation/exceptions` |
| `mail-notifications.md` | [06-06](../../plan.md#unit-06-06)（[验收](../06-06/README.md)） | [digging-deeper/mail.md](../../../../docs/zh_CN/digging-deeper/mail.md) | `mail` |
| `pagination-api-resources.md` | [05-06](../../plan.md#unit-05-06)（[验收](../05-06/README.md)）、[05-07](../../plan.md#unit-05-07)（[验收](../05-07/README.md)） | [eloquent/resources.md](../../../../docs/zh_CN/eloquent/resources.md) | `pagination` |
| `password-reset-email-verification.md` | [06-07](../../plan.md#unit-06-07)（[验收](../06-07/README.md)） | [security/passwords.md](../../../../docs/zh_CN/security/passwords.md) | `auth/passwords` |
| `queue-scheduling.md` | [06-03](../../plan.md#unit-06-03)（[验收](../06-03/README.md)）、[06-04](../../plan.md#unit-06-04)（[验收](../06-04/README.md)）、[06-05](../../plan.md#unit-06-05)（[验收](../06-05/README.md)） | [digging-deeper/queues.md](../../../../docs/zh_CN/digging-deeper/queues.md) | `queue` |
| `rate-limiting.md` | [04-07](../../plan.md#unit-04-07)（[验收](../04-07/README.md)）、[04-08](../../plan.md#unit-04-08)（[验收](../04-08/README.md)） | [digging-deeper/rate-limiting.md](../../../../docs/zh_CN/digging-deeper/rate-limiting.md) | `cache` |
| `session-cookie-csrf.md` | [04-04](../../plan.md#unit-04-04)（[验收](../04-04/README.md)） | [the-basics/session.md](../../../../docs/zh_CN/the-basics/session.md) | `session` |
| `url-generation.md` | [06-01](../../plan.md#unit-06-01)（[验收](../06-01/README.md)） | [the-basics/urls.md](../../../../docs/zh_CN/the-basics/urls.md) | `routing` |
| `validation.md` | [04-05](../../plan.md#unit-04-05)（[验收](../04-05/README.md)）、[04-06](../../plan.md#unit-04-06)（[验收](../04-06/README.md)） | [the-basics/validation.md](../../../../docs/zh_CN/the-basics/validation.md) | `validation` |
| `views-templates.md` | [03-07](../../plan.md#unit-03-07)（[验收](../03-07/README.md)）、[03-08](../../plan.md#unit-03-08)（[验收](../03-08/README.md)） | [the-basics/views.md](../../../../docs/zh_CN/the-basics/views.md) | `view` |
| `startup-files.txt` | [当前启动架构](../../architecture.md)、[00-01 验收](../00-01/README.md) | [请求生命周期](../../../../docs/zh_CN/architecture-concepts/lifecycle.md) | `main.rs`／`bootstrap/app.rs`（根应用） |

## 验证与恢复

- 校验归档文件数、逐文件字节及 SHA-256；核对上表的计划锚点、验收记录、正文、源码路径。
- 本批仅整理 Agent 文档，不改运行代码，不重复运行 Rust 编译或数据库测试。
- 如需追溯，从压缩包单独读取所需文件；不要整体恢复为现行指引。压缩包内路径为 `core/<原文件名>`。
