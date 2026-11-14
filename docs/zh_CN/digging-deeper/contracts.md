# 契约

契约独立于具体实现定义服务接口。Rust 中使用 `openrust::contracts` 下的 trait，接口服务在容器中使用 `Arc<dyn Trait>` 绑定。

| 职责 | 导入路径 |
| --- | --- |
| 服务绑定与解析 | `openrust::contracts::container::Container` |
| 容器错误 | `openrust::contracts::container::BindingResolutionError` |
| 应用服务与引导状态 | `openrust::contracts::foundation::Application` |
| 配置仓库 | `openrust::contracts::config::Repository` |

通过 `use …::Container as _;` 导入契约方法，避免遮蔽 `openrust::container::Container`。在 Provider 中注册实现，调用方解析 trait，而不是依赖具体类型。参阅[容器绑定](../architecture-concepts/container.md#绑定)和 [Provider](../architecture-concepts/providers.md)。

Rust trait 实现替代 PHP 接口声明。没有自动运行时反射，需要在工厂中声明接口绑定及构造依赖。本页覆盖启动和容器基线使用的契约，其他契约文件可能仍是骨架，随所属模块验收。
