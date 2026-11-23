# Pennant

## 安裝 {#installation}

在應用 Cargo.toml 中啟用 `pennant` feature。透過 `./artisan` 執行時，將 `"pennant"` 加入 `[features].default` 並保留既有功能。套件探索自動載入 `PennantServiceProvider`，預設應用無需追加 Provider 註冊。

## 設定 {#configuration}

預設儲存為 `database`，透過 `PENNANT_STORE` 選擇；`array` 僅在目前生命週期儲存狀態。資料庫預設表為 `features`，特性名稱與作用域構成唯一鍵。需要自訂時發布設定：

```sh
./artisan vendor:publish --tag=pennant-config
./artisan migrate
```

設定歸屬擴充套件。發布的 Rust 設定需要接入編譯型設定載入器並重新建構；僅發布不會執行新的 Rust 檔案。命中設定快取時使用快取值，跳過套件環境讀取。Provider 登記預設表遷移，啟動時不查詢資料庫。使用自訂表或連線時，由應用提供匹配的遷移。

## 定義特性 {#defining-features}

在 Pennant 啟動後的應用 Provider 中登記定義。解析器接收擁有所有權的 `Scope`，回傳 JSON 值；`defineValue` 定義常數。定義在應用內共用，解析值歸屬目前 HTTP 請求、Artisan 呼叫或佇列任務。

```rust
let store = app.make::<openrust_pennant::FeatureManager>().await?.store(None)?;
store.defineValue("new-api", serde_json::json!(true))?;
store.define("theme", std::sync::Arc::new(|scope| Box::pin(async move {
    Ok(serde_json::json!(if scope == openrust_pennant::Scope::Null { "default" } else { "blue" }))
})))?;
```

## 判斷特性 {#checking-features}

只有 JSON `false` 表示關閉。`null`、零、空字串和陣列均為開啟。未知特性回傳 false 並發出 `UnknownFeatureResolved`，不會持久化此回退值。`allAreActive` 要求每個作用域的全部特性開啟；`someAreActive` 要求每個作用域至少有一個匹配特性。

```rust
let user = store.r#for("user-42");
let enabled = user.active("new-api").await?;
let theme = user.value("theme").await?;
user.loadMissing(&["new-api", "theme"]).await?;
let both = user.allAreActive(&["new-api", "theme"]).await?;
```

## 作用域 {#scopes}

使用 `for` 明確選擇作用域，`Scope::from_model` 轉換 Eloquent 模型，`globally` 選擇單一全域作用域。全域與匿名作用域彼此獨立。模型預設以具體 Rust 類別和主鍵識別；`useMorphMap(true)` 將類別部分切換為設定的 morph 別名。`FeatureScopeSerializeable` 配合 `Scope::fromSerializable` 自訂序列化；`FeatureScopeable` 配合 `forScopeable` 按所選 store 轉換識別。

預設作用域讀取原生認證已解析的身分。使用延遲載入 guard 時，先呼叫 `Feature::for_request(request).await?` 再判斷，或先執行認證中介軟體。`EnsureFeaturesAreActive` 會主動解析預設 guard。非 HTTP 場景請明確指定作用域，或設定 `FeatureManager::resolveScopeUsing`。

## 類別式特性 {#feature-classes}

實作 `FeatureDefinition`，用 `defineClass` 登記非同步工廠。工廠可以解析容器依賴。`name` 指定自訂名稱；`acceptsNullScope` 明確替代 PHP 參數反射。`before` 回傳非空結果時覆蓋已儲存值，但不會持久化覆蓋值。既有快取優先；`flushCache` 後重新執行鉤子。`before` 回傳 `None` 時交由驅動處理。

```rust
use openrust_pennant::{FeatureDefinition, FeatureFuture, Scope};
use serde_json::Value;
use std::sync::Arc;

struct NewApi;
impl FeatureDefinition for NewApi {
    fn name() -> &'static str { "new-api" }
    fn resolve<'a>(&'a self, _scope: &'a Scope) -> FeatureFuture<'a, Value> {
        Box::pin(async { Ok(Value::Bool(true)) })
    }
}
// Application Provider:
store.defineClass(|| Box::pin(async { Ok(Arc::new(NewApi)) }))?;
```

Rust 模組和工廠在編譯時註冊，不提供 PHP 執行階段類別掃描、反射和巨集。列舉透過明確的字串表示傳入。工廠延遲執行，定義特性不會解析依賴。

`store.discover` 接收編譯後的註冊函數。透過 `store.instance::<NewApi>().await?` 取得型別明確的類別實例，呼叫應用自訂方法。`isValidForScope` 控制 `all` 與 `loadAll` 包含哪些定義。Eloquent 模型匯入 `openrust_pennant::concerns::HasFeatures` 後可呼叫 `features(store)`。`defineLottery` 重用原生 `Lottery`，與其他定義一樣儲存首次解析結果。

## 修改狀態 {#updating-values}

啟用操作寫入所選作用域。`activateForEveryone` 更新已儲存作用域；首次遇到的新作用域仍使用解析器。`forget` 刪除狀態，使其重新解析。`flushCache` 會清除 array 驅動已解析狀態，資料庫驅動保留持久值。

```rust
user.activate("new-api").await?;
user.activateWithValue(&["theme"], serde_json::json!("green")).await?;
user.deactivate("new-api").await?;
user.forget(&["theme"]).await?;
store.activateForEveryone("new-api", serde_json::json!(true)).await?;
```

## 中介軟體 {#middleware}

在原生路由群組中用 `Arc::new(...)` 加入 `EnsureFeaturesAreActive::using(["new-api"])`。特性關閉時回傳 HTTP 400。`whenInactive` 接收目前中介軟體實例的非同步回應回呼。按路由需要設定認證／工作階段中介軟體。

## 命令 {#commands}

`pennant:feature` 在 `app/features` 建立 Rust 類別；請宣告其模組並在 Provider 註冊工廠。`stubs/feature.rs.stub` 可覆蓋範本。除非傳入 `--force`，既有檔案會保留。清理命令刪除儲存值並保留定義；`pennant:clear` 是 `pennant:purge` 的別名。部署後透過 `./main artisan` 執行。

```sh
./artisan pennant:feature NewApi
./artisan pennant:purge new-api theme
./artisan pennant:purge --except=new-api --except=theme
./artisan pennant:purge --except-registered --store=database
./artisan pennant:clear
```

## 自訂驅動與事件 {#drivers-and-events}

透過 `FeatureManager::extend` 為設定驅動登記建立器，實作包含批次讀取的原生 `Driver` 契約。`stored` 是選用能力，預設回報不支援；提供批次寫入實作後才啟用 `supportsSetAll`，否則逐項寫入並在每項後發出事件。建立器不得遞迴解析自身 store。事件使用原生 Dispatcher：`FeatureResolved`、`FeatureRetrieved`、`UnknownFeatureResolved`、`UnexpectedNullScopeEncountered`、`FeatureUpdated`、`FeatureUpdatedForAllScopes`、`FeatureDeleted`、`FeaturesPurged` 和 `AllFeaturesPurged`。快取命中同樣發出讀取事件；監聽失敗向呼叫方傳播。
