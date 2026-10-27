# ADR-067：低基数指标与 OpenTelemetry 追踪

- 状态：Proposed（实现与真实服务验收完成后转为 Accepted）
- 日期：2026-10-27
- 范围：M5 可观测性与生产资源保障

## Spec

- 保留 `tracing` 为诊断事件 API；通过 `metrics` facade 记录框架指标，并以可选 Prometheus exporter 暴露采集格式。
- Prometheus 端点默认关闭，只有显式配置后才挂载；部署方负责将端点限制在可信网络或经授权的采集入口。
- HTTP 指标仅使用受限方法类别与状态类别标签，记录请求总数、持续时间和在途数；不把 URI、路由参数、请求 ID、trace ID、用户/租户 ID、错误文本或凭据作为标签。
- 请求日志和 span 默认不记录完整 URI、查询参数、请求 ID 或原始 `traceparent`；协议上下文通过 tracing/OpenTelemetry context 关联，不复制进普通属性。
- OpenTelemetry 使用可选 OTLP HTTP exporter，与现有 HTTP→gRPC→worker `traceparent` 上下文衔接；默认批量处理器在 SDK 后台线程执行，使用同步 HTTP client，避免把异步 client 放入无 Tokio reactor 的处理线程。禁用 feature 时不增加 exporter 运行时依赖。
- 同步补充单测、真实 HTTP scrape/trace 测试、配置与安全文档、`stack.md` 和验收记录。
- 资源与过载按独立验证逐步补齐：请求并发、gRPC 在途/流、worker 并发、实时会话与背压，明确拒绝/超时/取消及释放行为。
- 不把 readiness 探针实现重复纳入本 ADR；仅扩展其已有依赖状态观测。

## 验收边界

低基数由固定标签集合和无用户输入标签测试证明；端点须默认不注册、显式开启后 scrape 成功。追踪须在真实协议调用中保持 trace 关联且脱敏。压测报告必须记录固定环境、吞吐、p50/p95/p99、错误率、CPU、内存、过载响应和资源恢复；不以单测或单项微基准替代。
