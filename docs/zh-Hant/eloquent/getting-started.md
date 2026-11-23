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

### 模型欄位引用（擴充） {#model-field-references}

從既有資料表自動產生模型欄位引用，不需要逐一宣告欄位。先執行應用遷移，再選擇建立或更新模型：

```sh
./artisan make:model User --fields
# 既有模型只更新欄位，保留模型業務程式碼
./artisan model:fields User
# 自訂資料表、連線與巢狀模型目錄
./artisan model:fields Admin/User --table=accounts --database=pgsql
```

產生欄位使用巨集內部中繼資料 `#[generated(command = "artisan model:fields User")]`，由既有 `model_fields!` 巨集處理；它不是編譯器屬性，也不會執行其中的命令。產生器透過 Rust 語法定位整個巨集、驗證模型歸屬並更新該項，保留外部業務程式碼。不再產生 begin/end 註解或大段說明區塊；舊標記與舊區塊註解配置會在更新時升級。

新模型仍需依 `make:model` 提示在父 `mod.rs` 宣告模組。重新建置後，`model:fields` 從空模型的 `getTable()` 與 `resolveConnectionName()` 讀取表名與連線，無須反覆傳參。單一模型的 `--table`、`--database` 可覆寫本次結構讀取。更新會將標準 `metadata()` 靜態初始化中的表名字面值替換為 `User::TABLE`，其餘模型設定維持原文；明確指定 `--table` 會更新產生的表名常數，重新建置後標準模型使用該表。自訂 `getTable()`、動態運算式與非標準初始化不會被改寫。建立時 `--table` 與 `--database` 會同時儲存至新模型中繼資料，後者須搭配 `--fields`；已發佈的自訂模型範本使用 `{{connection}}` 預留位置接收連線設定。

新模型產生後尚未重新建置時，更新會共用產生區域中已儲存的 `TABLE`。首次更新尚未登記的舊模型時，優先讀取標準 `metadata()` 初始化中的資料表字串，否則按模型名轉底線並取複數推斷；連線使用應用預設值，自訂連線及動態表名須明確指定。隨後重新建置，後續同步即可讀取模型設定。修改模型表名、連線或模組宣告後，應先重新建置指令程式；原始碼表名字串與已編譯模型不一致時，命令拒絕隱式同步，須重新建置或明確指定 `--table`。自訂動態設定仍以已編譯模型為準。

只刪除生成的說明時，只要欄位巨集完整保留，再執行 `./artisan model:fields User` 即可恢復。刪除整個巨集會同時移除模型正在引用的常數；`./artisan` 透過 `cargo run` 先編譯應用程式，因此會在命令執行前失敗。若保留了本專案已編譯的應用程式，可直接執行它恢復生成區（例如保留的二進位檔名為 `main` 時使用 `./main artisan model:fields User`），然後重新建置。設定 `APP_BASE_PATH` 指向原始碼專案目錄。舊產物中的模型設定須與目前目標一致，必要時明確指定 `--table`、`--database`；沒有可用產物時，先從版本控制恢復宣告。指令稿不會在編譯失敗後靜默改用舊程式。

```sh
# 明確選擇多個模型
./artisan model:fields User Admin/User
# 同步全部已編譯且啟用自動欄位同步的應用模型
./artisan model:fields --all
# CI 檢查：結果相同回傳 0，過期回傳 1，不修改模型原始碼
./artisan model:fields --all --check
```

`--all` 只包含 `app/models/` 約定目錄中已編譯並攜帶自動產生區域的模型，不掃描全部資料表，也不自動發現任意 trait 實作或套件模型。首次接入、舊產生區域升級或新增模型後，先逐一更新、宣告模組並建置。批量同步使用各模型自己的設定，不接受共用的表或連線覆寫。指令先預檢全部模型，再逐一原子替換；預檢失敗不寫入模型，寫入階段的磁碟錯誤可能留下部分已完成結果，修復後可重複執行。`--check` 不建立原始碼鎖或原始碼暫存檔；缺表、無效連線、損壞的生成分區也回傳非零並說明錯誤，應用啟動自身的日誌或快取仍沿用原行為。

產生後即可使用 IDE 補全：

```rust
let mut query = User::query()?;
query.r#where(User::NAME, "=", "張三");
let users = query.orderBy(User::ID, "asc").get().await?;
```

資料庫目前的欄位結構是產生集合的來源，`fillable` 與 `casts` 仍各自控制批量賦值與屬性轉換。命令將自動產生欄位區放在模型檔案的匯入之後、型別定義之前，使用巨集內部產生中繼資料和簡潔宣告 `openrust::model_fields! { User { TABLE = "users"; NAME = "name"; } }`。重新整理時自動將舊標記區域遷移到新配置，保留模型業務程式碼；舊區域混入普通業務項、附加巨集屬性或其他模型宣告時拒絕覆寫；將產生的原始碼一併納入版本管理，一般建置、請求與部署無須連接資料庫。不要手改產生區域；遷移新增、刪除或重新命名欄位後，重新執行同步命令、修正受影響的引用並建置。未同步時，編譯器不會自動感知外部資料庫變更。不需要此能力的動態模型與字串查詢維持原用法；一般 `make:model` 不存取資料庫，也不產生表名與欄位常數。

產生順序固定為表名 `TABLE` 在前，其後保留資料庫實際欄位順序，不按名稱排序，也不單獨提前主鍵。`User::TABLE` 可傳給既有查詢入口，模型中繼資料透過 `ModelMetadata::new(User::TABLE)` 共用表名；資料表別名仍使用既有查詢介面。`TABLE` 儲存模型設定中的邏輯表名（可含 schema），連線前綴繼續由既有機制套用，不會重複寫入常數。

欄位名稱轉為大寫底線常數，保留實際資料庫欄名，例如 `display-name` → `DISPLAY_NAME`、`1st` → `COLUMN_1ST`。`TABLE` 保留給表名，資料庫的 `table` 欄位產生 `COLUMN_TABLE`；它與實際 `column_table` 欄位重名時預檢失敗。轉換後重名、非 ASCII 欄名或無法形成識別字時，命令失敗並保留原檔；缺表、缺連線或產生分區損壞也不會覆蓋模型。特殊欄位可繼續使用字串，或以進階 `model_fields!` 巨集做明確映射。由舊手寫宣告遷移時先移除重複宣告；自動產生不會刪除手寫常數。產生區域外已有手寫 `TABLE` 時也會拒絕寫入，請先消除重複定義。

常數實際型別是 `&'static str`。不存在或同步後已刪除的常數引用會編譯失敗；**不檢查查詢模型歸屬、查詢值型別或建置時的資料庫結構**。NULL、數值、日期、JSON、自訂 cast 與運算式沿用既有規則，沒有新增值型別保證。

欄名不會自動附帶表名。選擇欄位使用 `select([User::NAME])`；更新使用 `[(User::NAME.into(), value)].into()` 作為屬性映射；關聯鍵直接傳常數。聯表或表別名可用 `format!("u.{}", User::NAME)`，再傳給既有查詢介面。欄名仍由語法器作為識別字處理，值繼續參數繫結；不要把常數拼入 `whereRaw` 等原始 SQL。動態欄位與原始運算式維持原入口。資料庫存取與產生邏輯位於框架模組，模型僅保存對應宣告和自身行為；跨模型的業務流程仍由應用服務組織。

資料庫欄位註解會產生為 Rust 文件註解 `///`（需要跳脫控制字元時使用 `#[doc = "…"]`），IDE 懸停即可查看欄位描述。MySQL、PostgreSQL 透過既有結構介面提供欄位註解；SQLite 不儲存原生欄位註解。缺少或空白註解時不產生描述。修改欄位註解後也需重新整理，`--check` 會偵測這類變化。描述直接取自資料庫，不根據欄位名稱猜測，也不在產生時翻譯。

`TABLE` 上方直接顯示資料庫表註解，不添加前綴；沒有註解或註解為空白時使用目前語言的預設說明，例如「表名」。表註解變更後也需同步，`--check` 會偵測這類變化。

`where` 是 Rust 關鍵字，因此方法呼叫寫作 `r#where`。原始識別字保留既有方法名稱；`query.where(...)` 會在 Rust 解析階段報錯。

宣告重複、產生中繼資料無效、模型歸屬錯誤或舊區域有歧義時，拒絕修改原始碼。業務方法請放在產生巨集之外。

`DB_PREFIX` 由設定的連線套用。例如 `DB_PREFIX=tenant_` 時，`User::TABLE` 仍為 `"users"`，結構讀取和查詢實際使用 `tenant_users`；不要再把前綴寫入常數。

Artisan 提示與 `TABLE` 的預設說明透過統一譯器及 `lang/<locale>/console.json` 讀取；`model_fields.table` 控制沒有表註解時的說明。產生中繼資料鍵與命令語法不隨語言變化；資料庫描述保持原文。

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

### 嚴格屬性策略

在應用啟動時透過 `Flight::shouldBeStrict(&app, true)?` 為模型開啟嚴格策略，也可分別呼叫 `preventLazyLoading(&app, enabled)`、`preventSilentlyDiscardingAttributes(&app, enabled)`、`preventAccessingMissingAttributes(&app, enabled)`。預設關閉，設定屬於目前應用和模型型別；脫離應用的模型需要透過 `newInstanceOn` 關聯應用連線，或在目前應用上下文中使用策略。

嚴格批量賦值遇到被丟棄欄位時回傳 `DatabaseError::MassAssignment`，此前已賦值的欄位保留在實例上。`forceFill`、`forceFillAsync` 和 `unguarded` 繼續繞過保護。可覆寫 `handleDiscardedAttributeViolationUsing` 處理被丟棄欄位，回傳成功表示允許丟棄。

使用 `getAttributeValue` 讀取需要檢查的屬性。開啟防護後，已檢索模型缺少屬性時回傳 `DatabaseError::MissingAttribute`；明確讀取到的 SQL NULL、存取器、未儲存模型和剛建立模型仍依其正常行為處理。可覆寫 `handleMissingAttributeViolationUsing` 為未知屬性提供值；原生或列舉轉換器在處理鉤子成功後繼續依空值轉換。`getAttribute` 和 `getRawOriginal` 保留底層 `Option` 讀取。延遲載入策略在水合多模型結果時儲存到實例，由隱式關係讀取檢查；切換預設值影響後續水合，模型明確覆寫的 `preventsLazyLoading` 仍然生效。

### 暫時停用關聯時間戳記更新

`Owner::withoutTouching(work).await` 在 Future 執行期間抑制該模型型別作為關聯目標的時間戳記更新。匯入 `std::any::TypeId` 後，可用 `Owner::withoutTouchingOn(&[TypeId::of::<Owner>(), TypeId::of::<Team>()], work).await` 指定多個型別。`isIgnoringTouch()` 查詢是否停用關聯時間戳記更新；沒有更新時間戳記的模型也回傳 true。儲存子模型仍寫入其自身屬性並派發模型事件，關聯 Saved 事件和後續關聯遍歷保持正常行為。巢狀作用域在成功、錯誤、恐慌或取消後恢復此前設定，不影響其他任務。

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
