# 資料庫設定（擴充）

`openrust-settings` 將型別化應用設定存入資料庫，可用於網站資訊、服務設定及業務策略等群組。重用 OpenRust 的資料庫、加密、遷移與容器作用域。

## 啟用與遷移

使用本倉庫應用骨架時，在 `Cargo.toml` 的既有預設 feature 清單加入 `settings`（保留其他預設項目）。骨架已宣告 `settings = ["dep:openrust-settings"]`：

```toml
[features]
default = ["settings"]
```

這樣後續 `./artisan`、應用建置與 Worker 都會啟用此套件。單次 `cargo run --features settings` 不會儲存 feature 選擇；若繼續按需啟用，每次建置／執行都必須帶上 `--features settings`。

套件探索登記自身 Provider 與預設資料表遷移，不註冊應用業務設定群組：

```sh
./artisan migrate
```

macOS／Linux 使用 `./artisan`；Windows CMD／PowerShell 使用 `.\artisan.bat`。正式環境自動部署遵循既有遷移確認機制，例如 `./artisan migrate --force`，執行前確認目標資料庫。

套件本身提供預設設定，預設應用不包含 `config/settings.rs`。需要自訂時執行 `./artisan vendor:publish --tag=settings-config`，在 `config/mod.rs` 的模組宣告與設定映射中分別加入 `mod settings;` 與 `("settings".to_owned(), settings::load(env)?),`，然後重新建置。修改資料表或連線時保留自己的對應遷移。

其他應用加入 `openrust-settings` 相依套件並使用 OpenRust 套件探索。宣告自己的設定型別，在套件註冊完成後，由該應用的 Provider 呼叫 `SettingsServiceProvider::register_settings::<YourSettings>(app).await?`。預設應用沒有預先註冊的 `SiteSettings` 業務群組。首次讀取前執行遷移；Provider 註冊與啟動不查詢設定表。

`settings.repository` 預設為 `database`；`settings.repositories.database.connection` 預設使用應用資料庫連線，`table` 預設為 `settings`。自訂連線或資料表時，登記對應的 `CreateSettingsTable` 遷移並在該連線執行，取代預設遷移。遷移仍使用既有紀錄及執行器。

## 宣告與讀寫

為衍生 `Clone`、`Serialize`、`Deserialize` 的結構實作 `SettingsGroup`。`group()` 宣告唯一群組名稱，`defaults(config)` 回傳完整預設物件，`validate()` 驗證業務限制，`encrypted()` 宣告敏感的頂層屬性。序列化欄位集合須穩定，不要依值省略欄位。群組與屬性名稱使用不帶限定詞的 ASCII 識別字，最多 128 位元組；屬性名稱前綴 `__settings_` 保留供內部使用。

### 定義型別並註冊

建立 `app/settings.rs`，寫入完整的設定群組定義：

```rust
use openrust::contracts::config::Repository;
use openrust_settings::{SettingsGroup, SettingsResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SiteSettings {
    pub title: String,
    pub registration_enabled: bool,
}

impl SettingsGroup for SiteSettings {
    fn group() -> &'static str { "site" }

    fn defaults(config: &dyn Repository) -> SettingsResult<Self> {
        Ok(Self {
            title: config.get("app.name")
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "My Site".into()),
            registration_enabled: false,
        })
    }
}
```

在 `app/mod.rs` 加入模組宣告，保留既有模組：

```rust
pub(crate) mod settings;
```

在 `app/providers/app_service_provider.rs` 既有的 `impl ServiceProvider for AppServiceProvider` 中，合併所需匯入並將以下邏輯加入 `boot`，保留原有啟動邏輯。此時所有 Provider 已完成註冊，套件的設定群組註冊器可用：

```rust,ignore
use openrust::{
    foundation::Application,
    http::HttpFuture,
    support::ProviderResult,
};
use openrust_settings::SettingsServiceProvider;
use crate::app::settings::SiteSettings;

fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
    Box::pin(async move {
        SettingsServiceProvider::register_settings::<SiteSettings>(app).await?;
        Ok(())
    })
}
```

這裡只登記延遲的作用域綁定，不在啟動時查詢設定表。不要再往 `bootstrap/providers.rs` 重複登記 `SettingsServiceProvider`，也不要將解析出的 `Settings<SiteSettings>` 長期保存在單例中。新增 Rust 檔案後重新建置；原始碼 Artisan 指令碼會自動重建。

### 讀取、更新與還原預設值

下例的 `app` 是應用整合程式碼持有的已啟動 `Application`，在既有 HTTP 請求或佇列工作作用域內執行：

```rust,ignore
use openrust_settings::Settings;
use crate::app::settings::SiteSettings;

let settings = app.make::<Settings<SiteSettings>>().await?;
let current = settings.get().await;
settings.fill(serde_json::json!({"title": "我的網站"})).await?;
settings.save().await?;
assert_eq!(settings.get().await.title, "我的網站");
settings.reset("title").await?;
settings.save().await?;
```

首次讀取尚無覆寫值的群組時，`current.title` 來自 `app.name`，缺少時為 `My Site`。儲存後標題變為範例值；`reset("title")` 刪除覆寫值並還原該預設值，不刪除群組與並行版本錨點。

`get()` 回傳型別化副本，透過 `fill` 修改後明確 `save`。`fill` 拒絕未宣告欄位與錯誤型別，完整候選值驗證成功後才接受修改。巢狀物件與陣列整項取代；後台須明確合併允許修改的巢狀欄位。`reset` 刪除資料庫覆寫並使用宣告的預設值。未提交欄位保持原值；型別允許時，`null`、空字串、零與 `false` 各自保留。`refresh()` 捨棄本機編輯並重新載入。

`lock`／`unlock` 操作已持久化的屬性，呼叫前須儲存待提交編輯。修改鎖定屬性明確報錯，不會無聲成功。每組保留一筆內部 `__settings_version` 記錄，序列化寫入並偵測過期編輯，涵蓋首次建立與還原預設值；這是對 Spatie 資料表結構的明確擴充。發生 `Conflict` 後須重新讀取並處理衝突，不可盲目重試舊快照。舊版設定型別更新時保留新版新增的資料庫屬性。

模組設定使用相同介面。將型別放在模組內，在設定 Provider 註冊之後，由模組 Provider 呼叫 `SettingsServiceProvider::register_settings::<DeliverySettings>(app).await?`。該群組共用設定表，與 `site` 相互獨立，無需修改儲存庫。

```rust
use openrust::contracts::config::Repository;
use openrust_settings::{SettingsGroup, SettingsResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct DeliverySettings { enabled: bool }
impl SettingsGroup for DeliverySettings {
    fn group() -> &'static str { "module_delivery_options" }
    fn defaults(_: &dyn Repository) -> SettingsResult<Self> {
        Ok(Self { enabled: true })
    }
}
```

## 交易與生命週期

`save()` 成功提交後才更新實例的持久快照。`save_in(&mut transaction)` 在呼叫端交易內使用儲存點，不發布新的持久快照；傳入目標資料庫的交易，外層提交後呼叫 `refresh`。外層回滾後仍可重試待提交編輯。取消操作時，交易擁有者須回滾或釋放交易。

例如，設定儲存庫使用預設連線時，在相同資料庫交易內儲存；設定了其他連線時，應選擇對應連線：

```rust,ignore
use openrust::database::DatabaseManager;

let database = app.make::<DatabaseManager>().await?;
let mut transaction = database.connection(None)?.begin_transaction().await?;
settings.fill(serde_json::json!({"registration_enabled": true})).await?;
settings.save_in(&mut transaction).await?;
transaction.commit().await?;
drop(transaction);
settings.refresh().await?;
assert!(settings.get().await.registration_enabled);
```

每個群組透過處理程序內的版本快照供既有請求／工作作用域重用。使用 `SettingsServiceProvider` 時，下個作用域在核對後看見新值（預設六十秒），不再每次請求查詢完整屬性。既有作用域及尚未儲存的編輯保留原快照；`refresh()` 可略過間隔並捨棄本機編輯。長時間命令、排程輪次與串流連線須明確定義重新整理邊界；多個群組不保證全域相同快照。資料庫故障及損壞資料在檢查時明確失敗，只有缺少的覆寫值回退至預設值。

預設儲存庫每組限制 256 個屬性、1 MiB 持久 JSON，內部少量中繼資料另計入限制。無需共用快取即可使用；靜態 `Config` 契約維持，動態設定不會寫入 `config:cache`。

## 單機與叢集

單台伺服器與多個應用／Worker 處理程序使用相同的 `Settings<T>` 介面、Provider 與設定。各節點共用權威設定資料庫及資料表，不需要叢集開關、Redis 或額外守護命令。首次實際讀取啟動輕量的處理程序內檢查器；僅註冊設定群組不查詢設定表。

### 設定參考

| 設定鍵 | 預設值 | 用途 |
| --- | --- | --- |
| `repository` | `database` | 內建持久化儲存庫 |
| `repositories.database.connection` | `null` | 使用應用預設資料庫連線 |
| `repositories.database.table` | `settings` | 設定表；自訂名稱須搭配遷移 |
| `namespace` | `null` | 根據環境與資料庫／資料表推導穩定的快取及通知身分 |
| `refresh_interval` | `60` | 資料庫版本檢查間隔，單位秒；`0` 關閉自動檢查 |
| `cache.enabled` | `true` | 是否使用快取加速；關閉快取不等於關閉資料庫檢查 |
| `cache.store` | `null` | 使用既有預設 Cache 儲存，必要時指定已設定的儲存名稱 |
| `cache.ttl` | `3600` | 快照／提示有效期，單位秒（1～604800），不是重新整理間隔 |

只有覆寫預設值時才需要發布並登記 `config/settings.rs`。這些是本應用的設定項目，不是另一套分散式設定服務。修改後重新建置；部署使用設定快取時，也要重新產生 `config:cache`。

發布的 `config/settings.rs` 包含以下選用設定：

```rust,ignore
"namespace": null,
"refresh_interval": 60,
"cache": {
    "enabled": true,
    "store": null,
    "ttl": 3600,
},
```

`refresh_interval` 單位為秒（預設 60，Provider 支援 1～86400）。設為 **0** 可關閉背景檢查與已載入快照的自動到期；首次載入後持續重用快照，直到明確 `refresh()`、`poll()`、`notify()` 或本處理程序儲存更新它。處理器的自動重試也會停止，需要時主動呼叫 `poll()` 或 `notify()`；快取是否啟用獨立控制。同群組並行讀取會重用已完成的版本檢查，避免排隊重複查詢。對不頻繁修改的設定，預設一分鐘更保守，但不代表容量保證，仍應結合活動處理程序與設定群組數量評估資料庫負載。`cache.store = null` 沿用應用預設 Cache 儲存；明確選擇的儲存必須存在，`cache.enabled = false` 則停用快取加速。預設命名空間由環境、資料庫驅動、資料庫名稱、資料表前綴及名稱組成，各節點須保持一致且穩定。獨立應用共用 Cache 時，應使用不同命名空間及獨立設定表或連線。命名空間隔離快取與通知，**不隔離資料庫資料列**；預設身分無法區分部署或資料庫 Schema 時，須明確設定。

檢查器僅對本處理程序已使用的群組，從**寫入連線**批次讀取持久版本；版本改變才重讀完整屬性。既有 Cache 儲存帶版本的原始快照及僅含中繼資料的變更提示，每個命名空間／群組最多兩個鍵，不隨每次更新累積快取檔案。宣告為加密的屬性在共用快照中仍為密文。提示檢查間隔最長一秒；沒有共用快取、使用本機快取、提示遺失、快取到期或 Redis 重新啟動時，仍由資料庫定期核對補齊。這是最終一致，不保證各節點同時切換；緩慢的資料庫或應用處理器可能延長正常間隔。

資料庫提交是權威結果，快取逾時或處理器失敗不會將已提交的儲存偽裝成回滾。快取操作有逾時限制，故障期間暫時略過快取，`cache_failures()` 提供處理程序故障計數。資料庫故障使已檢查狀態失效，新載入明確失敗直到復原；既有作用域的值仍可用。新處理程序直接讀取最新提交狀態，無需重播通知。所有更新須透過儲存庫寫入介面更新版本；直接 SQL 修改而不更改版本時，須明確 `refresh()`。

`save_in` 僅在外層交易提交後發布提示，回滾不發布。提交與提示之間崩潰，由下次資料庫檢查復原。每個處理器按本處理程序觀察到的版本去重，失敗處理器重試；連續更新的中間版本可能合併。處理器須冪等，處理程序重新啟動不提供業務事件的持久或恰好一次投遞保證。

### 關閉自動重新整理

在已發布設定的 `load` 回傳值中修改：

```rust,ignore
"refresh_interval": 0,
```

修改編譯期設定後重新啟動／部署各處理程序。節點首次使用設定群組仍會讀取資料，之後持續重用快照，不進行背景檢查或讀取時自動到期。即使使用 Redis，也不再自動接收其他節點的設定變更；處理器自動重試也停止。需要時在明確的邊界主動呼叫：

```rust,ignore
settings.refresh().await?; // 捨棄尚未儲存的編輯，立即讀取資料來源。
manager.poll().await?;     // 檢查活動群組，並重試待處理的處理器。
```

`manager.notify(&notification).await?` 也會核實中繼資料提示並執行待處理的處理器。`poll`／`notify` 更新管理器供後續作用域使用的快照，不改變已解析出的 `Settings<T>`。本處理程序儲存仍會發布並更新本機管理器；`save_in` 外層提交後，編輯實例須明確重新整理。

### 部署共用設定

1. 各應用／Worker 處理程序啟用相同擴充，註冊一致的設定群組結構。
2. 同一應用的節點使用相同設定資料庫／資料表、命名空間及適用的加密金鑰；首次讀取前執行遷移。
3. 保留預設 60 秒或設定其他正數間隔。既有共用 Cache／Redis 可加速提示，沒有共用快取也沿用相同資料庫核對機制。
4. 在**每個處理程序**首次實際讀取設定群組前登記使用端失效處理器。尚未使用的群組沒有需要重新整理的快取使用端，僅註冊不啟動該群組的輪詢。
5. 正常啟動應用，佇列使用 `./artisan queue:work`，無需額外設定守護處理程序。節點 A 儲存後，B 的新作用域在核對後讀到新值；B 的舊作用域保留原值，直到明確重新整理。

獨立應用共用基礎設施時，除了分離快取命名空間，也須分離設定表／連線；不要為了匹配另一應用的命名空間而複製其設定憑證或金鑰。

### 重新整理應用使用端

透過 `SettingsManager` 註冊應用自己的失效／重建邏輯。套件註冊後使用 `app.make::<SettingsManager>().await?` 取得。例如，使應用自己的快取投影失效：

```rust,ignore
use openrust::{cache::CacheManager, contracts::cache::Repository as _};
use openrust_settings::{SettingsError, SettingsManager};

let manager = app.make::<SettingsManager>().await?;
let cache = app.make::<CacheManager>().await?.store(None)?;
let handler = manager.on_change("site", move |_change| {
    let cache = cache.clone();
    Box::pin(async move {
        cache.forget("site:rendered").await
            .map_err(|_| SettingsError::Refresh)?;
        Ok(())
    })
}).await?;

let state = manager.status("site").await;
// state：observed_version、applied_version、pending_handlers、last_error
manager.forget_handler("site", handler).await;
```

處理器在獨立非同步工作中執行，不繼承目前 HTTP／工作的門面作用域，須擷取所需相依服務。需要管理器本身時，擷取 `manager.downgrade()` 並在處理器內 `upgrade()`，避免循環參照。處理器五秒逾時會記錄遮罩錯誤並保留待處理版本，開啟自動檢查時，沒有新版本也繼續重試。首次載入只建立基準，不觸發變更處理器。`status()` 只表示本處理程序已註冊處理器的狀態，**不代表叢集全部節點確認**，不能用作全域發布屏障或持久稽核。

`poll().await?` 主動核對所有活動群組；`notify(&SettingsNotification { namespace, group, version }).await?` 可接入應用選用的通知傳輸層，只接受中繼資料並向資料庫核實，忽略其他命名空間及非活動群組，不信任通知傳入的值。兩者都不依賴 Redis Pub/Sub。`shutdown().await` 停止本管理器背景工作，不關閉共用連線；之後仍可明確 load／refresh／poll。

不使用應用 Provider 時，可組合 `SettingsManager::new(source, SettingsOptions::default(), None)?`，包裝為 `Arc` 後交給 `Settings::<T>::load`；依部署設定穩定命名空間即可。自訂 `SettingsRepository` 可覆寫 `versions()` 進行批次檢查，預設回退完整讀取。非空快照須透過 `SettingsSnapshot::with_version` 提供非空、不透明且每次提交改變的版本；從未持久化的空群組可沒有版本。

本功能不分發 `.env`、不改寫 `Config`、部署金鑰、監聽連接埠或資料庫連線池。服務自己的重新整理邊界與滾動發布仍由應用明確控制。

## 憑證與後台整合

敏感屬性使用應用加密器及舊金鑰環；巢狀憑證可作為單一物件整體加密。`re_encrypt()` 用目前金鑰重寫已存在且未鎖定的加密屬性，完成重寫與復原驗證後再移除舊金鑰。備份復原同時需要資料庫資料及適用的部署金鑰；密文損壞不無聲回退預設值。

設定包裝物件不可序列化，偵錯輸出不包含屬性值。`get()` 回傳的型別化資料供受信任內部使用，HTTP、日誌與佇列載荷須使用明確的公開或遮罩投影。後台授權、驗證、保留未修改憑證的表單更新及持久稽核由應用提供。

本擴充不註冊後台路由，也不自動動態改寫簡訊、郵件或儲存管理器。這些使用端須透過明確的配接重建快取中的服務實例。執行中的操作保留原快照；歷史上傳記錄須保留磁碟識別。資料庫翻譯詞條繼續由[翻譯載入器](../digging-deeper/localization.md)管理。

## 常見問題

| 現象 | 檢查項目 |
| --- | --- |
| 找不到 `settings-config` 發布資源 | 確認目前命令／建置啟用了擴充，不是只有上一次命令啟用 feature |
| 找不到設定表 | 在設定的連線上遷移；自訂資料表／連線須登記自己的對應遷移 |
| 註冊或綁定失敗 | 套件註冊後只登記一次唯一群組名稱，檢查未宣告屬性與錯誤值型別 |
| 其他節點仍讀取舊值 | 檢查間隔是否為 `0`、資料庫／資料表／命名空間是否一致、設定快取是否更新，以及該節點是否使用過設定群組；建立新作用域或明確重新整理 |
| 處理器持續待處理 | 查看本處理程序 `status()`，確認冪等性與擷取相依服務；關閉自動檢查後須主動 `poll()`／`notify()` |
| `Conflict`／`Locked` | 重新整理後再處理編輯，或在授權後明確解除鎖定；不要盲目重試或覆寫 |
| 解密或資料庫錯誤 | 還原正確金鑰、連線與資料；此類錯誤不會無聲回退預設值 |
