# 接收配置的具名工厂读取

固定 Spatie 配置提供验证器、筛选、响应、模型及任务类的公开属性。Rust 通过应用容器注册这五类实现，`WebhookConfig` 提供对应的具名读取入口，运行时解析使用同一入口。响应缺省或为空值时采用 `default`。其余四项由具名装配提供，手动构建的 Processor 仍可直接注入运行时依赖。 [固定 Spatie WebhookConfig](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookConfig.php)

取得的配置保留原快照，工厂对象随当前应用及请求上下文解析。字段读取没有触发容器解析，也没有序列化工厂或改变错误顺序。命名、配置错型、未知实现、错误回传仍由原有类型验证与容器入口处理。

## 验收

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 的真实 HTTP 路由、Artisan、缓存和独立 Worker 回归通过。实际应用自定义验证器工厂通过五个入口核对它收到的配置。包测试另核对 Builder 组合、响应默认、缺项、错型及应用隔离。包、夹具严格 Clippy、相关格式和三语浏览器检查通过。
