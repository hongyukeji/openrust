# 正式版开发路线图

| 阶段 | 交付目标 |
| --- | --- |
| [00 启动与配置](plan.md#unit-00-01) | 接通应用装配、默认 HTTP 和 Artisan 入口，形成可独立启动和部署的根应用。 |
| [01 容器、日志与异常](plan.md#unit-01-01) | 建立容器、Provider、Facade 和 Contracts 的生命周期，接通日志、Context 与异常处理。 |
| [02 数据库与集合](plan.md#unit-02-01) | 交付连接、事务、查询、Schema、迁移与填充，建立 Eloquent 和分页所需的集合基础。 |
| [03 应用基础服务](plan.md#unit-03-01) | 交付文件系统、加密与哈希、缓存与 Redis、国际化、视图和模板。 |
| [04 HTTP 请求链](plan.md#unit-04-01) | 交付路由、请求、响应、控制器和中间件，接通 Cookie、Session、CSRF、验证及限流。 |
| [05 Eloquent、认证与 API](plan.md#unit-05-01) | 交付模型、关系、转换、工厂、分页与 API Resources，接通 Guard 和授权。 |
| [06 URL、后台任务与通知](plan.md#unit-06-01) | 交付 URL 与签名、事件、队列、调度、邮件和通知，接通密码重置、邮箱验证与 Artisan 交互。 |
| [07 框架专题](plan.md#unit-07-01) | 交付 Helpers、Strings、HTTP 客户端、进程、并发、图像、搜索与包开发能力。 |
| [08 框架广播、扩展与发布](plan.md#unit-08-01) | 框架广播在授权、事件和队列就绪后接入。Reverb、gRPC 和发布验收按各自的单元跟踪。 |

测试能力随各阶段交付，见[测试单元](plan.md#unit-t-01)。阶段内的前置依赖、文档路径和验证条件见[分批交付计划](plan.md)。

## 核心、扩展与发布

- 核心范围包括 00–07、[08-01 框架广播](plan.md#unit-08-01)和 T 测试能力。闭合状态以[核心功能闭合对账](audits/core-closure-ledger.md)为准。
- [08-02 Reverb](plan.md#unit-08-02)、[08-03 gRPC](plan.md#unit-08-03)和 [Sanctum](plan.md#unit-sanctum-01)单独跟踪。其余生态范围见[文档与交付对照](plans/laravel-docs-delivery-map.md)。
- [08-04 发布验收](plan.md#unit-08-04)覆盖安装、部署、升级、目标平台、兼容数据库与外部驱动。
