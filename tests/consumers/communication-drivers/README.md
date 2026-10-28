# 自定义邮件与短信 driver

此独立 Cargo consumer 展示业务应用如何只依赖 OpenRust 的公开 API，实现 `MailDriver` / `SmsDriver`，通过应用自有 `ServiceProvider` 读取配置并注册 `Mailer` / `SmsSender`。业务代码只调用统一服务，不引用具体供应商实现。示例 driver 仅计数，不会发送网络请求。

应用自行维护供应商配置及其验证；不要把自定义 driver 塞入内建 `MailerServiceProvider` / `SmsServiceProvider` 的 `driver` 枚举。选择自定义供应商时，应用注册自己的 Provider，并省略对应内建 Provider。工厂从统一配置仓储读取当前配置，服务绑定按 Provider 注册顺序覆盖。公共服务保证统一调用入口，不自动定义供应商重试、送达状态、幂等或凭据轮换语义；短信回执仅表示供应商接受提交。

```sh
cargo test --locked --manifest-path tests/consumers/communication-drivers/Cargo.toml
cargo run --locked --manifest-path tests/consumers/communication-drivers/Cargo.toml
```
