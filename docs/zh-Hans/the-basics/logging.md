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
