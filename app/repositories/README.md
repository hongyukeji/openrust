# 数据访问

把应用专用的查询和持久化放在这里，例如 `user_repository.rs` 中按租户和状态筛选用户。查询可从模型的 Eloquent Builder 起步，写入可使用模型的 `create`、`save` 和 `delete`。需要与其他写入保持原子性时，接收调用方传入的事务。

模型继续负责字段映射、属性转换和关系。[业务流程](../services/README.md)负责业务规则和事务边界。
