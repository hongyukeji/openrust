# 資料庫設定（擴充）

`openrust-settings` 將型別化應用設定存入資料庫，可用於網站資訊、服務設定及業務策略等群組。命名參考 Spatie Laravel Settings，重用 OpenRust 的資料庫、加密、遷移與容器作用域。

## 啟用與遷移

透過 `settings` Cargo feature 啟用選用套件。套件探索登記自身 Provider 與預設資料表遷移，不註冊應用業務設定群組：

```sh
cargo run --features settings -- artisan migrate
```

套件本身提供預設設定，預設應用不包含 `config/settings.rs`。需要自訂時執行 `./artisan vendor:publish --tag=settings-config`，在 `config/mod.rs` 的模組宣告與設定映射中分別加入 `mod settings;` 與 `("settings".to_owned(), settings::load(env)?),`，然後重新建置。修改資料表或連線時保留自己的對應遷移。

其他應用加入 `openrust-settings` 相依套件並使用 OpenRust 套件探索。宣告自己的設定型別，在套件註冊完成後，由該應用的 Provider 呼叫 `SettingsServiceProvider::registerSettings::<YourSettings>(app).await?`。預設應用沒有預先註冊的 `SiteSettings` 業務群組。首次讀取前執行遷移；Provider 註冊與啟動不查詢設定表。

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

模組設定使用相同介面。將型別放在模組內，在設定 Provider 註冊之後，由模組 Provider 呼叫 `SettingsServiceProvider::registerSettings::<DeliverySettings>(app).await?`。該群組共用設定表，與 `site` 相互獨立，無需修改儲存庫。

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

`save()` 成功提交後才更新實例的持久快照。`saveIn(&mut transaction)` 在呼叫端交易內使用儲存點，不發布新的持久快照；傳入目標資料庫的交易，外層提交後呼叫 `refresh`。外層回滾後仍可重試待提交編輯。取消操作時，交易擁有者須回滾或釋放交易。

每組從寫入連線一次查詢，目前請求／工作內重用，下個作用域讀取資料庫新值。不要將設定實例保存在永久單例。長時間命令、排程輪次與串流連線須明確定義重新整理邊界；多個群組不保證全域相同快照。資料庫故障及損壞資料明確失敗，只有缺少的覆寫值回退至預設值。

預設儲存庫每組限制 256 個屬性、1 MiB 持久 JSON，內部少量中繼資料另計入限制。無需共用快取即可使用；靜態 `Config` 契約維持，動態設定不會寫入 `config:cache`。

## 憑證與後台整合

敏感屬性使用應用加密器及舊金鑰環；巢狀憑證可作為單一物件整體加密。`reEncrypt()` 用目前金鑰重寫已存在且未鎖定的加密屬性，完成重寫與復原驗證後再移除舊金鑰。備份復原同時需要資料庫資料及適用的部署金鑰；密文損壞不無聲回退預設值。

設定包裝物件不可序列化，偵錯輸出不包含屬性值。`get()` 回傳的型別化資料供受信任內部使用，HTTP、日誌與佇列載荷須使用明確的公開或遮罩投影。後台授權、驗證、保留未修改憑證的表單更新及持久稽核由應用提供。

本擴充不註冊後台路由，也不自動動態改寫簡訊、郵件或儲存管理器。這些使用端須透過明確的配接重建快取中的服務實例。執行中的操作保留原快照；歷史上傳記錄須保留磁碟識別。資料庫翻譯詞條繼續由[翻譯載入器](../digging-deeper/localization.md)管理。
