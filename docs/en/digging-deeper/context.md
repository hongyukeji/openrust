# Context

## Capturing Context

Use the `Context` facade inside an HTTP request or Artisan command to attach data to the current execution. For example, middleware can call `Context::add("trace_id", trace_id.into())?`. Use `addMany` to add a map or `addIf` to keep an existing entry. Context keys are exact strings.

`get` returns `None` for a missing key or JSON null. Use `has` to distinguish a stored null from a missing key. Use `only` and `except` for subsets, `forget` to remove a key, and `pull` to read and remove it in one operation. The stack APIs `push`, `pop` and `stackContains` operate on JSON lists with strict equality. Counters use checked integer arithmetic.

## Scoped Context

`Context::scope(future, data, hidden).await` overlays temporary data on the current context. Use `scopeSync` for a synchronous callback. Outer fields are restored on completion, error, cancellation or panic. Concurrent child scopes have independent overlays.

An application task can run inside `Container::scope` and resolve `openrust::log::context::Repository` from the container. Each separate lifecycle gets its own repository. Automatic transfer to queued jobs and the hydration events belong to the queue integration.

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

## Hidden Context

Use `addHidden`, `getHidden` and the corresponding hidden methods for values that should stay out of log metadata. Log records carry public Context fields in a second JSON object, after the fields passed to the individual log call. Only the individual fields participate in placeholder substitution.

Use `openrust::container::attributes::Context::<T>::new(key)` in compiled injection metadata. Add `.hidden()` to read hidden context, and `.withDefault(value)` or `.withDefaultUsing(factory)` for missing or null values. Values must deserialize to the declared Rust type. [Logging](../the-basics/logging.md)

## Queue context

`Queue::dispatch` captures public and hidden context before enqueueing. The worker hydrates it in a fresh execution scope for every attempt. Hidden fields stay out of log output. They are still part of the queue payload unless a `dehydrating` hook removes them. Register `dehydrating` and `hydrated` callbacks during application boot.

```rust
use openrust::support::facades::Context;

fn configure_queue_context() -> Result<(), openrust::log::context::ContextError> {
    Context::dehydrating(|context| context.forgetHidden("secret"))?;
    Context::hydrated(|context| context.add("hydrated", serde_json::json!(true)))?;
    Ok(())
}
```
