# 目录结构

## 应用目录

根目录的 `main.rs` 连接 Tokio 和 `bootstrap::create_application`。`Cargo.toml` 与 `Cargo.lock` 声明应用、工作区和锁定依赖。应用代码位于 `app/`，编译型配置加载器位于 `config/`，Web 和控制台路由声明位于 `routes/`。

框架源码位于 `vendor/openrust/framework/`。视图位于 `resources/views/`，网站文件位于 `public/`，公共存储磁盘位于 `storage/app/public/`。测试放在根应用或所属 crate 的 `tests/` 目录。

## 模型、持久化与服务

`app/models/model.rs` 提供应用模型使用的 Eloquent `Model` trait。参照 `User` 实现字段映射、属性转换和关系，并按需覆写默认方法。生成的表名和列名常量放在模型顶部附近。

`app/repositories/repository.rs` 提供 `EloquentRepository<M>` 和 `Repository`。用 `EloquentRepository::<M>::new()` 处理常用增删改查。定制时，实现 `Repository` 并声明 `type Model = M`，按需覆写 `query` 或单项操作。`UserRepository` 示范添加具名查询。分页时把已筛选的 Builder 传给 `paginate`。查询条件不会自动写入 `create` 的属性。`update` 和 `delete` 处理调用方传入的模型，调用前应在授权范围内选定记录。

`app/services/service.rs` 提供 `TransactionalService`。实现 `connection` 选择业务使用的连接，再调用 `transaction` 将相关写入放进同一事务。`UserService` 示范构造注入，以及在模型使用的数据库连接上执行事务更新。控制器、命令和任务可以在授权和输入校验后调用同一服务。

## Bootstrap 目录

应用的 `bootstrap/` 目录存放构造应用所需的装配声明。

`bootstrap/app.rs` 声明配置、路由、中间件和命令入口。`bootstrap/mod.rs` 向二进制入口导出 `bootstrap::create_application`。`bootstrap/providers.rs` 列出应用的[服务提供者](../architecture-concepts/providers.md#注册服务提供者)。

装配入口通过 `with_routing` 声明 Web 路由、控制台路由和健康路径。Rust 用 `Routing` Builder 表达这些选项，并通过 `with_config`、`with_providers` 和 `with_commands` 注册编译进应用的配置、Provider 和命令。

框架使用这些声明的顺序见[请求生命周期](../architecture-concepts/lifecycle.md)。
## 应用构建钩子

`Application::configure` 支持 `with_bindings::<T, _>`、`with_singletons::<T, _>`、`with_scoped_singletons::<T, _>` 工厂。按 Rust 类型逐项链式注册，工厂返回 `Result<Arc<T>, BindingResolutionError>`。普通绑定每次解析，单例属于应用，作用域单例属于当前请求或队列任务。

`registered`、`booting`、`booted` 围绕 Provider 启动依次执行，成功启动后重复 boot 不再执行。钩子失败会停止后续阶段并返回启动错误。`with_schedule` 延迟登记调度配置，不在应用启动时执行任务。`prefers_json_responses(true)` 在 booted 阶段启用宽泛 Accept 请求头的 JSON 偏好，显式异常 JSON 策略仍优先。
