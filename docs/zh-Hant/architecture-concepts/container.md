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

作用域綁定、延遲服務、上下文綁定、擴充器和解析回呼屬於下一容器單元。上述綁定與解析能力不代表這些功能已經實作。

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

這些操作不會清除門面自己的根物件快取，需要時另行呼叫門面快取清除介面。作用域服務清理由容器單元中的作用域生命週期部分交付。
