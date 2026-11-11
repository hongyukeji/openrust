# 服務容器

## 綁定

在 Provider 的 `register` 中透過 `app.container()` 註冊服務。`bind::<T, _>` 每次解析都會呼叫工廠，`singleton::<T, _>` 共用首次成功結果，`instance::<T>` 提供已有的 `Arc<T>`。工廠接收同一個容器，可顯式解析依賴。

Rust 沒有 PHP 的建構函式反射。將介面綁定為 `dyn Trait`，由工廠回傳具體實作。Rust 在編譯時驗證介面實作，容器失敗透過 `BindingResolutionError` 回傳。沒有綁定或編譯建構中繼資料的型別不會自動建構。

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

## 參數覆寫

使用 `Application::makeWith::<T>(parameters).await` 提供明確建構參數。直接呼叫容器時，同步使用 `makeWith`，非同步使用 `makeWithAsync`；具名鍵對應 `makeWithNamed` 與 `makeWithAsyncNamed`。應用提供 `makeWithNamed(...).await` 解析具名鍵。

空參數集合沿用普通解析，可重用共用實例。非空集合選擇已註冊工廠，繞過明確實例與共用快取，單獨建構結果；仍套用情境選擇、裝飾器與解析回呼，但不取代普通共用值，也不寫入普通解析歷史。僅登記實例而無工廠的服務收到非空參數時回傳 `NotBound`。

PHP 工廠接收參數陣列；Rust 工廠保留明確容器引數，透過 `getLastParameterOverride()` 讀取目前不可變快照。解析前回呼透過 `ResolutionEvent::parameters()` 取得快照。普通巢狀解析使用空參數，需要轉交時明確傳遞快照。非同步解析每次 poll 還原自己的參數框架，包括在另一帶參數建構中還原普通暫停解析的情況。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
fn arguments(number: u32) -> ParameterOverrides {
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("number"), ParameterValue::new(number));
    parameters
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonAsync::<u32, _>(|container| Box::pin(async move {
        tokio::task::yield_now().await;
        let value = container.getLastParameterOverride()
            .get(&ParameterKey::name("number"))
            .map(|value| value.downcast::<u32>()).transpose()?;
        Ok(Arc::new(value.map_or(0, |value| *value)))
    })).await?;
    assert_eq!(*app.makeWithAsync::<u32>(arguments(7)).await?, 7);
    assert!(!app.resolved(ServiceKey::typed::<u32>())?);
    let normal = app.makeAsync::<u32>().await?;
    assert_eq!(*normal, 0);
    assert_eq!(*app.makeWithAsync::<u32>(arguments(9)).await?, 9);
    assert!(Arc::ptr_eq(&normal, &app.makeAsync::<u32>().await?));
    Ok(())
}
```

`ParameterOverrides` 保留插入順序；覆寫同鍵保持位置並回傳舊值。複製共用不可變資料，後續修改複製集合。`ParameterKey::Name` 與 `Index` 是明確身分，數字字串不隱式轉換成整數鍵。`ParameterValue::null()` 與缺少參數不同。值需要 `Send + Sync + 'static`，無需實作 Clone 或序列化；`from_arc` 保留既有物件身分。儲存 `Arc<dyn Trait>` 時使用 `new` 包裝，並按相同 `Arc<dyn Trait>` 型別讀取。

`downcast` 要求宣告的 Rust 型別；型別錯誤或 null 回傳 `ParameterTypeMismatch`，不暴露承載內容。此載體不執行 PHP 純量隱式轉換。錯誤、panic、取消後還原原參數框架。暫停工廠清理可讀取自己的參數；退役參數內容在空參數框架內釋放，不借用另一呼叫的參數。呼叫者明確保留的快照與服務參考仍由呼叫者持有。

## 自動建構式注入

Rust 沒有 PHP 的執行期建構式反射。可在 Provider 的 `register` 中透過 `app.container().registerClass(...)` 登記 `ClassDescriptor`。描述器宣告具名參數型別與編譯後的建構函式，取代反射檢查與動態呼叫。登記後，普通 `make` / `makeAsync` 自動解析其相依服務，無需額外綁定服務工廠。既無中繼資料也無綁定的型別回傳 `NotBound`。

`ClassDescriptor::concrete::<T, _>` 接收同步建構函式，`concreteAsync` 接收可等待的建構函式。同步解析無法初始化非同步建構式或相依服務，會回傳既有非同步解析錯誤；非同步通道自動載入相依服務的延遲 Provider。中繼資料屬於目前應用且不可變：重複登記同一描述器複本是冪等的，同型別的不同定義則拒絕。中繼資料本身不會讓 `bound` 或 `isShared` 成為 true。普通成功解析記錄歷史，明確非空參數覆寫則不記錄。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::{BindingResolutionError, Container as _},
};
struct Client;
struct Service { client: Arc<Client>, name: String }
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.registerClass(ClassDescriptor::concreteAsync::<Client, _>(vec![], |_| Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Arc::new(Client))
    }))?)?;
    app.registerClass(ClassDescriptor::concrete::<Service, _>(vec![
        ConstructorParam::class::<Client>("client"),
        ConstructorParam::scalar::<String>("name")
            .withDefault(|| Ok(ParameterValue::new(String::from("default")))),
    ], |args| Ok(Arc::new(Service {
        client: args.service(0)?, name: (*args.get::<String>(1)?).clone(),
    })))?)?;
    assert!(!app.bound::<Service>());
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("name"), ParameterValue::new(String::from("custom")));
    assert_eq!(app.makeWithAsync::<Service>(parameters).await?.name, "custom");
    app.singletonAsync::<Service, _>(|container| Box::pin(async move {
        container.buildAsync::<Service>().await
    })).await?;
    let service = app.makeAsync::<Service>().await?;
    assert_eq!(service.name, "default");
    let cached = app.makeAsync::<Service>().await?;
    assert!(Arc::ptr_eq(&service.client, &cached.client));
    assert!(Arc::ptr_eq(&service, &cached));
    Ok(())
}
```

`ConstructorParam::scalar::<T>("name")` 選擇宣告的 Rust 型別值；`class::<T>("name")` 選擇 `Arc<T>`，包括 `dyn Trait` 契約。`ResolvedArgs::get` 讀取原始值槽，`service` 回傳原服務 Arc，`value` 暴露型別化載體。槽位按宣告順序排列；空參數名、重複名稱在登記前拒絕。

選參遵循固定基線：同名明確覆寫優先。原始值隨後選擇 `$name` 情境綁定、`withDefault`、啟用 `nullable` 時的空值，否則失敗。類別預設值僅在該類別沒有明確綁定或直接情境綁定時使用；否則透過容器解析類別。沒有預設值的可空類別仍嘗試解析相依服務，不會把無法解析的類別靜默改成 null。可選預設值使用 `nullable().withDefault(|| Ok(ParameterValue::null()))`，透過 `optional` 或 `optionalService` 讀取可空槽。

相依服務與預設值按宣告順序求值，再對已選參數驗證型別後呼叫建構函式。預設函式只在選中時執行，每次重新求值。參數保持嚴格 Rust 型別，不實作 PHP 弱純量隱式轉換。普通巢狀相依服務使用空覆寫；既有情境、裝飾器、解析回呼與就緒保護繼續有效。

`build::<T>()` / `buildAsync::<T>().await` 直接呼叫編譯建構函式，繞過目標的綁定、快取、裝飾器、解析回呼與歷史；其相依服務仍使用普通容器解析。可用 `singleton(|container| container.build::<T>())`、`scoped` 或對應非同步介面為描述類別設定共用生命週期。`build` 使用目前參數快照，帶參數工廠可直接轉交，不把參數寫入全域註冊表。

方法呼叫、SelfBuilding、情境/類別屬性與自動介面投影仍待交付，登記中繼資料不代表這些能力已可用。

## 可變參數注入

為末尾的 `ConstructorParam` 加上 `variadic()`。可變參數不能有預設值，且只能有一個宣告。`ResolvedArgs::values(index)` 讀取有序項目，包括空清單；`services::<T>(index)` 回傳服務參考。`len()` 統計宣告槽位；普通 `value` / `get` / `service` 讀取可變參數槽會回傳 `InvalidArgumentShape`。

具名覆寫恰好傳入一項，值本身是向量時也不會展開。整數索引覆寫不會取代同名槽。每項必須符合宣告的 Rust 型別，`nullable()` 允許明確 null 項目。缺少原始值可變參數或無法解析的類別可變參數均產生空清單，不注入 null。可建構的類別沒有情境清單時，提供一個普通解析實例。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam},
    contracts::container::{BindingResolutionError, Container as _},
};
trait Filter: Send + Sync { fn name(&self) -> &str; }
struct Named(&'static str);
impl Filter for Named { fn name(&self) -> &str { self.0 } }
struct Pipeline(Vec<Arc<dyn Filter>>);
fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.bindNamed::<dyn Filter, _>("first", |_| Ok(Arc::new(Named("first"))))?;
    app.bindNamed::<dyn Filter, _>("second", |_| Ok(Arc::new(Named("second"))))?;
    app.registerClass(ClassDescriptor::concrete::<Pipeline, _>(
        vec![ConstructorParam::class::<dyn Filter>("filters").variadic()],
        |args| Ok(Arc::new(Pipeline(args.services(0)?))),
    )?)?;
    app.when::<Pipeline>()?.needs::<dyn Filter>()
        .giveServices(["first", "second", "first"])?;
    let pipeline = app.make::<Pipeline>()?;
    let names: Vec<_> = pipeline.0.iter().map(|filter| filter.name()).collect();
    assert_eq!(names, ["first", "second", "first"]);
    Ok(())
}
```

Laravel 的類別服務 `give(array)` 對應 `needs::<T>().giveServices(keys)`。服務鍵在原消費者情境中按順序解析，保留重複項目；登記不預先建構，相依鍵的別名會正規化，進行中的建構持有不可變鍵快照。非同步建構通道會等待延遲或非同步服務，無需另一套清單登記介面。

標籤或計算型集合工廠沿用既有型別化集合契約：`needs::<Vec<Arc<T>>>().giveTagged(tag)` 或 `giveTaggedAsync(tag)`，可變參數建構式可使用該集合。`T` 的直接情境綁定優先於集合轉接。集合工廠作為自己的解析執行，項目不會繼承消費者類別的情境綁定；直接 `giveServices` 陣列則保留該類別情境。集合回呼使用 `Vec<Arc<T>>` 身分，這是 Rust 清單輸出契約的轉換。

原始值的 `$name` 情境可提供 `Vec<Arc<T>>` 或 `Vec<ParameterValue>`。例如 `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` 讀取目前設定陣列；框架啟用 Serde 的 `rc` 特性支援這些型別化清單。共用項目無需實作 Clone，異質載體保留 null 並逐項驗證。所有權表示有參數包裝與參考計數成本，未宣稱零成本或數值效能收益。

原始值情境閉包在建構情境內直接執行，對應 Laravel 的原始值解析器；保留建構參數快照，不為 `$name` 增加服務事件、快取或解析歷史。閉包內的普通相依服務仍按容器機制解析，非同步閉包可在相同情境等待初始化。

類別可變參數遇到缺少綁定或相依服務的原始值無法解析錯誤時，將整個參數退回空清單，包括已部分解析的服務陣列。業務失敗、型別錯誤、相依/別名環、非同步解析要求、Provider 失敗與生命週期關閉仍傳播；原始值可變參數的錯誤亦傳播。取消時在原情境釋放部分項目，共用服務與應用主動保留的值仍按原所有權存活。

## 方法呼叫

在 Container 或 Application 上使用 `call(target, parameters).await`。Rust 沒有 PHP 執行期反射：`Callback` 提供閉包參數中繼資料，`registerMethod` 與 `registerStaticMethod` 登記編譯後的方法介接器。閉包可以借用資料並回傳一般 Future。登記方法的回傳值要求 `Send + 'static`，不要求 `Sync`；匿名閉包可回傳借用值。

```rust
use openrust::{
    container::{Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::Container as _,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    let signature = [ConstructorParam::scalar::<u32>("number")];
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("number"), ParameterValue::new(7u32));
    let mut result = String::new();
    let result = &mut result;
    let value = container.call(Callback::new(&signature, move |args| async move {
        result.push_str(&args.resolved.get::<u32>(0)?.to_string());
        Ok(result.as_str())
    }), parameters).await.unwrap();
    assert_eq!(value, "7");
}
```

BoundMethod 使用獨立選參規則。同名覆寫優先，其次 `ParameterKey::class::<T>()`，再解析類別相依，每個明確鍵僅消費一次。類別未綁定時使用預設值，即使存在僅情境綁定；原始值使用預設值，nullable 本身不補缺失值，也不讀取 `$name` 情境綁定。整數鍵及其他未消費輸入按插入順序保留於 `CallArguments.remaining`，不替代缺失的具名參數。末尾變參接收這些剩餘值。明確陣列仍是一項，計算類別集合會展開，建構用的 `giveServices` 陣列不適用於方法注入；缺失類別相依繼續報錯。

```rust
use openrust::{
    container::{ConstructorParam, Container, MethodCall, ParameterOverrides, StaticCall},
    contracts::container::Container as _,
};
use std::sync::Arc;

struct Worker;
struct Client(&'static str);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.instance(Arc::new(Worker)).unwrap();
    container.instance(Arc::new(Client("ready"))).unwrap();
    container.registerMethod::<Worker, String, _>("run",
        vec![ConstructorParam::class::<Client>("client")],
        |_, args, _| Box::pin(async move {
            Ok(args.resolved.service::<Client>(0)?.0.to_owned())
        }),
    ).unwrap();
    let value: String = container.call(
        MethodCall::<Worker>::class("run"), ParameterOverrides::new(),
    ).await.unwrap();
    assert_eq!(value, "ready");
    container.bindMethod::<Worker, String, _>("run", |_, _| {
        Box::pin(async { Ok("replacement".to_owned()) })
    }).unwrap();
    assert!(container.hasMethodBinding::<Worker>("run"));
    let value: String = container.call(
        MethodCall::on(Arc::new(Worker), "run"), ParameterOverrides::new(),
    ).await.unwrap();
    assert_eq!(value, "replacement");
    container.registerStaticMethod::<Worker, u32, _>("version", vec![], |_, _| {
        Box::pin(async { Ok(1) })
    }).unwrap();
    container.bindStaticMethod::<Worker, u32, _>("version", |_, _| {
        Box::pin(async { Ok(2) })
    }).unwrap();
    assert_eq!(container.call::<u32, _>(StaticCall::<Worker>::qualified("version"),
        ParameterOverrides::new()).await.unwrap(), 1);
    assert_eq!(container.call::<u32, _>(StaticCall::<Worker>::array("version"),
        ParameterOverrides::new()).await.unwrap(), 2);
}
```

`MethodCall::on(receiver, method)` 對應物件陣列呼叫；`class(method)` 對應 `Class@method`，先建構接收者再讀取替代綁定。`defaultMethod(method)` 與 `invokable()` 對應類別預設方法和已宣告的 `__invoke`，僅登記替代綁定不算方法宣告。空字串與 "0" 預設名走 invokable。方法宣告以 ASCII 忽略大小寫查找，替代綁定按呼叫者原始拼寫匹配。

`bindMethod` 替代原方法本體，並跳過原參數注入與驗證。`hasMethodBinding` 檢查替代綁定，`callMethodBinding` 直接執行它，不額外建立類別情境。靜態重載為 `bindStaticMethod`、`callStaticMethodBinding`。`StaticCall::qualified` 對應 `Class::method` 字串，不查詢方法綁定；`StaticCall::array` 對應類別陣列呼叫，會查詢綁定。錯誤的接收者形式或回傳型別回傳型別化錯誤。

實際可呼叫的物件或靜態方法，只在邏輯建構堆疊尚無該類別時加入類別情境。`Class@method` 按 Laravel BoundMethod.callClass 保留父情境；相依仍建立自己的正常建構情境。逐次非同步 poll 與取消時恢復情境，確保並行與多應用隔離。Application 入口同時建立該應用的 Facade 情境。進行中的呼叫保留選定綁定、原始參數與接收者，直到完成或取消。

編譯型別 `C` 替代 PHP 類名反射；Rust trait 別名與具體類別間不自動投影方法。介接器以 `Result<R, BindingResolutionError>` 傳播注入錯誤，`R` 本身可為業務 Result。為支援 Rust 1.94 的非同步借用，儲存回呼及呼叫分派 Future 使用 Box。未做效能基準，不宣稱零配置。本節尚不包含參數屬性或 SelfBuilding。

## 延遲閉包

`factory::<T>()` 回傳每次執行 `make::<T>()` 的閉包。`factoryNamed::<T>(key)` 保存原始、擁有所有權的 ServiceKey，每次即時查找別名。建立時不檢查或解析服務，後續登記、替換及執行個體驅逐立即可見；singleton/scoped 與一般解析的快取行為一致。容器非同步重載為 `factoryAsync`、`factoryAsyncNamed`。Application 提供非同步 `factory`、`factoryNamed`，每次解析均建立該應用的 Facade 情境。

`wrap(Callback::reusable(signature, callback), parameters)` 回傳重複執行 `call` 的非同步閉包。輸入參數保存為寫入時複製快照，相依與預設值每次重新選擇。同一回呼狀態被共享，不複製回呼或快取結果；失敗與取消不消耗包裝器。作用域與類別情境按執行時決定，不在建立包裝器時凍結。

```rust
use openrust::{
    container::{Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::Container as _,
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    let factory = container.factory::<u32>();
    container.instance(Arc::new(3u32)).unwrap();
    assert_eq!(*factory().unwrap(), 3);
    container.instance(Arc::new(9u32)).unwrap();
    assert_eq!(*factory().unwrap(), 9);

    let signature = [ConstructorParam::class::<u32>("number"),
        ConstructorParam::scalar::<u32>("increment")];
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("increment"), ParameterValue::new(2u32));
    let wrapped = container.wrap(Callback::reusable(&signature, |args| async move {
        Ok(*args.resolved.service::<u32>(0)? + *args.resolved.get::<u32>(1)?)
    }), parameters.clone());
    parameters.insert(ParameterKey::name("increment"), ParameterValue::new(7u32));
    assert_eq!(wrapped().await.unwrap(), 11);
    container.instance(Arc::new(5u32)).unwrap();
    assert_eq!(wrapped().await.unwrap(), 7);

    let async_factory = container.factoryAsyncNamed::<u32>(
        openrust::container::ServiceKey::typed::<u32>(),
    );
    assert_eq!(*async_factory().await.unwrap(), 5);
}
```

`Callback::reusable` 向 Rust 1.94 的閉包推斷提供可重複執行的 `Fn` 契約，一般呼叫的 `Callback::new` 繼續支援 `FnOnce`。包裝回呼需要 `Send + Sync`，可變共享狀態使用同步機制。回傳閉包借用 Container/Application，不能比它存活更久。每次回傳獨立 Send Future，可回傳外部借用值。具體閉包型別避免包裝器本身的 Box，共享回呼使用 Arc，非同步分派保留前述 Box Future 介接；不宣稱實測效能收益。

## 綁定目錄與清理

`getBindings()` 回傳擁有所有權、保留插入順序的 `Bindings` 快照。`get(&ServiceKey)` 精確查鍵，`iter`、`keys` 與所有權迭代保留登記順序；替換綁定不移動位置。每個 `BindingDefinition` 暴露原始 `concrete` 工廠及宣告的 `shared`，明確執行個體覆寫不改變該旗標。別名單獨鍵、僅執行個體鍵、情境綁定及自動類別中繼資料不在其中。讀取或修改快照不會解析服務或修改容器。

保留的 `FactoryHandle` 對應直接執行 Laravel 綁定陣列中的 concrete 閉包。`call::<T>(&container, parameters)` 與 `callAsync::<T>(...)` 使用傳入容器及明確參數框架，不加入目標類別情境。它們繞過目標快取、裝飾器、事件和解析歷史；工廠內部相依仍正常解析，使用新參數框架。因此共享原工廠每次也產生新結果。輸出型別錯誤在執行使用者程式碼前拒絕；同步呼叫非同步工廠會失敗，即使一般解析已快取結果。`sameFactory` 比較保留配方的身分。

```rust
use openrust::{
    container::{Container, ParameterOverrides, ServiceKey},
    contracts::container::Container as _,
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.singleton::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    container.extend::<u32, _>(ServiceKey::typed::<u32>(), |value, _| {
        Ok(Arc::new(*value + 10))
    }).unwrap();
    let definitions = container.getBindings();
    let definition = definitions.get(&ServiceKey::typed::<u32>()).unwrap();
    assert!(definition.shared);
    assert_eq!(*definition.concrete.callAsync::<u32>(
        &container, ParameterOverrides::new(),
    ).await.unwrap(), 1);
    assert!(!container.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert_eq!(*container.make::<u32>().unwrap(), 11);

    container.flush();
    assert!(container.getBindings().is_empty());
    assert!(!container.bound::<u32>());
    assert_eq!(*definition.concrete.call::<u32>(
        &container, ParameterOverrides::new(),
    ).unwrap(), 1);
    container.bind::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    assert_eq!(*container.make::<u32>().unwrap(), 12);
}
```

`flush()` 清空綁定、別名及反向別名、明確/共享/scoped 執行個體、scoped 登記和解析歷史。按 Laravel 基礎 Container 保留情境規則、標籤、裝飾器、解析/重新綁定回呼、方法宣告/綁定及編譯類別中繼資料。同時解除本容器所有活動生命週期的快取，不關閉這些生命週期。使用者值於狀態提交後、登錄表與快取鎖外釋放，允許解構重入。flush 與正常生命週期結束均在中立的 Facade、參數及類別情境中釋放 scoped 快取值。解構 panic 不回滾清理，也不污染登錄表鎖。

應用自行持有的物件及原工廠快照仍有效，已准入建構可回傳舊值，但不能回填已移除或替換的綁定及其歷史。此基礎容器操作不重新啟動 Application Provider，也不清除獨立 Facade 快取；應用終止由生命週期職責負責。

Rust 使用型別化定義與擁有所有權的 ServiceKey 替代動態 PHP 陣列。快照查詢按定義數量線性查找，屬於明確檢查，不加入一般請求解析路徑。為支援並行請求的全域 flush，每個生命週期在登記/註銷時以短 Mutex 操作維護 Weak 參考，flush 遍歷活動記錄。讀取已預熱共享綁定僅保留配方，不保留快取物件。未做效能基準，不宣稱零成本。

## 環境解析回呼

`resolveEnvironmentUsing(Some(Arc::new(callback)))` 安裝基礎容器環境解析回呼，`None` 清除。無回呼時，`currentEnvironmentIs(query)` 回傳 `Ok(EnvironmentResult::Boolean(false))`。基礎容器不解析環境檔案、不比對萬用字元，也不重新解釋空參數，而是原樣轉交 `EnvironmentQuery::Name` 或 `Names`，保留陣列順序、重複項與空字串。同步回呼借用查詢字串，不複製參數。

回呼可回傳 `EnvironmentResult::Boolean` 或 `String`，保留 PHP 支援的兩種結果形狀；`Result` 將回呼例外作為容器錯誤傳播。`bool::from(result)` 或 `bool::from(&result)` 按 PHP 真值轉換，false、空字串及字串 "0" 為假。範例刻意採用明確自訂策略：空陣列回傳名稱，並不代表 Laravel Application 預設環境比對規則。

```rust
use openrust::{
    container::{Container, EnvironmentQuery, EnvironmentResult},
    contracts::container::Container as _,
};
use std::sync::Arc;

fn main() {
    let container = Container::default();
    assert_eq!(container.currentEnvironmentIs("local").unwrap(), false.into());
    container.resolveEnvironmentUsing(Some(Arc::new(|query| {
        Ok(match query {
            EnvironmentQuery::Name(name) => (name == "testing").into(),
            EnvironmentQuery::Names([]) => "testing".into(),
            EnvironmentQuery::Names(names) => names.contains(&"testing").into(),
        })
    })));
    assert!(bool::from(container.currentEnvironmentIs("testing").unwrap()));
    assert!(bool::from(container.currentEnvironmentIs(&["local", "testing"]).unwrap()));
    assert_eq!(container.currentEnvironmentIs(&[]).unwrap(),
        EnvironmentResult::from("testing"));
    container.flush();
    assert!(bool::from(container.currentEnvironmentIs("testing").unwrap()));
    container.resolveEnvironmentUsing(None);
    assert_eq!(container.currentEnvironmentIs("testing").unwrap(), false.into());
}
```

回呼在登錄表鎖外執行，可以重入或替換解析器；進行中的呼叫保留原回呼直到回傳。舊捕獲在鎖外釋放，錯誤與 panic 不污染登錄表鎖。更換或清除解析器不驅逐服務快取、不觸發解析事件，`flush` 保留解析器，各容器策略獨立。

Rust 用借用查詢列舉、結果列舉、Result 與可選 Arc 回呼對應 PHP 聯合型別、例外和可空 callable。回呼要求 Send + Sync，以支援同容器並行請求。環境檢查僅在短讀鎖內複製回呼參考，未做效能基準。Application 預設偵測、LoadConfiguration 接線及主控台 --env 支援記為 CT-ENV-APPLICATION，須在環境類別屬性前繼續接通。
