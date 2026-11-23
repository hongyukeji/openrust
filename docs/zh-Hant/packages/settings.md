# 資料庫設定（擴充）

`openrust-settings` 將型別化應用設定存入資料庫，可用於網站資訊、服務設定及業務策略等群組。命名參考 Spatie框架Settings，重用 OpenRust 的資料庫、加密、遷移與容器作用域。

## 啟用與遷移

透過 `settings` Cargo feature 啟用選用套件。套件探索登記自身 Provider 與預設資料表遷移，不註冊應用業務設定群組：

```sh
cargo run --features settings -- artisan migrate
```

套件本身提供預設設定，預設應用不包含 `config/settings.rs`。需要自訂時執行 `./artisan vendor:publish --tag=settings-config`，在 `config/mod.rs` 的模組宣告與設定映射中分別加入 `mod settings;` 與 `("settings".to_owned(), settings::load(env)?),`，然後重新建置。修改資料表或連線時保留自己的對應遷移。

其他應用加入 `openrust-settings` 相依套件並使用 OpenRust 套件探索。宣告自己的設定型別，在套件註冊完成後，由該應用的 Provider 呼叫 `SettingsServiceProvider::register_settings::<YourSettings>(app).await?`。預設應用沒有預先註冊的 `SiteSettings` 業務群組。首次讀取前執行遷移；Provider 註冊與啟動不查詢設定表。

`settings.repository` 預設為 `database`；`settings.repositories.database.connection` 預設使用應用資料庫連線，`table` 預設為 `settings`。自訂連線或資料表時，登記對應的 `CreateSettingsTable` 遷移並在該連線執行，取代預設遷移。遷移仍使用既有紀錄及執行器。

## 宣告與讀寫

為衍生 `Clone`、`Serialize`、`Deserialize` 的結構實作 `SettingsGroup`。`group()` 宣告唯一群組名稱，`defaults(config)` 回傳完整預設物件，`validate()` 驗證業務限制，`encrypted()` 宣告敏感的頂層屬性。序列化欄位集合須穩定，不要依值省略欄位。群組與屬性名稱使用不帶限定詞的 ASCII 識別字，最多 128 位元組；屬性名稱前綴 `__settings_` 保留供內部使用。

在請求或工作作用域中，從應用／容器非同步解析 `Settings<YourSettings>`。受信任應用程式碼可這樣操作：

```rust,ignore
let settings = app.make::<Settings<SiteSettings>>().await?;
let current = settings.get().await;
settings.fill(serde_json::json!({"title": "我的網站"})).await?;
settings.save().await?;
settings.reset("title").await?;
settings.save().await?;
```

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

每個群組透過處理程序內的版本快照供既有請求／工作作用域重用。使用 `SettingsServiceProvider` 時，下個作用域在核對後看見新值（預設六十秒），不再每次請求查詢完整屬性。既有作用域及尚未儲存的編輯保留原快照；`refresh()` 可略過間隔並捨棄本機編輯。長時間命令、排程輪次與串流連線須明確定義重新整理邊界；多個群組不保證全域相同快照。資料庫故障及損壞資料在檢查時明確失敗，只有缺少的覆寫值回退至預設值。

預設儲存庫每組限制 256 個屬性、1 MiB 持久 JSON，內部少量中繼資料另計入限制。無需共用快取即可使用；靜態 `Config` 契約維持，動態設定不會寫入 `config:cache`。

## 單機與叢集

單台伺服器與多個應用／Worker 處理程序使用相同的 `Settings<T>` 介面、Provider 與設定。各節點共用權威設定資料庫及資料表，不需要叢集開關、Redis 或額外守護命令。首次實際讀取啟動輕量的處理程序內檢查器；僅註冊設定群組不查詢設定表。

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
