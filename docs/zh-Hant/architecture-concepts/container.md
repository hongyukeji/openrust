# 服務容器

## 綁定

在 Provider 的 `register` 中透過 `app.container()` 註冊服務。`bind::<T, _>` 每次解析都會呼叫工廠，`singleton::<T, _>` 共用首次成功結果，`instance::<T>` 提供已有的 `Arc<T>`。工廠接收同一個容器，可顯式解析依賴。

Rust 沒有 PHP 的建構函式反射。將介面綁定為 `dyn Trait`，由工廠回傳具體實作。Rust 在編譯時驗證介面實作，容器失敗透過 `BindingResolutionError` 回傳。未註冊類型不會自動建構。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};

trait Mailer: Send + Sync { fn send(&self) -> &'static str; }
struct SmtpMailer;
impl Mailer for SmtpMailer { fn send(&self) -> &'static str { "sent" } }

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singleton::<dyn Mailer, _>(|_| Ok(Arc::new(SmtpMailer)));
    app.alias(ServiceKey::typed::<dyn Mailer>(), "mailer")?;
    let mailer = app.get::<dyn Mailer>("mailer")?;
    assert_eq!(mailer.send(), "sent");
    assert!(Arc::ptr_eq(&mailer, &app.make::<dyn Mailer>()?));
    Ok(())
}
```

### 命名綁定與別名

使用 `bindNamed::<T, _>(name, factory)`、`singletonNamed::<T, _>(name, factory)` 或 `instanceNamed::<T>(name, instance)` 註冊命名項。這些是同一操作的命名形式，因為 Rust 不能按參數數量多載方法。`get::<T>(name)` 解析命名項，在呼叫工廠前驗證輸出類型。類型鍵與命名鍵相互獨立，即使顯示名稱相同也不會混淆。

`alias(abstract, alias)` 接收 `ServiceKey::typed::<T>()`、`ServiceKey::named(name)` 或字串。別名可以指向其他別名或稍後才註冊的項。`isAlias` 檢查原始鍵，`getAlias` 跟隨別名鏈，`has` 檢查綁定或別名而不建構服務。名稱精確比對且區分大小寫。在別名鍵上註冊綁定或實例會替換該別名。重新綁定目標影響後續解析，已回傳的 `Arc` 保持有效。

直接指向自身的別名會被拒絕。別名循環和工廠依賴循環回傳錯誤。單例建構失敗或 panic 不會快取結果，後續呼叫可以重試。並行解析共用首次成功的單例。呼叫使用者工廠前釋放容器鎖。

## 解析

使用 `make::<T>()` 按 Rust 類型解析，使用 `get::<T>(key)` 按名稱或顯式鍵解析。預設設定儲存庫綁定為 `dyn openrust::contracts::config::Repository`，並有 `config` 別名。每個應用擁有自己的容器，綁定和實例不是程序全域狀態。

延遲服務、上下文綁定、擴充器和解析回呼仍在 01-02 容器單元推進。上述綁定與解析能力不代表這些功能已經實作。

## 條件綁定與實例狀態

`bindIf`、`singletonIf` 僅在類型鍵沒有綁定、實例或別名時註冊工廠，命名形式為 `bindIfNamed`、`singletonIfNamed`。存在性檢查和寫入是原子操作，被拒絕的工廠不執行，並在登錄表解鎖後釋放。即使別名指向尚不存在的服務，也視為已有項目。

`isShared(key)` 檢查原始鍵，不跟隨別名。`resolved(key)` 跟隨別名，在存在實例或工廠已經成功解析時回傳 true。清除實例不會清除解析歷史。提供實例不會執行或丟棄原工廠。

`forgetInstance(key)` 清除精確鍵對應的實例，不跟隨別名。已有單例工廠隨後可重新建構實例。`forgetInstances()` 清除全部現存實例，保留工廠、別名和解析歷史。沒有工廠的實例項目會變為未綁定；已經回傳的 `Arc` 仍然有效。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{Container as _, BindingResolutionError}};

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonIfNamed::<u32, _>("counter", |_| Ok(Arc::new(7)));
    app.bindIfNamed::<u32, _>("counter", |_| Ok(Arc::new(8)));
    assert!(app.isShared("counter"));
    assert!(!app.resolved("counter")?);
    let original = app.get::<u32>("counter")?;
    app.instanceNamed("counter", Arc::new(99_u32));
    assert_eq!(*app.get::<u32>("counter")?, 99);
    app.forgetInstance("counter");
    let rebuilt = app.get::<u32>("counter")?;
    assert_eq!(*rebuilt, 7);
    assert!(!Arc::ptr_eq(&original, &rebuilt));
    Ok(())
}
```

這些操作不會清除門面自己的根物件快取，需要時另行呼叫門面快取清除介面。作用域服務清理見下文。

## 作用域綁定

`scoped::<T, _>` 在一次執行生命週期中共用服務，命名形式為 `scopedNamed::<T, _>(name, factory)`。使用 `scopedIf`、`scopedIfNamed` 僅在原始鍵未綁定時註冊。HTTP 處理器執行及每次 Artisan 分派自動進入生命週期，普通 `singleton` 實例仍由應用共用。

手動執行使用 Rust 生命週期輔助方法 `Container::scope(future)`。同一容器中的巢狀呼叫複用目前作用域，並行執行擁有獨立快取，即使執行在同一個 Tokio 執行緒中也不會混用。不同應用各自保存服務綁定及實例。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::Container as _};

fn main() {
    let app = Container::default();
    app.scoped::<u32, _>(|_| Ok(Arc::new(7)));
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        let first = app.scope(async {
            let instance = app.make::<u32>().unwrap();
            assert!(Arc::ptr_eq(&instance, &app.make::<u32>().unwrap()));
            instance
        }).await;
        let second = app.scope(async { app.make::<u32>().unwrap() }).await;
        assert!(!Arc::ptr_eq(&first, &second));
    });
}
```

生命週期 Future 持有快取引用，在執行完成、失敗或取消並釋放關聯 Future 後釋放。呼叫方可以持有超過作用域的 `Arc`，物件在最後一個引用釋放後銷毀。新作用域使用新的作用域實例，不複用容器根生命週期中已快取的實例。

`forgetScopedInstances()` 清除目前生命週期的作用域快取，保留其他活動生命週期及普通單例。在執行作用域之外呼叫，則清除根作用域實例。`forgetInstance(key)` 仍按原始鍵操作；`forgetInstances()` 同時清除目前作用域實例及應用的普通實例快取。作用域服務的 `instance` 覆蓋僅在目前生命週期生效，重新綁定仍是應用級註冊操作。

容器清理不會清除門面獨立快取的根物件；需要在同一次執行中重新解析時，同時清除門面快取。應用自行建立的 Tokio 任務不繼承環境作用域，應顯式傳入服務，或為任務設定所需生命週期。串流本文可以透過持有已解析服務的 `Arc` 延續其使用。串流及終止回呼接線仍屬於 HTTP 交付單元。
