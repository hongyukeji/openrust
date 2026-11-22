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

`getFillable()` 和 `getGuarded()` 回傳模型宣告的大量賦值名單。`Model::isUnguarded()` 查詢目前略過保護的作用域；`Model::unguarded(future)` 在完成、失敗或取消時恢復保護，不影響其他任務。`getDeletedAtColumn()` 回傳設定的軟刪除欄位 `Option<&str>`，未啟用軟刪除時為 `None`。

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

### 游標 {#cursors}

呼叫 `cursor()` 可在輪詢時逐列水合模型。模型作用域、屬性轉換與 `Retrieved` 事件仍然生效。需要預先載入關聯時，請使用分批查詢。讀取期間游標會占用一個資料庫連線。如果讀取事件監聽器也要查詢，需要另一個可用連線。

```rust
use futures_util::TryStreamExt as _;
use openrust::database::{DatabaseResult, eloquent::Model};

async fn first_model<M: Model>() -> DatabaseResult<Option<M>> {
    let mut models = M::query()?.cursor();
    models.try_next().await
}
```

### 分批讀取結果 {#chunking-results}

Eloquent 查詢的 `chunk(count, callback)` 將每頁作為模型集合交給非同步回呼。沒有指定排序時按主鍵排序。回呼執行前，每批都會觸發模型讀取事件、預先載入關聯並呼叫模型集合工廠。參見[查詢建構器的分批規則](../database/queries.md#chunking-results)。

Eloquent 也支援 `chunkById` 和 `chunkByIdDesc`。傳入 `None` 時以模型主鍵分批。後續查詢的邊界值來自 SQL 投影，請保留分批欄位或其投影別名。每批仍會執行 Eloquent 的讀取事件及預先載入。

Eloquent 的 `each` 將模型及其目前批次中的 `ArrayKey` 傳給回呼，保留自訂集合鍵。`eachById` 將批次偏移加到整數集合鍵上。集合鍵無法表示為有號整數時回傳錯誤。

Eloquent 的惰性方法在回傳模型前按批水合並預先載入。每批在使用模型前釋放資料庫連線。`chunkMap` 限制每次讀取的輸入模型數量，回傳的集合包含全部映射結果。

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

`withGlobalScope(name, scope)` 為目前查詢註冊或替換作用域，接收 `Scope<M>` 實作或回傳 `DatabaseResult<()>` 的 `Send + Sync + 'static` 閉包。作用域的 `extend` 鉤子在註冊時執行，`apply` 在套用作用域時執行。`withoutGlobalScopes()` 移除目前已註冊的作用域，之後重新註冊的仍會生效；`withoutGlobalScopesExcept(&["tenant"])` 僅保留指定名稱。`removedScopes()` 保留移除呼叫的順序和重複名稱。註冊表屬於目前查詢，不會改變其他查詢或模型型別。

軟刪除使用相同註冊表。移除其他作用域時需要保留軟刪除篩選，可匯入 `eloquent::SoftDeletingScope` 並將 `std::any::type_name::<SoftDeletingScope>()` 加入保留名單。`onlyTrashed()` 或 `withoutTrashed()` 明確加入的條件不會因移除全部已註冊作用域而消失。

`applyScopes()` 消費模型查詢構造器並回傳已套用作用域的構造器，重複呼叫會再次套用目前註冊項。`getQuery()` 消費模型查詢構造器並回傳目前底層查詢，不自動套用尚未執行的作用域；需要這些約束時呼叫 `applyScopes()?.getQuery()`。`setQuery(query)` 替換底層查詢及其舊條件，保留模型作用域與預載入設定。`getEagerLoads()` 借用型別化的 `EagerLoads<M>`，可用 `names()` 讀取關係名稱；`setEagerLoads(loads)` 替換設定，也可設為空。覆寫 `Model::newEloquentBuilder(query)` 並用 `Builder::fromQuery(query)` 構造，可自訂公開模型查詢使用的構造器。工廠回傳框架的型別化 Builder，額外業務方法使用 Rust trait 或包裝型別擴充，不使用執行期類名。

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

## 主鍵與單值查詢 {#keys-and-single-values}

`whereKey`、`whereKeyNot`、`orWhereKey`、`orWhereKeyNot` 接收單值、`Vec` 或陣列。整數主鍵列表使用整數條件，字串主鍵列表使用綁定值；正向空列表不符合任何列，反向空列表符合全部列。主鍵欄位帶模型表限定，OR 條件仍受全域作用域約束。字串主鍵模型應覆寫 `getKeyType()` 回傳 `ModelKeyType::String`，此時單值主鍵依字串轉換。傳模型實例時，明確傳入其 `getKey()` 的值。動態條件可用 `eloquent::Key` 表示單值或列表。

```rust
let mut query = User::query()?;
query.whereKey([1, 2, 3]).orWhereKeyNot(4);
let users = query.get().await?;
```

`value(column)` 回傳首個模型的屬性，無結果回傳 `Value::Null`。`valueOrFail(column)` 在無結果時回傳模型不存在錯誤。`sole()` 和 `soleValue(column)` 要求恰好一列：零列回傳模型不存在錯誤，多列回傳 `DatabaseError::MultipleModelsFound`。這些介面經過模型水合，保留存取器、型別轉換、檢索事件與全域作用域。已選欄位不會被覆蓋；只選擇 `id` 後查詢其他屬性，不會自動補查。實際列的欄位為 SQL NULL 時，`valueOrFail` 仍回傳 `Value::Null`。

## 儲存與刪除變體 {#write-variants}

`saveOrFail()` 在模型連線上開啟交易。`updateOrFail(attributes)` 先填入已存在的模型，再在交易中儲存；`deleteOrFail()` 在交易中刪除已存在的模型。事件取消回傳 `false`，SQL 或監聽器錯誤回滾寫入並向上傳播；非同步任務取消也經現有機制釋放並回滾交易。資料庫回滾不會還原模型記憶體中的屬性或存在標記，應捨棄或重新讀取受影響實例。需要共用外層交易時，將同一個可變交易傳給 `saveInTransaction`、`updateInTransaction` 或 `deleteInTransaction`；自訂交易儲存邏輯也在這些型別化鉤子中實作。

`updateQuietly(attributes)` 先填入已存在的模型，再抑制實例事件儲存。兩個更新變體對不存在的模型在填入前回傳 `false`；`deleteOrFail` 此時也不會開啟交易。

`saveOrIgnore(None)` 插入新模型並忽略唯一衝突，`Some(&["email"])` 指定衝突欄位。發生衝突回傳 `false`，不標記模型已存在，也不觸發 `Created` / `Saved`；插入成功會取得產生的主鍵並發出正常建立事件。對已存在模型呼叫會報錯。該方法復用驅動的插入忽略並回傳能力：PostgreSQL、SQLite 支援；MySQL、SQL Server 對非空插入明確回傳不支援錯誤。該方法不會自動開啟交易，也不會吞掉監聽器錯誤。關閉時間戳且沒有屬性的空模型可能回傳 `true` 而不插入列。

`original` 只在儲存後事件與關聯觸碰成功後，同步最終模型屬性；儲存後監聽器失敗時保留待儲存變化，監聽器成功修改的屬性也納入同步。
