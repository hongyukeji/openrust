# 自定义队列 driver

此独立 Cargo 项目演示如何在不修改 OpenRust 源码的情况下实现 `QueueDriver`，并使用公开的 `JobRegistry` 注册与执行 typed Job。

```sh
cargo run --locked --manifest-path examples/queue-custom-driver/Cargo.toml
```

示例 driver 使用进程内队列，仅用于说明扩展边界。它不提供持久化、跨进程共享、失败任务保留或事务派发；不支持的事务派发会明确返回错误。生产 driver 必须自行定义持久化、至少一次执行、取消、失败恢复和进度存储语义。
