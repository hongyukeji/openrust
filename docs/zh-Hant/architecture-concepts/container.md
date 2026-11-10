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

## 上下文綁定與標籤服務

`when::<Consumer>()?.needs::<Dependency>().give(factory)?` 為目前建構的消費者選擇依賴。`whenNamed(name)` 和 `whenMany(keys)` 對應具名及多個消費者；`needsNamed::<T>("$parameter")` 對應明確命名的建構輸入。Rust 沒有 PHP 建構反射，工廠須明確解析依賴，登記的消費者身分對應 PHP 反射取得的類別。

上下文規則只作用於最近的消費者，優先於預設實例、單例和作用域快取，但不替換預設快取，也不將依賴記為全域已解析。上下文工廠可以再次解析同一依賴取得預設綁定；實際遞迴工廠仍傳回循環依賴錯誤。消費者別名在呼叫 `when` 時正規化，依賴別名在呼叫 `give` 時正規化，解析時還會檢查後續增加的直接別名。

`give` 接收型別明確的工廠。`giveService(key)` 對應 Laravel 中傳服務名稱的 `give`，目標須按預期回傳契約登記；`Arc<Concrete>` 到 `Arc<dyn Trait>` 的轉換在工廠中明確完成。`giveConfig(key, default)` 在解析依賴時讀取目前 `config` 服務，`None` 對應 Laravel 省略預設值。鍵缺失時使用預設值，明確 JSON null 保持 null。反序列化會檢查 Rust 宣告型別，不符則傳回錯誤，不隱含執行 PHP 純量轉換。

`tag(keys, tags)` 保留登記順序和重複項。`tagged::<T>(tag)` 迭代至某項時才解析服務，每次迭代按原服務生命週期重新解析。未知標籤取得空序列；既有序列的下一次迭代可看到之前追加的服務。`len()` 傳回建立序列時固定的數量，對應 Laravel 可重複產生器的行為。`giveTagged` 收集為 `Vec<Arc<T>>`，未綁定或型別不符的項目傳播錯誤。

```rust
use std::sync::Arc;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};
struct Report {
    channel: Arc<String>,
    port: Arc<u16>,
    services: Arc<Vec<Arc<String>>>,
}
fn main() {
    let app = Application::configure(std::env::current_dir().unwrap()).create().unwrap();
    let container = app.container();
    let config = container.get::<dyn openrust::contracts::config::Repository>("config").unwrap();
    config.set("reports.port", 8080.into()).unwrap();
    container.singleton::<String, _>(|_| Ok(Arc::new("default".into())));
    container.instanceNamed("local", Arc::new(String::from("local")));
    container.tag(["local"], ["reports"]);
    container.bind::<Report, _>(|c| Ok(Arc::new(Report {
        channel: c.make()?, port: c.get("$port")?, services: c.make()?,
    })));
    container.when::<Report>().unwrap()
        .needs::<String>().giveService("local").unwrap()
        .needsNamed::<u16>("$port").giveConfig("reports.port", None).unwrap()
        .needs::<Vec<Arc<String>>>().giveTagged("reports").unwrap();
    let report = container.make::<Report>().unwrap();
    assert_eq!(&*report.channel, "local");
    assert_eq!(*report.port, 8080);
    assert_eq!(report.services.len(), 1);
    assert_eq!(&*container.make::<String>().unwrap(), "default");
}
```

## 容器解析事件

`beforeResolving`、`resolving` 和 `afterResolving` 用 `None` 登記全域回呼，用 `Some(ServiceKey)` 選擇服務身分。登記時正規化別名。回呼接收 `ResolutionEvent` 與目前容器，傳回 `Result<(), BindingResolutionError>`。實作提取舊版的事件檢視與有序選擇器設計，適配目前同步工廠。

執行順序為全域 before → 指定 before → 建構 → 全域 resolving → 指定 resolving → 全域 after → 指定 after。before 回呼可以登記尚未綁定的服務，每次查找都執行，快取命中也執行。明確提供的實例及共享快取命中略過建構後回呼。每階段先取得全域回呼快照，全域執行完再讀取指定回呼；因此全域回呼中新登記的指定回呼可以參與目前階段，登記到後續階段的回呼也可在本次建構中執行。

`event.key()` 是執行 before 前確定的正規服務身分。`event.value::<T>()` 在建構前傳回 `None`，建構後按已宣告的回傳契約取得同一 `Arc<T>`。建構後的指定回呼按服務鍵或 Rust 宣告的回傳型別匹配。Rust 無法於執行期發現任意 PHP 繼承或介面關係，須明確登記預期的回傳契約；需要設定共享物件時使用內部可變性。動態建構參數覆寫和屬性投影尚未納入本回呼介面。

共享物件先發布再執行建構後回呼，回呼可重入解析同一共享物件；其他執行緒等待回呼結束，不讀取設定到一半的物件。建構後回呼失敗或 panic 時，已發布物件仍保留快取，命中快取不重複執行回呼，但不記錄成功解析歷史。暫態物件失敗時沒有快取，下次重新建構。回呼錯誤沿既有 HTTP/Artisan 錯誤處理傳播。沒有終止條件的同服務回呼遞迴傳回循環依賴錯誤。執行使用者回呼時不持有登錄表鎖。

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::{Container, ServiceKey}, contracts::container::Container as _};
fn main() {
    let app = Container::default();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let before = trace.clone();
    app.beforeResolving(None, move |event, _| {
        assert!(event.value::<String>().is_none());
        before.lock().unwrap().push("before");
        Ok(())
    }).unwrap();
    app.singleton::<String, _>(|_| Ok(Arc::new("service".into())));
    let resolving = trace.clone();
    app.resolving(Some(ServiceKey::typed::<String>()), move |event, container| {
        assert_eq!(&*event.value::<String>().unwrap(), "service");
        assert!(container.resolved(event.key().clone())?);
        resolving.lock().unwrap().push("resolving");
        Ok(())
    }).unwrap();
    let after = trace.clone();
    app.afterResolving(None, move |_, _| {
        after.lock().unwrap().push("after");
        Ok(())
    }).unwrap();
    let first = app.make::<String>().unwrap();
    assert!(Arc::ptr_eq(&first, &app.make::<String>().unwrap()));
    assert_eq!(*trace.lock().unwrap(), ["before", "resolving", "after", "before"]);
}
```
