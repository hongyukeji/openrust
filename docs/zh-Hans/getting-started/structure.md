# 目录结构

## 应用目录

根目录的 `main.rs` 连接 Tokio 和 `bootstrap::create_application`。`Cargo.toml` 与 `Cargo.lock` 声明应用、工作区和锁定依赖。应用代码位于 `app/`，编译型配置加载器位于 `config/`，Web 和控制台路由声明位于 `routes/`。

框架源码位于 `vendor/openrust/framework/`。视图位于 `resources/views/`，网站文件位于 `public/`，公共存储磁盘位于 `storage/app/public/`。测试放在根应用或所属 crate 的 `tests/` 目录。

## 模型、持久化与服务

`app/models/` 保存模型映射、属性转换、关系和模型自身行为。生成的表名与列名常量放在模型顶部附近，数据库结构读取与源码生成由框架负责。按业务需要，将应用专用查询放在 `app/repositories/`，业务流程放在 `app/services/`。模型保留 Eloquent 查询和保存接口；仓储实现持久化，服务协调业务规则与事务。

## Bootstrap 目录

应用的 `bootstrap/` 目录存放构造应用所需的装配声明。

`bootstrap/app.rs` 声明配置、路由、中间件和命令入口。`bootstrap/mod.rs` 向二进制入口导出 `bootstrap::create_application`。`bootstrap/providers.rs` 列出应用的[服务提供者](../architecture-concepts/providers.md#注册服务提供者)。

装配入口通过 `with_routing` 声明 Web 路由、控制台路由和健康路径。Rust 用 `Routing` Builder 表达这些选项，并通过 `with_config`、`with_providers` 和 `with_commands` 注册编译进应用的配置、Provider 和命令。

框架使用这些声明的顺序见[请求生命周期](../architecture-concepts/lifecycle.md)。
## 应用构建钩子

`Application::configure` 支持 `with_bindings::<T, _>`、`with_singletons::<T, _>`、`with_scoped_singletons::<T, _>` 工厂。按 Rust 类型逐项链式注册，工厂返回 `Result<Arc<T>, BindingResolutionError>`。普通绑定每次解析，单例属于应用，作用域单例属于当前请求或队列任务。

`registered`、`booting`、`booted` 围绕 Provider 启动依次执行，成功启动后重复 boot 不再执行。钩子失败会停止后续阶段并返回启动错误。`with_schedule` 延迟登记调度配置，不在应用启动时执行任务。`prefers_json_responses(true)` 在 booted 阶段启用宽泛 Accept 请求头的 JSON 偏好，显式异常 JSON 策略仍优先。
