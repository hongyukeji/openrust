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
    app.singleton::<dyn Mailer, _>(|_| Ok(Arc::new(SmtpMailer)))?;
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

使用 `make::<T>()` 按 Rust 類型解析，使用 `get::<T>(key)` 按名稱或顯式鍵解析。引導時，設定儲存庫綁定為 `config`，`dyn openrust::contracts::config::Repository` 是其型別別名。每個應用擁有自己的容器，綁定和實例不是程序全域狀態。

上下文綁定、擴展器、解析回呼及重綁定見下文。延遲 Provider、動態建構覆寫和屬性投影仍待完成，01-02 尚未整體關閉。

## 條件綁定與實例狀態

`bindIf`、`singletonIf` 僅在類型鍵沒有綁定、實例或別名時註冊工廠，命名形式為 `bindIfNamed`、`singletonIfNamed`。存在性檢查和寫入是原子操作，被拒絕的工廠不執行，並在登錄表解鎖後釋放。即使別名指向尚不存在的服務，也視為已有項目。

`isShared(key)` 檢查原始鍵，不跟隨別名。`resolved(key)` 跟隨別名，在存在實例或工廠已經成功解析時回傳 true。清除實例不會清除解析歷史。提供實例不會執行或丟棄原工廠。

`forgetInstance(key)` 清除精確鍵對應的實例，不跟隨別名。已有單例工廠隨後可重新建構實例。`forgetInstances()` 清除全部現存實例，保留工廠、別名和解析歷史。沒有工廠的實例項目會變為未綁定；已經回傳的 `Arc` 仍然有效。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{Container as _, BindingResolutionError}};

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonIfNamed::<u32, _>("counter", |_| Ok(Arc::new(7))).unwrap();
    app.bindIfNamed::<u32, _>("counter", |_| Ok(Arc::new(8))).unwrap();
    assert!(app.isShared("counter"));
    assert!(!app.resolved("counter")?);
    let original = app.get::<u32>("counter")?;
    app.instanceNamed("counter", Arc::new(99_u32)).unwrap();
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
    app.scoped::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
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
use openrust::{container::Container, contracts::container::Container as _};
struct Report {
    channel: Arc<String>,
    port: Arc<u16>,
    services: Arc<Vec<Arc<String>>>,
}
fn main() {
    let container = Container::default();
    let config: Arc<dyn openrust::contracts::config::Repository> = Arc::new(openrust::config::Repository::new(Default::default()));
    container.instanceNamed("config", config.clone()).unwrap();
    config.set("reports.port", 8080.into()).unwrap();
    container.singleton::<String, _>(|_| Ok(Arc::new("default".into()))).unwrap();
    container.instanceNamed("local", Arc::new(String::from("local"))).unwrap();
    container.tag(["local"], ["reports"]);
    container.bind::<Report, _>(|c| Ok(Arc::new(Report {
        channel: c.make()?, port: c.get("$port")?, services: c.make()?,
    }))).unwrap();
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
    app.singleton::<String, _>(|_| Ok(Arc::new("service".into()))).unwrap();
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

## 擴展綁定

`extend::<T, _>(key, |service, container| ...)` 跟隨別名，裝飾已登記的回傳契約。閉包傳回設定後的 `Arc<T>`，或實作同一契約的包裝物件；可透過容器參數解析其他服務。契約不符時，在不相容的工廠或裝飾器執行前傳回錯誤。

尚無快取實例時，`extend` 儲存閉包供未來建構，不提前解析服務。裝飾器按登記順序在建構完成後、解析回呼前執行，上下文建構同樣適用。共享服務快取裝飾後的值。明確提供的 `instance` 略過待執行鏈；清除實例後恢復原工廠及該鏈。

已有快取實例時，`extend` 立即替換快取值，這次閉包不加入未來建構鏈。既有呼叫方保留原 `Arc`，下一次查找取得新值。與 Laravel 一樣，清除快取後不保留這種僅即時生效的裝飾。`forgetExtenders(key)` 移除未來裝飾器，不撤銷已經裝飾的快取實例。

建構和快取替換在登錄表鎖外執行使用者程式碼；建構等待期間登記的裝飾器在發布前納入。同一快取槽的更新循序處理，其他讀取方等待更新完成，裝飾器可讀取自身槽的舊值。已退役槽上的更新不會覆寫新綁定。遞迴擴展或依賴循環傳回循環依賴錯誤。發布前失敗或 panic 的共享建構可重試；快取更新失敗保留舊指標，不回復共享物件內部已經發生的修改。

待執行鏈屬於應用登記。對已快取作用域服務的即時擴展只修改目前生命週期實例，下一生命週期繼續使用原工廠和待執行鏈。門面自行快取的根物件不會自動替換，需要清理門面快取或使用不快取的門面重新解析。裝飾器及移除的擷取物件在內部鎖外釋放，退役裝飾器的擷取物件在新建構發布前釋放。重綁定監聽器與 `refresh` 見下文。

```rust
use std::sync::Arc;
use openrust::{container::{Container, ServiceKey}, contracts::container::Container as _};
trait Service: Send + Sync { fn name(&self) -> String; }
struct Base;
impl Service for Base { fn name(&self) -> String { "base".into() } }
struct Decorated(&'static str, Arc<dyn Service>);
impl Service for Decorated {
    fn name(&self) -> String { format!("{}:{}", self.0, self.1.name()) }
}
fn main() {
    let app = Container::default();
    let key = ServiceKey::typed::<dyn Service>();
    app.singleton::<dyn Service, _>(|_| Ok(Arc::new(Base))).unwrap();
    app.extend::<dyn Service, _>(key.clone(), |service, _| {
        Ok(Arc::new(Decorated("decorated", service)))
    }).unwrap();
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "decorated:base");
    app.extend::<dyn Service, _>(key.clone(), |service, _| {
        Ok(Arc::new(Decorated("cached", service)))
    }).unwrap();
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "cached:decorated:base");
    app.forgetInstance(key.clone());
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "decorated:base");
    app.forgetExtenders(key.clone()).unwrap();
    app.forgetInstance(key);
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "base");
}
```

## 重綁定與重新整理消費者

`rebinding::<T, _>(key, |container, service| ...)` 在登記監聽器時跟隨別名。服務已綁定時，登記會解析並傳回 `Some(Arc<T>)`，但不呼叫該監聽器；尚未綁定時傳回 `None`。初始解析失敗也保留已經登記的監聽器。

替換已解析的綁定時觸發監聽器；提供實例時，只有該原始鍵在替換前已經綁定才通知。首次綁定、被拒絕的條件綁定以及清除實例不通知。為已解析暫態服務或已清除快取的共享服務追加未來裝飾器時，會為監聽器重新建構服務；裝飾既有快取則通知替換後的值。每次事件持有自己的已提交值和有序回呼快照，並行更新不會將後一次值錯交給前一次事件。派發期間新登記的監聽器參與後續事件。

回呼在新綁定或快取提交後執行，錯誤和 panic 向呼叫方傳播，不回復已提交狀態，失敗事件中後續監聽器不再執行。同服務遞迴通知在巢狀變更提交後傳回循環錯誤。因此綁定登記方法傳回 `Result`，應透過 `?` 傳播錯誤；`instance` 和 `instanceNamed` 成功時傳回傳入的 `Arc`。

`refresh::<T, _, _>(key, target, setter)` 對應 Laravel 的目標物件/方法重新整理，接收明確的 `Arc` 目標與 setter，替代 PHP 方法名稱字串。登記傳回目前服務，不立即呼叫 setter，後續重綁定才呼叫。回呼使用明確回傳契約，在登錄表鎖外執行，保持應用及作用域隔離。門面 swap 先更新門面快取，再綁定容器；監聽器失敗時兩邊的新值都保留。

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::Container, contracts::container::{BindingResolutionError, Container as _}};
fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.instanceNamed("service", Arc::new(String::from("first")))?;
    let target = Arc::new(Mutex::new(String::new()));
    let current = app.refresh::<String, _, _>("service", target.clone(), |target, value| {
        *target.lock().unwrap() = (*value).clone();
        Ok(())
    })?.unwrap();
    assert_eq!(&*current, "first");
    assert!(target.lock().unwrap().is_empty());
    app.instanceNamed("service", Arc::new(String::from("second")))?;
    assert_eq!(&*target.lock().unwrap(), "second");
    Ok(())
}
```

## 非同步工廠

服務建構需要 I/O 或延遲相依服務時，使用非同步工廠。這些介面共用現有綁定表與生命週期，不建立第二個容器；將 PHP 的閉包執行轉為 Rust 明確等待的 Future。原同步介面繼續可用。

| 生命週期 | 型別工廠 | 具名工廠 |
| --- | --- | --- |
| 每次建構 | `bindAsync` | `bindAsyncNamed` |
| 單例 | `singletonAsync` | `singletonAsyncNamed` |
| 作用域 | `scopedAsync` | `scopedAsyncNamed` |

條件變體將 `If` 放在 `Async` 前，例如 `bindIfAsync`、`singletonIfAsyncNamed` 與 `scopedIfAsync`。註冊與重綁定通知都需 await；條件不符時不執行工廠。工廠接收目前容器，回傳 `Box::pin(async move { ... })`，結果為 `Result<Arc<T>, BindingResolutionError>`；公開型別別名是 `openrust::container::FactoryFuture`。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{BindingResolutionError, Container as _}};
struct Client;
struct Worker(Arc<Client>);
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonAsync::<Client, _>(|_| Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Arc::new(Client))
    })).await?;
    app.scopedAsync::<Worker, _>(|container| Box::pin(async move {
        Ok(Arc::new(Worker(container.makeAsync::<Client>().await?)))
    })).await?;
    app.scope(async {
        let a = app.makeAsync::<Worker>().await?;
        let b = app.makeAsync::<Worker>().await?;
        assert!(Arc::ptr_eq(&a, &b));
        assert!(Arc::ptr_eq(&a.0, &app.makeAsync::<Client>().await?));
        Ok::<(), BindingResolutionError>(())
    }).await?;
    Ok(())
}
```

工廠內的 `makeAsync`、`getAsync` 自動載入相依服務的延遲 Provider 並等待就緒，無需預先初始化。同步與非同步綁定共用消費者情境、別名、輸出型別、裝飾器與解析回呼。同步解析尚未建構的非同步工廠會回傳 `AsyncResolutionRequired`；共用快取就緒後可同步讀取。

情境選擇需要非同步解析時，使用 `giveAsync`、`giveServiceAsync`、`giveConfigAsync` 或 `giveTaggedAsync`。`tagged(...).collectAsync().await` 解析集合；`iter().nextAsync().await` 逐項解析，取消後該項仍留給下次呼叫。`rebindingAsync`、`refreshAsync`、`extendAsync` 等待必要建構與通知，但它們的回呼與裝飾器仍為同步函式；需要 await 的初始化放在工廠內。

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::{Container, ServiceKey}, contracts::container::{BindingResolutionError, Container as _}};
struct Worker(Arc<String>);
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.bindAsyncNamed::<String, _>("message", |_| Box::pin(async {
        Ok(Arc::new(String::from("first")))
    })).await?;
    app.when::<Worker>()?.needs::<String>().giveServiceAsync("message")?;
    app.bindAsync::<Worker, _>(|container| Box::pin(async move {
        Ok(Arc::new(Worker(container.makeAsync::<String>().await?)))
    })).await?;
    app.tag([ServiceKey::typed::<Worker>()], ["workers"]);
    let workers = app.tagged::<Worker>("workers").collectAsync().await?;
    assert_eq!(&*workers[0].0, "first");
    let target = Arc::new(Mutex::new(String::new()));
    app.refreshAsync::<String, _, _>("message", target.clone(), |target, value| {
        *target.lock().unwrap() = (*value).clone();
        Ok(())
    }).await?;
    app.bindAsyncNamed::<String, _>("message", |_| Box::pin(async {
        Ok(Arc::new(String::from("second")))
    })).await?;
    assert_eq!(&*target.lock().unwrap(), "second");
    Ok(())
}
```

並行共用解析不阻塞執行器，同次建構的等待者共用結果。建構失敗或取消時喚醒已有等待者並回傳錯誤，後續新呼叫可重試；相依環回傳錯誤。若解析回呼在值發布後失敗，已發布的共用值仍保留快取，沿用現有容器行為。

框架每次 poll 還原建構情境，暫停的工廠不會把消費者洩漏給其他 Future。請求與命令結束時關閉作用域並清空快取，即使外部還保留暫停的 Future。透過已關閉作用域繼續解析回傳 `ScopeClosed`；將門面 Future 移到另一應用執行中回傳 `FacadeNotSet`。銷毀暫停工作時還原仍存活的原情境；原情境關閉後不提供環境門面。應用主動持有的 Future 與服務參考仍由應用負責，drop 時釋放。這些明確的非同步錯誤與生命週期保護，是 PHP 順序機制在 Rust 並行執行環境中的轉換。
