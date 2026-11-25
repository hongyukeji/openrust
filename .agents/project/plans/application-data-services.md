# 应用数据访问与服务骨架

## 参考与取舍

- 跟随 [Laravel Eloquent](https://laravel.com/framework/docs/eloquent) 的 Active Record 习惯。OpenRust 的 `Model` 和 `Builder` 已提供查询、查找、创建、更新、删除和分页，模型还提供事务内保存及删除。`v0.x` 的应用骨架也没有通用 Repository／Service 可直接复用。本项目的基础控制器已经采用 Rust trait 作为应用扩展入口。
- [l5-repository](https://github.com/andersao/l5-repository/blob/master/src/Prettus/Repository/Eloquent/BaseRepository.php) 验证了按模型复用 Repository 操作的做法。首版采用这一思路，范围限于无状态的 Eloquent 适配。参考 [Spatie Query Builder](https://github.com/spatie/laravel-query-builder) 从现有 Builder 接续查询的方式，具体筛选留给应用，HTTP 参数由调用方解释。
- [Laravel 容器](https://laravel.com/framework/docs/container) 的依赖注入和 [Jetstream](https://jetstream.laravel.com/concept-overview.html#actions) 的应用可定制 Action 可用于服务接线。业务流程归应用或所属包，通用工作由现有 ORM、数据库事务和容器提供。

## 框架与应用入口

1. 框架 Eloquent 模块提供 `Repository` trait 和 `EloquentRepository<M>`。直接使用默认实现即可调用 `query`、`find`、`find_or_fail`、`create`、单模型 `update`／`delete` 和 `paginate`。定制实现只需声明关联模型，按需覆写 `query` 或单项操作。保留 `Model` 和 `Builder` 的主键、连接、作用域、批量赋值、事件和软删除语义。分页接收已限定范围的查询，更新和删除接收调用方在授权范围内选定的实例。批量写入沿用 Builder。
2. 应用在 `app/models/model.rs`、`app/repositories/repository.rs` 和 `app/services/service.rs` 重导出框架入口。`User` 原有的框架 `Model` 导入便于安装和测试夹具单独载入源码。应用的 `Model` 入口与其使用同一 trait。`UserRepository` 使用默认 CRUD，并补充按邮箱查询和事务内锁定查询。每次调用取得独立查询实例。
3. 框架 `TransactionalService` 以 `Connection::transaction` 提供默认执行入口，连接选择和业务规则由实现者定义。`UserService` 演示构造注入、沿用仓储查询实际使用的连接，以及同一事务内查询和更新。调用方拥有授权和输入校验。Fortify 已发布的 Action 沿用原有入口。
4. 三个目录已接入默认应用模块，并同步三语目录指南。沿用现有 `make:class` 生成普通类型，专用生成命令待具体模板的重复需求得到验证后再核定。

## 验收

- 在独立消费应用中，用正式公开导入组合 Repository 和 Service。通过真实 HTTP 和独立 Artisan 调用同一服务，验证查找、分页、创建、更新、删除，以及无记录、意外错误和事务回滚。
- 分别用 PostgreSQL 18、MySQL 8 和 SQLite 验证表前缀、具名连接、作用域隔离、模型事件、软删除和写入失败路径；包含一个自定义查询及一个非默认主键模型。验证 HTTP 输入只有经过显式校验和字段筛选后才能进入写操作。
- 执行 Rust 1.94.0 格式检查、严格 Clippy、三语文档构建及相关浏览器检查。实际结果见[应用基础入口验收](../audits/model-layering/README.md#应用基础入口验收)。
