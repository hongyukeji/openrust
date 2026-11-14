# Eloquent 入門

## 產生模型類別

執行 `./artisan make:model Flight` 產生 `app/models/flight.rs`。在 `app/models/mod.rs` 宣告 `mod flight;`，並在 `app/mod.rs` 匯出 `models` 模組後重新建置。使用 `--force` 可覆寫產生的檔案。Rust 模型在 `ModelMetadata` 宣告資料表，每個實例持有一個 `ModelState`。

```rust
use openrust::database::eloquent::{Model, ModelMetadata, ModelState};

pub struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("flights")
            .fillable(&["name", "active"])
            .softDeletes("deleted_at");
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
```

## 模型慣例

預設主鍵為遞增的 `id`，並維護 `created_at` 和 `updated_at`。在 `ModelMetadata` 中透過 `primaryKey`、`incrementing`、`timestamps` 或 `connection` 設定模型。使用 `fillable` 宣告可批次賦值的欄位。

### UUID 與 ULID 主鍵 {#uuid-and-ulid-keys}

使用 `UniqueId::Uuid` 產生 UUIDv7，或使用 `UniqueId::Ulid` 產生小寫 ULID。預設產生主鍵，並將其視為非自動遞增字串。使用 `uniqueIds` 指定產生欄位。插入時，Eloquent 在 `Saving` 之後、`Creating` 之前填入空值，保留已提供的非空值。複製時排除這些欄位，新記錄插入時重新產生識別碼。這些欄位的路由模型繫結遇到格式錯誤會回傳 404。

```rust
use openrust::database::eloquent::{ModelMetadata, UniqueId};

static TICKET: ModelMetadata = ModelMetadata::new("tickets")
    .uniqueId(UniqueId::Uuid)
    .uniqueIds(&["id", "reference"]);
```

### 寫入後重新整理屬性 {#refreshing-attributes-after-writes}

透過 `ModelMetadata::refreshes` 宣告由資料庫管理的欄位。插入或實際更新後，Eloquent 從寫入連線讀取這些欄位，再呼叫 `Created` 或 `Updated` 監聽器。顯式交易在同一交易內回讀。複製模型時排除這些欄位，由資料庫為新列計算值。

```rust
use openrust::database::eloquent::ModelMetadata;

static ARTICLE: ModelMetadata = ModelMetadata::new("articles")
    .fillable(&["title"])
    .refreshes(&["title_length"]);
```

## 查詢模型

在應用程式請求或指令中使用 `Flight::query()?` 取得模型查詢建構器，使用 `Flight::all().await?` 取得套用作用域後的模型集合，或用 `Flight::find(id).await?` 依主鍵查找。模型不存在時，`findOrFail` 回傳 `DatabaseError::ModelNotFound`。可用 `queryOn` 指定資料庫連線。

### 重新整理模型 {#refreshing-models}

`fresh()` 重新讀取獨立實例，`freshWithPaths(&["posts.comments"])` 也能預先載入已登記的路徑。`refresh()` 更新目前的實例，並重新載入已載入的登記關聯。兩者都按原主鍵從寫入連線讀取，略過全域作用域並包含軟刪除記錄。未儲存的模型呼叫 `fresh()` 會得到 `None`，呼叫 `refresh()` 則維持原狀。資料庫記錄已移除時，`refresh()` 回傳 `ModelNotFound`。需要重新載入的關聯須在 `namedRelations()` 登記，中介資料表的 Pivot 投影會保留。重新載入關聯失敗時，目前的屬性保留剛讀取的值，原始快照維持先前的值。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn reload<M: Model>(model: &mut M) -> DatabaseResult<Option<M>> {
    let copy = model.fresh().await?;
    model.refresh().await?;
    Ok(copy)
}
```

用 `refreshForUpdate(&mut transaction)` 在交易內重新整理模型。MySQL 資料列鎖由呼叫端的交易持有，提交或回滾時釋放。交易須從模型所屬連線開啟。同連線的已載入關聯及其讀取事件可以看到交易內尚未提交的變更。跨連線關聯使用各自的連線。自訂關聯可實作 `matchResultsInTransaction` 參與交易內載入。SQLite 在交易內讀取，遵循 SQLite 的交易鎖規則。

```rust
use openrust::database::{DatabaseResult, Transaction, eloquent::Model};

pub async fn reload_locked<M: Model>(
    model: &mut M,
    transaction: &mut Transaction,
) -> DatabaseResult<()> {
    model.refreshForUpdate(transaction).await?;
    Ok(())
}
```

## 插入和更新模型

`create` 依批次賦值規則儲存新模型。`setAttribute` 設定單一屬性，`save` 以原主鍵寫入已變更的屬性。寫入成功後同步模型快照。建構器的 `update` 和 `delete` 回傳受影響列數。

### 更新計數器 {#updating-counters}

使用 `increment`、`decrement`、`incrementEach` 或 `decrementEach` 原子更新數值屬性。多欄方法接收欄名與步長的映射，回傳受影響的列數。已持久化的模型會觸發 `Updating` 和 `Updated`，並讀回寫入的屬性。未持久化的實例將操作套用至帶有作用域的查詢。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model}};

pub async fn record_visit<M: Model>(model: &mut M) -> DatabaseResult<u64> {
    model.incrementEach(
        Attributes::from([("visits".into(), 1.into()), ("views".into(), 2.into())]),
        Attributes::new(),
    ).await
}
```

更新實例時，可使用帶有 `Quietly` 後綴的方法抑制模型事件。Eloquent Builder 提供同名的四個運算方法，依查詢作用域批次更新。批次更新會寫入模型的更新時間，呼叫方可以明確提供時間戳記。批次更新不會派發個別模型事件。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model}};

pub async fn count_visit_quietly<M: Model>(model: &mut M) -> DatabaseResult<u64> {
    model.incrementQuietly("visits", 1_i64, Attributes::new()).await
}
```

在 Eloquent 查詢上使用 `fillAndInsert(rows)`，批次插入前會套用模型預設值、查詢預設屬性、修改器和轉換。建構器補齊缺少的唯一識別碼與時間戳記，保留明確提供的時間戳記，並略過批次賦值保護和模型持久化事件。`fillAndInsertOrIgnore(rows)` 使用驅動程式的忽略插入行為，`fillAndInsertGetId(row)` 回傳單筆記錄由資料庫產生的 ID。`fillForInsert(rows)` 僅回傳準備好的屬性。

## 刪除模型

使用 `softDeletes("deleted_at")` 啟用軟刪除。模型查詢預設排除軟刪除記錄，可用 `withTrashed` 或 `onlyTrashed` 查詢這些記錄，用 `restore` 清除刪除時間，用 `forceDelete` 永久刪除。

## 複製模型 {#replicating-models}

呼叫 `replicate(&[]).await?` 建立尚未儲存的副本。框架會排除主鍵、已設定的時間戳記和內部中介資料表鍵。可傳入其他要排除的欄位名稱。副本保留已載入的關聯，也可以單獨移除關聯。副本沿用來源模型的資料庫連線並觸發 `Replicating` 生命週期事件。使用 `replicateQuietly` 跳過該事件，呼叫 `save` 插入副本。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

async fn copy_without_secret<M: Model>(source: &M) -> DatabaseResult<M> {
    source.replicate(&["secret"]).await
}
```

## 查詢作用域

實作 `Scope<Model>` 可重複使用查詢條件。用 `withScope` 套用它，或在模型的 `globalScopes` 註冊全域作用域。使用 `withoutGlobalScope` 移除指定作用域。以下範例使用上面的 `Flight` 模型，資料表需包含已宣告欄位和時間戳記。

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Attributes, Builder, Scope},
};

struct Active;

impl Scope<Flight> for Active {
    fn apply(&self, query: &mut Builder<'_, Flight>) -> DatabaseResult<()> {
        query.r#where("active", "=", true);
        Ok(())
    }
}

pub async fn save_flight(connection: &Connection) -> DatabaseResult<()> {
    let mut flight = Flight::queryOn(connection)
        .create(Attributes::from([
            ("name".into(), "Flight 42".into()),
            ("active".into(), true.into()),
        ]))
        .await?;
    flight.setAttribute("name", "Flight 42A");
    flight.save().await?;

    let mut flights = Flight::queryOn(connection);
    flights.withScope(&Active)?;
    assert_eq!(flights.get().await?.len(), 1);

    flight.delete().await?;
    assert!(flight.trashed());
    flight.restore().await?;
    Ok(())
}
```

## 在交易中寫入模型 {#model-transactions}

將同一可變交易傳給 `saveInTransaction`、`updateInTransaction`、`deleteInTransaction`、`forceDeleteInTransaction` 或 `restoreInTransaction`。由呼叫端提交或回復。已有來源連線的模型必須使用該連線的交易。監聽器的 SQL 若要加入同一交易，應實作 `ModelEventListener::handleInTransaction` 並透過收到的交易執行。現有以連線為基礎的回呼繼續接收池化連線。回復會撤銷資料庫寫入。已反映本次寫入的模型實例應捨棄或重新查詢。

```rust
use openrust::database::{Connection, DatabaseResult, eloquent::Model};

async fn save_in_transaction<M: Model>(
    connection: &Connection,
    model: &mut M,
) -> DatabaseResult<bool> {
    let mut transaction = connection.beginTransaction().await?;
    match model.saveInTransaction(&mut transaction).await {
        Ok(true) => {
            transaction.commit().await?;
            Ok(true)
        }
        Ok(false) => {
            transaction.rollBack(None).await?;
            Ok(false)
        }
        Err(error) => {
            transaction.rollBack(None).await?;
            Err(error)
        }
    }
}
```

## 模型事件 {#model-events}

應用程式啟動時用 `Model::registerModelEvent` 註冊型別化監聽器。監聽器可以在寫入前修改模型，也可回傳 `ModelEventResponse::Cancel` 取消寫入。成功儲存會觸發 `Saved`，包括屬性未變更的情況。查詢建構器的批次更新與刪除不派送實例事件。使用 `saveQuietly`、`deleteQuietly`、`forceDeleteQuietly`、`restoreQuietly` 或 `Model::withoutEvents` 在目前非同步呼叫鏈內抑制模型事件。寫入後監聽器的錯誤會回傳給呼叫端，此時資料庫寫入已執行。

```rust
use openrust::{
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{
            Model,
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
        },
    },
    foundation::Application,
};

struct RequireTitle;

impl<M: Model> ModelEventListener<M> for RequireTitle {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            Ok(match model.getAttribute("title") {
                Some(openrust::database::Value::Text(title)) if !title.trim().is_empty() => {
                    ModelEventResponse::Continue
                }
                _ => ModelEventResponse::Cancel,
            })
        })
    }
}

fn register<M: Model>(app: &Application) -> DatabaseResult<()> {
    M::registerModelEvent(app, ModelEvent::Saving, RequireTitle)
}
```

### 對應模型事件 {#mapping-model-events}

實作 `dispatchesEvents`，依模型目前的生命週期階段建構事件，透過[應用程式派發器](../digging-deeper/events.md)註冊相應監聽器。寫入前監聽器可回傳 `ListenerResponse::Value(Box::new(false))` 取消寫入。沒有回應時回退至一般模型監聽器及觀察者，已處理的回應優先。交易寫入會將目前交易傳給派發器。提交後監聽器收到生命週期事件觸發時建構的自有承載資料。

```rust
use std::sync::Arc;
use openrust::{
    contracts::events::Event,
    database::{Value, eloquent::{Model, ModelMetadata, ModelState, concerns::ModelEvent}},
    events::NamedEvent,
};

struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("flights").fillable(&["name"]);
        &META
    }

    fn from_state(state: ModelState) -> Self { Self(state) }
    fn state(&self) -> &ModelState { &self.0 }
    fn state_mut(&mut self) -> &mut ModelState { &mut self.0 }

    #[allow(non_snake_case)]
    fn dispatchesEvents(&self, event: ModelEvent) -> Option<Arc<dyn Event>> {
        let (ModelEvent::Saved, Some(Value::Text(name))) = (event, self.getAttribute("name")) else {
            return None;
        };
        Some(Arc::new(NamedEvent::new("flight.saved", name.clone())))
    }
}
```

### 觀察者 {#observers}

執行 `./artisan make:observer FlightObserver --model=Flight`，在 `app/observers/flight_observer.rs` 產生五個寫入後生命週期方法。宣告模組，並在服務提供者啟動時註冊觀察者。省略 `--model` 會產生一般結構體，使用 `--force` 可覆寫既有檔案。透過 `./artisan stub:publish` 發布並自訂 `observer.rs.stub` 與 `observer.plain.rs.stub`。

用 `Observer<M>` 集中實作模型生命週期方法，在服務提供者啟動時呼叫 `M::observe` 註冊。透過 `EVENTS` 指定要註冊的生命週期方法。方法接收模型及本次操作的資料庫連線，寫入前可回傳 `ModelEventResponse::Cancel` 取消寫入。註冊表由目前應用程式持有。

```rust
use openrust::{
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{Model, Observer, concerns::{ModelEvent, ModelEventResponse}},
    },
    foundation::Application,
};

struct NormalizeTitle;

impl<M: Model> Observer<M> for NormalizeTitle {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Saving];

    fn saving<'a>(
        &'a self,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if let Some(openrust::database::Value::Text(title)) = model.getAttribute("title") {
                let title = title.trim().to_owned();
                model.setAttribute("title", title);
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}

fn register<M: Model>(app: &Application) -> DatabaseResult<()> {
    M::observe(app, NormalizeTitle)
}
```

#### 觀察者與資料庫交易 {#observers-and-database-transactions}

實作 `ShouldHandleEventsAfterCommit` 並透過 `Model::observeAfterCommit` 註冊。使用顯式模型交易時，六個寫入前回呼仍即時執行，可取消寫入。其他回呼等待最外層提交成功。交易或儲存點回復時捨棄對應回呼。無交易時即時執行。Rust 要求模型可複製，並在事件觸發時保留模型狀態。延後回呼失敗時，提交回傳 `DatabaseError::AfterCommit`，資料庫寫入已經提交。

```rust
use openrust::{
    contracts::events::ShouldHandleEventsAfterCommit,
    database::{
        Connection, DatabaseFuture, DatabaseResult,
        eloquent::{Model, Observer, concerns::{ModelEvent, ModelEventResponse}},
    },
    foundation::Application,
};

struct ArticleCommitted;

impl ShouldHandleEventsAfterCommit for ArticleCommitted {}

impl<M: Model> Observer<M> for ArticleCommitted {
    const EVENTS: &'static [ModelEvent] = &[ModelEvent::Created];

    fn created<'a>(
        &'a self,
        _model: &'a mut M,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            connection.insert("INSERT INTO article_audit (action) VALUES (?)", &["created".into()]).await?;
            Ok(ModelEventResponse::Continue)
        })
    }
}

fn register<M: Model + Clone>(app: &Application) -> DatabaseResult<()> {
    M::observeAfterCommit(app, ArticleCommitted)
}
```
