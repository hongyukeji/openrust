# 日志

## 配置

通过 `LOG_CHANNEL` 选择默认渠道，`LOG_STACK` 设置默认栈的成员顺序，`LOG_LEVEL` 设置最低级别。默认 `stack` 包含 `single`，向 `storage/logs/openrust.log` 追加记录。相对路径以应用根目录为基准。渠道可用 `name` 自定义记录标签，默认标签取应用环境。

当前驱动包括文件 `single`、标准错误 `stderr`、丢弃记录的 `null` 和组合渠道的 `stack`。记录达到最低级别时才打开文件。`single` 支持 `locking`、`bubble` 和整数 `permission`，如 Rust 的 `0o600`。权限设置在 Unix 生效。记录使用 UTC 时间。

## 构建日志栈

`channels` 支持列表或逗号分隔字符串。成员按顺序执行。`bubble: false` 的处理器接收记录后停止后续处理。`null` 丢弃达到阈值的记录并停止后续处理。设置 `ignore_exceptions: true` 后，组内尝试每个处理器并抑制写入错误。

`Log::stack(&["single", "stderr"], None)` 创建临时日志栈。命名渠道按应用惰性解析和缓存。更新命名渠道配置后，可用 `forgetChannel(Some(name))` 重新解析。

## 写入日志

在应用生命周期中调用 `Log::info("Order {id} created", &context)`。`Log::channel(Some("stderr"))?` 可选择命名渠道。上下文使用结构化 JSON，`replace_placeholders` 启用占位符替换。Rust 日志接口同步写入。

无效渠道回退到 `logging.channels.emergency.path`。写入失败时，紧急渠道写入诊断和原记录。紧急文件也无法写入时，改用标准错误。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager}};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let logger = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    let context = LogContext::from_iter([("id".into(), 42.into())]);
    logger.info("Order {id} created", &context)?;
    Ok(())
}
```

## 轮转文件渠道

使用 `daily` 或 `monthly` 生成如 `openrust-2026-11-11.log` 和 `openrust-2026-11.log` 的文件。每个 UTC 周期内，首条达到阈值的记录打开当期文件。`max_files` 包含当前文件，设为 0 时不限保留数量。应用默认保留 14 个日文件和 3 个月文件。清理仅处理基本文件名、扩展名和周期格式均匹配的普通文件。

## 临时渠道与自定义渠道

`Log::build(&settings)?` 创建临时日志渠道。使用 `Log::stackChannels` 及 `StackChannel::Named`、`StackChannel::Logger` 可组合配置渠道和临时渠道。组合后的日志栈有自己的标签。

配置 `driver: "custom"` 时，将 `via` 指向通过 `LogManager::registerFactory` 登记的工厂。使用 `LogManager::extend` 可注册具名驱动。工厂是编译后的 Rust 闭包，接收管理器和渠道配置，返回实现日志契约的对象。首次并发解析时可能同时调用工厂，初始化应可安全重复。成功解析的具名渠道共用发布后的实例。工厂失败时使用紧急日志，后续调用可重试。

```rust
use openrust::{config::Repository, log::{LogContext, LogManager, StackChannel}};
use serde_json::json;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let manager = LogManager::new(Arc::new(Repository::new(Default::default())), root);
    manager.registerFactory("audit", |manager, config| {
        let logger = manager.build(&json!({"driver":"monthly", "path":config["path"]}))?;
        Ok(logger)
    });
    let audit = manager.build(&json!({"driver":"custom", "via":"audit", "path":"audit.log"}))?;
    let stack = manager.stackChannels(
        &[StackChannel::Named("single"), StackChannel::Logger(&audit)],
        Some("orders"),
    )?;
    stack.info("Audit ready", &LogContext::new())?;
    Ok(())
}
```
