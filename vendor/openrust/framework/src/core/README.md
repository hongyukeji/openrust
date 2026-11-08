# 通用错误与执行上下文

本模块保留当前错误/响应类型、执行元数据、合作取消、限流及就绪状态。应用运行对象与启动装配见 [`crate::foundation`]，服务容器见 [`crate::container`]，命令见 [`crate::console`]，Provider公开扩展见 [`crate::support::ServiceProvider`]。

- [`Error`] / [`Result`] / [`ApiResponse<T>`](ApiResponse) / [`PageMeta`]：当前错误与响应扩展；后续按HTTP/Validation领域逐项收敛，不以现模块归属宣称完整官方对应。
- [`ExecutionContext`] / [`ExecutionMetadata`] / [`Cancellation`]：请求、命令与任务的关联信息、期限和合作取消；普通 `tokio::spawn` 不自动传播 task-local 上下文。
- [`RateLimiter`] / [`RateLimitStore`]：配额及后端扩展；后端故障返回错误，超限语义由调用入口执行。
- [`Readiness`]：listener状态及有界异步依赖探针。
