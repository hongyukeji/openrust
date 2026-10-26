# M3 数据输出与查询边界 Spec

- 目标：用现有 SeaORM 和响应 DTO 约定交付字段白名单、安全筛选/排序、软删除、scope 与关联加载示例。
- 输出：响应 struct 只声明公开字段并显式从模型映射；敏感字段不序列化，不直接输出 SeaORM Model。
- 过滤：每个 endpoint 使用有类型、`deny_unknown_fields` 的 query DTO；handler 显式映射到 SeaORM `Condition`，不得把用户字段拼接进 SQL。
- 排序：公开实现 ADR-008 `sort=-created_at,title`；新增 `SortParams::parse(value, allowlist)` 拒绝空项、重复项、未知字段和超长列表，调用方再映射到实体 Column。
- 软删除：`generate model/migration --soft-delete` 增加 nullable UTC `deleted_at` 与 `Entity::not_deleted()` 显式 scope；不伪装成 SeaORM 自动全局过滤，恢复通过 ActiveModel 清空字段。
- 主键：默认继续自增整数；`--uuid-id` 使用 UUIDv7，生成 model 与 migration 均须传相同选项，并通过 `db-uuid` feature启用 SeaORM UUID 支持。
- 关联：生成的 references 外键保持显式；README 给出 SeaORM `find_with_related` / `load_many` 预加载示例，说明避免 N+1 及分页边界。
- 覆盖边界：`Db::read_connection()` 仍由调用方明确选择；scope/helper 不改写 raw SQL、不自动注入租户权限或授权条件。
- 验收：解析单测、生成代码 check、生成的 PG18/MySQL8.4 UUID/软删除迁移与插入/删除/恢复真实双库测试，以及 DTO/filter/sort 行为测试。
- 排除：不新增 ORM、通用动态过滤语言、全局软删除魔法、自动 DTO 推导宏或租户产品模型。

后续范围已扩展：[底层补齐](v1-foundation-completion.md) F5/F6 将提供显式受保护的查询规则和租户入口；本 Spec 记录现有普通查询边界，不否定新契约，也不表示原始 SQL 已被自动保护。
