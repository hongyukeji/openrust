# 启动架构

| 位置 | 职责 |
|---|---|
| [main.rs](../../main.rs) | 建立 Tokio 运行时，将应用工厂交给 `runApplication`。 |
| [bootstrap/app.rs](../../bootstrap/app.rs) | 确定应用根目录，声明配置、Provider、Artisan 命令、路由、中间件及异常策略。 |
| [bootstrap/providers.rs](../../bootstrap/providers.rs) | 登记应用的服务提供者。 |
| [框架入口](../../vendor/openrust/framework/src/foundation/entry.rs) | 解析参数，选定 HTTP 或 Artisan，再创建应用并返回进程退出码。 |
| [Application](../../vendor/openrust/framework/src/foundation/application.rs) | 通过 `start` 或 `handleCommand` 进入对应内核，复用环境、配置及 Provider 引导。 |

应用入口负责声明注册，框架负责执行生命周期。构造时解析根目录和容器，运行时由所选内核完成引导。HTTP 在监听前完成路由装配，Artisan 在命令分派前完成命令登记。启动顺序、根目录优先级及发布布局见[请求生命周期](../../docs/zh-Hans/architecture-concepts/lifecycle.md)。部署命令见[部署指南](../../docs/zh-Hans/getting-started/deployment.md)。

启动范围以实际 Cargo 工作区、根应用装配和相应验收为准。最小应用启动的归档复现、运行时文件与测试结果见 [00-01 验收记录](audits/00-01/README.md)。各批次依赖见[交付计划](plan.md)，上游版本与适配原则见[项目规范](conventions.md)。
