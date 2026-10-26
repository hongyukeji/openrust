# 资源查询授权 consumer

此独立 Rust package 演示应用如何使用 `openrust::auth::QueryRule` 与公开实体类型，为自己的数据定义行范围；`visible_documents_in_batches` 展示授权范围逐批保留、主键升序遍历和批次边界取消。SeaORM derive 宏需要应用直接声明 `sea-orm` 依赖。它与根 workspace 分开构建，内部依赖使用相对 path，且禁止发布。

`visible_documents` 把可信身份范围与可选 ID 条件相交。真实应用还须在 handler 中调用 `Policy::authorize` 检查操作权限，并对普通 SeaORM、直接关联加载和原生 SQL 路径自行维护授权条件。

在仓库根目录运行：

```bash
cargo check --manifest-path examples/query-authorization/Cargo.toml
```
