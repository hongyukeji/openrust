# 短信

统一短信是 OpenRust 借鉴 Laravel [文件存储](filesystem.md)机制提供的扩展。`Storage::disk` 按名称选择由已注册驱动支持的磁盘，`Sms::driver` 同样按名称选择由已注册供应商驱动支持的短信配置。多个配置可以复用同一驱动，并分别设置参数。

## 注册供应商 {#custom-sms-providers}

通过 `Sms::driver(None)` 选择默认配置，或用 `Sms::driver(Some("backup"))` 显式指定。直接发送时调用 `driver.send(SmsMessage::new(number, text)?).await?`。在应用服务提供者的 `register` 中解析 `SmsManager`，用 `extend` 注册每个驱动。工厂接收选中的配置并返回实现 `SmsDriver` 的驱动。`sms.drivers` 下的配置名称可以与注册的驱动名称不同。

用 `ChannelManager::extend` 将 `SmsChannel::default()` 注册为 `sms`，用 `SmsChannel::using("backup")` 注册固定使用备用配置的渠道。在 `Notification::via` 中选择已注册渠道，由 `to_sms` 返回短信正文，并在 `sms` 路由键下用 `NotificationRoute::Sms` 指定号码。派发进程和 Worker 使用相同的服务提供者和渠道注册。现有 `vonage` 渠道仍可使用。

## 配置

以下配置默认选择 `primary`。令牌通过环境变量传入，应用运行时将其加载到对应配置。

```json
{
  "sms": {
    "default": "primary",
    "drivers": {
      "primary": {"driver": "demo-alpha", "endpoint": "http://127.0.0.1:8767/alpha"},
      "backup": {"driver": "demo-beta", "endpoint": "http://127.0.0.1:8767/beta"}
    }
  }
}
```

## 运行示例

从仓库根目录运行完整示例。先在一个终端启动模拟端点。

```sh
python3 -B examples/sms/mock.py
```

再在另一个终端启动独立 Redis，并执行示例应用的 Artisan 命令。

```sh
cargo build --locked --example sms
docker run -d --rm --name openrust-sms-demo -p 127.0.0.1:16379:6379 \
  --tmpfs /data redis:8 redis-server --save '' --appendonly no

export APP_BASE_PATH="$(mktemp -d)"
export APP_ENV=local CACHE_STORE=array QUEUE_CONNECTION=redis QUEUE_FAILED_DRIVER=null
export REDIS_HOST=127.0.0.1 REDIS_PORT=16379 REDIS_QUEUE=sms-demo
export SMS_ALPHA_URL=http://127.0.0.1:8767/alpha SMS_ALPHA_TOKEN=demo-alpha-token
export SMS_BETA_URL=http://127.0.0.1:8767/beta SMS_BETA_TOKEN=demo-beta-token

SMS_DRIVER=primary target/debug/examples/sms artisan sms:demo +12025550123 "First"
SMS_DRIVER=backup target/debug/examples/sms artisan sms:demo +12025550123 "Second"
SMS_DRIVER=backup target/debug/examples/sms artisan sms:demo +12025550123 "Third" --profile primary
SMS_DRIVER=backup target/debug/examples/sms artisan queue:work redis --stop-when-empty --tries=1
```

模拟端点会依次打印 `/alpha`、`/beta` 和 `/alpha`。为 `sms:demo` 加上 `--now` 可试用门面的直接发送。队列保存配置名称、号码和正文。Worker 从自己的配置读取端点和凭据。派发后修改 `sms.default`，已入队任务仍使用派发时选中的配置，需保留该配置直到任务完成。

用 Ctrl+C 停止模拟服务，再运行 `docker stop openrust-sms-demo`。模拟端点只接收本地 HTTP 请求，不发送真实短信。
