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

## 查詢模型

在應用程式請求或指令中使用 `Flight::query()?` 取得模型查詢建構器，使用 `Flight::all().await?` 取得套用作用域後的模型集合，或用 `Flight::find(id).await?` 依主鍵查找。模型不存在時，`findOrFail` 回傳 `DatabaseError::ModelNotFound`。可用 `queryOn` 指定資料庫連線。

## 插入和更新模型

`create` 依批次賦值規則儲存新模型。`setAttribute` 設定單一屬性，`save` 以原主鍵寫入已變更的屬性。寫入成功後同步模型快照。建構器的 `update` 和 `delete` 回傳受影響列數。

## 刪除模型

使用 `softDeletes("deleted_at")` 啟用軟刪除。模型查詢預設排除軟刪除記錄，可用 `withTrashed` 或 `onlyTrashed` 查詢這些記錄，用 `restore` 清除刪除時間，用 `forceDelete` 永久刪除。

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
