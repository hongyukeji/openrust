# 上下文

## 擷取上下文

在 HTTP 請求或 Artisan 命令中，透過 `Context` Facade 為目前執行加入資料。例如，中介軟體可以呼叫 `Context::add("trace_id", trace_id.into())?`。`addMany` 批次加入，`addIf` 保留既有值。鍵依字串精確比對。

鍵缺少或值為 JSON null 時，`get` 回傳 `None`，可用 `has` 區分兩種情況。`only` 和 `except` 讀取子集，`forget` 移除鍵，`pull` 同時讀取並移除。`push`、`pop` 和 `stackContains` 操作 JSON 清單，比對使用嚴格相等。計數器使用具溢位檢查的整數運算。

## 作用域上下文

`Context::scope(future, data, hidden).await` 在目前上下文之上疊加臨時資料。同步回呼使用 `scopeSync`。完成、錯誤、取消或 panic 後恢復外層欄位。並行子作用域各自維護臨時欄位。

應用程式工作可在 `Container::scope` 內從容器解析 `openrust::log::context::Repository`。不同的執行生命週期各用一個儲存庫。佇列整合會接入自動傳遞與還原事件。

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

## 隱藏上下文

透過 `addHidden`、`getHidden` 與對應方法保存不應進入日誌中繼資料的欄位。公開 Context 欄位作為第二個 JSON 物件附加到日誌，排在單筆日誌欄位之後。預留位置替換只讀取單筆日誌欄位。

在編譯後的參數注入中繼資料中使用 `openrust::container::attributes::Context::<T>::new(key)`。透過 `.hidden()` 讀取隱藏欄位，透過 `.withDefault(value)` 或 `.withDefaultUsing(factory)` 為缺少或 null 設定預設值。欄位須可反序列化為宣告的 Rust 型別。 [日誌](../the-basics/logging.md)

## 佇列 Context

`Queue::dispatch` 在加入佇列前擷取公開與隱藏 Context。Worker 在每次嘗試的獨立作用域中恢復這些欄位。隱藏欄位不會寫入記錄，但會進入佇列承載資料，可使用 `dehydrating` 回呼移除。在應用程式啟動時登記 `dehydrating` 和 `hydrated` 回呼。

```rust
use openrust::support::facades::Context;

fn configure_queue_context() -> Result<(), openrust::log::context::ContextError> {
    Context::dehydrating(|context| context.forgetHidden("secret"))?;
    Context::hydrated(|context| context.add("hydrated", serde_json::json!(true)))?;
    Ok(())
}
```
