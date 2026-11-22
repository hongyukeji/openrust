# 上下文

## 捕获上下文

在 HTTP 请求或 Artisan 命令中，通过 `Context` Facade 为当前执行添加数据。例如，中间件可以调用 `Context::add("trace_id", trace_id.into())?`。`addMany` 批量添加，`addIf` 保留已有值。键按字符串精确匹配。

键缺失或值为 JSON null 时，`get` 返回 `None`，可用 `has` 区分两种情况。`only` 和 `except` 读取子集，`forget` 移除键，`pull` 同时读取并移除。`push`、`pop` 和 `stackContains` 操作 JSON 列表，比较使用严格相等。计数器使用带溢出检查的整数运算。

## 作用域上下文

`Context::scope(future, data, hidden).await` 在当前上下文之上叠加临时数据。同步回调使用 `scopeSync`。完成、报错、取消或 panic 后恢复外层字段。并发子作用域各自维护临时字段。

应用任务可以在 `Container::scope` 内从容器解析 `openrust::log::context::Repository`。不同的执行生命周期各用一个仓库。队列集成会接入自动传递和还原事件。

```rust
use openrust::log::{LogContext, context::Repository};

fn main() {
    let context = Repository::default();
    context.add("trace_id", "outer".into());
    context.addHidden("job_token", "secret".into());

    context.scopeSync(
        || assert_eq!(context.get("trace_id"), Some("inner".into())),
        LogContext::from_iter([("trace_id".into(), "inner".into())]),
        LogContext::new(),
    );
    assert_eq!(context.get("trace_id"), Some("outer".into()));
    assert!(context.get("job_token").is_none());
    assert_eq!(context.getHidden("job_token"), Some("secret".into()));
}
```

## 隐藏上下文

通过 `addHidden`、`getHidden` 和对应方法保存不应进入日志元数据的字段。公开 Context 字段作为第二个 JSON 对象追加到日志，排在单条日志字段之后。占位符替换只读取单条日志字段。

在编译后的参数注入元数据中使用 `openrust::container::attributes::Context::<T>::new(key)`。通过 `.hidden()` 读取隐藏字段，通过 `.withDefault(value)` 或 `.withDefaultUsing(factory)` 为缺失或 null 设置默认值。字段必须能反序列化为声明的 Rust 类型。 [日志](../the-basics/logging.md)

## 队列 Context

`Queue::dispatch` 在入队前捕获公开和隐藏 Context。Worker 在每次尝试的独立作用域中恢复它们。隐藏字段不会写入日志，但会进入队列载荷，可用 `dehydrating` 回调移除。应用启动时注册 `dehydrating` 和 `hydrated` 回调。

```rust
use openrust::support::facades::Context;

fn configure_queue_context() -> Result<(), openrust::log::context::ContextError> {
    Context::dehydrating(|context| context.forgetHidden("secret"))?;
    Context::hydrated(|context| context.add("hydrated", serde_json::json!(true)))?;
    Ok(())
}
```
