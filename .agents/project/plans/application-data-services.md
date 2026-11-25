# 应用数据访问与服务骨架

## 参考与取舍

- 跟随 [Laravel Eloquent](https://laravel.com/framework/docs/eloquent) 的 Active Record 习惯。OpenRust 的 `Model` 和 `Builder` 已提供查询、查找、创建、更新、删除和分页，模型还提供事务内保存及删除。`v0.x` 的应用骨架也没有通用 Repository／Service 可直接复用。本项目的基础控制器已经采用 Rust trait 作为应用扩展入口。
- [l5-repository](https://github.com/andersao/l5-repository/blob/master/src/Prettus/Repository/Eloquent/BaseRepository.php) 验证了按模型复用 Repository 操作的做法。首版采用这一思路，范围限于无状态的 Eloquent 适配。参考 [Spatie Query Builder](https://github.com/spatie/laravel-query-builder) 从现有 Builder 接续查询的方式，具体筛选留给应用，HTTP 参数由调用方解释。
- [Laravel 容器](https://laravel.com/framework/docs/container) 的依赖注入和 [Jetstream](https://jetstream.laravel.com/concept-overview.html#actions) 的应用可定制 Action 可用于服务接线。业务流程归应用或所属包，通用工作由现有 ORM、数据库事务和容器提供。

## 框架与应用入口

1. 在框架 Eloquent 模块内提供 `EloquentRepository<M>`，以组合方式复用 `Model` 和 `Builder`。首批候选入口为 `query`、`find`、`find_or_fail`、`create`、单模型 `update`／`delete`，以及在调用方已限定范围的查询上分页。批量写入沿用 Builder 自身的入口与语义。保留已有主键、具名连接、全局作用域、批量赋值、类型转换、事件和软删除行为。使用现有事务接口，事务由调用方持有。
2. 在 `app/repositories/repository.rs` 提供可直接采用的应用入口，优先重导出框架实现。应用可在 `user_repository.rs` 等具体类型中组合基础 Repository，加上具名查询或显式查询约束。每次调用取得独立查询实例，避免请求间共享可变 Builder。
3. 用 `app/services/user_service.rs` 或其他具体应用 Service 展示构造注入、校验、事务和跨对象协调。应用侧的公共扩展入口沿用基础控制器的 trait 表达方式，在有可复用方法时提取默认实现。首版 CRUD 放在 Repository；服务直接组合 Repository、模型和现有事务能力。应用可通过 Provider 装配具体服务。Fortify 已发布的 Action 继续使用原有入口。
4. 完成可编译样例后，将两个目录接入应用模块，并同步三语入口。沿用现有 `make:class` 生成普通类型，专用生成命令待具体模板的重复需求得到验证后再核定。

## 验收

- 在独立消费应用中，用正式公开导入组合 Repository 和 Service。通过真实 HTTP 和独立 Artisan 调用同一服务，验证查找、分页、创建、更新、删除，以及无记录、意外错误和事务回滚。
- 分别用 PostgreSQL 18、MySQL 8 和 SQLite 验证表前缀、具名连接、作用域隔离、模型事件、软删除和写入失败路径；包含一个自定义查询及一个非默认主键模型。验证 HTTP 输入只有经过显式校验和字段筛选后才能进入写操作。
- 执行 Rust 1.94.0 格式检查、严格 Clippy、三语文档构建及相关浏览器检查。目录说明与计划属于设计交付，基础接口和示例随实现验证。
