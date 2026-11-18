# 自定义短信供应商验收

## 契约

以固定 Laravel v13.40.0 的通知 `ChannelManager::extend` 和通用 `Manager::driver` 为依据。OpenRust 通过应用容器注册类型化的 `SmsDriver` 工厂，并以 `Option<&str>` 表达默认或显式选择。`SmsChannel` 通过现有自定义通知渠道注册，应用的派发端和 Worker 共用同一服务提供者。旧版 `v0.x` 的 `vendor/openrust/framework/src/sms.rs` 提供了驱动抽象与国际号码校验的参考。主线复用现有 Vonage 传输，保留 `vonage` 渠道及其既有负载。 [固定通知管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/ChannelManager.php) · [固定通用管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Manager.php)

## 实际结果

- 用 Rust 1.94.0 执行 `cargo build --locked --example sms`，构建[示例应用](../../../../examples/sms/main.rs)。通过[验收入口](../../../../tests/sms_example.py)运行示例二进制的 `artisan sms:demo`、`artisan migrate --force` 和 `artisan queue:work <连接> --once --tries=1`。
- 独立 MySQL 8.4.13 与 Redis 8.10.3 各通过两次直接发送和四次独立 Worker 投递。逐条读取两种存储的实际队列载荷，短信正文、号码和选定的配置名一致。端点和凭据均未入队。Worker 的默认配置特意设为与任务已选配置相反，实际仍投递到任务选中的模拟供应商，并使用 Worker 专用凭据。
- 现有 Vonage 的 `vonage_protocol_and_worker_retries_use_the_notification_route` 定向回归通过，使用真实 MySQL 及 Redis Worker 和本地协议端点。两家示例供应商也只通过本地 HTTP 验证，未进行真实短信投递。
- `cargo +1.94.0 fmt --all --check` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。三语文档使用 `npm --prefix docs run build` 构建，在 `/` 与 `/docs/` 下通过 12 项通用浏览器检查和 6 项新章节检查。文档的 `queue:work redis --stop-when-empty --tries=1` 实际向 `/alpha`、`/beta`、`/alpha` 依次投递并正常退出。
- 已清理本批临时容器、隔离编译和文档产物，数据卷清单一致。项目磁盘可用量净增 3.45 GB，剩余 85.56 GB。原始日志保留于 `/private/tmp/openrust-sms.8ejsv5mh`。
