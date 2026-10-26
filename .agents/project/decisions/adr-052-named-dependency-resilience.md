# ADR-052：命名出站依赖隔离与熔断

- 状态：Accepted
- 日期：2026-10-27

## 背景

统一 HTTP client 已具备显式幂等重试、总时限与响应流限制，但多个外部服务共用进程资源时，单个故障依赖仍可占满调用容量。重试没有完整处理抖动和 `Retry-After`，也没有每依赖的可选熔断边界。

## 决策

- 在 `openrust::http` 增加 `HttpClient::dependency(name, policy)`。同一客户端及其克隆按名称共享 runtime；同名但不同 policy 在创建时返回 `InvalidConfig`。不同名称使用独立容量与熔断状态。
- `DependencyPolicy` 分别限制并发数与等待数。排队满时立即返回 `DependencyOverloaded`；排队等待、请求发送与重试共享 `HttpClientConfig::timeout` 总预算。取消会释放等待槽位和 semaphore permit。收到响应头后 permit 随 `HttpResponse` 保留，正文读完、发生读取错误或响应被释放时归还。
- 熔断为可选、本进程内状态；连续 transport/timeout 错误或 5xx 达阈值时打开。冷却后只允许配置数量的半开探测；成功关闭熔断，失败重新打开，取消探测释放名额。4xx 与成功响应均清除连续失败计数；本地策略、配置和正文限制错误不计作依赖故障。
- 默认仍不重试。`RetryPolicy` 只允许 `idempotent()` 请求触发，最多五次；对 transport/timeout 以及 429、502、503、504 使用指数退避与全抖动。`Retry-After` 支持秒数与 HTTP 日期，按 `max_delay` 与 `retry_after_cap` 取上限；重试总预算不变，一次性正文仍拒绝重放。
- 不引入并行的 Tower 重试层或服务发现控制面。HTTP 隔离器只覆盖显式使用 `DependencyClient` 的调用；不覆盖直接 `HttpClient`、数据库、存储、其他客户端或跨进程总量。gRPC 实现后再按调用语义复用适用部分；RPC stream 不默认重试。

## 影响

默认 HTTP 行为不变。容量许可包括响应正文消费阶段，调用者必须读取/丢弃响应以释放容量；熔断状态不是集群共享限额。不同依赖需分别命名，避免一个依赖故障拖慢其他依赖。

## 验收

真实 loopback HTTP consumer 验证不同名称独立、同名共享容量、有界等待与取消释放、熔断打开/半开恢复；客户端单测验证重试尝试上限、一次性 body、jitter 上限与 `Retry-After` 解析/截断。完整门禁与实际运行结果写入 `memory/testing.md`。
