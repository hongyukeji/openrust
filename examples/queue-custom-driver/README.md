# 自定义队列 driver

此独立 Cargo 项目演示如何在不修改 OpenRust 源码的情况下实现 `QueueDriver`，并使用公开的 `JobRegistry` 注册与执行 typed Job。

```sh
cargo run --locked --manifest-path examples/queue-custom-driver/Cargo.toml
```

示例 driver 使用进程内队列，仅用于说明扩展边界。它不提供持久化、跨进程共享、失败任务保留或事务派发；不支持的事务派发会明确返回错误。生产 driver 必须自行定义持久化、至少一次执行、取消、失败恢复和进度存储语义。

调用方当前存在非空 `ExecutionMetadata`（request ID / locale）时，默认 metadata-aware 派发方法返回 `queue_driver_metadata_unsupported`，不会默默丢弃关联值。需要跨任务传递它的 driver 应实现 `dispatch_with_metadata` 及事务、任务组对应方法，并只持久化框架公开的白名单字段；认证主体、token、Cookie、任意 header、deadline 和取消句柄不应进入存储 payload。
