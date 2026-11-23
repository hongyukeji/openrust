# Eloquent 序列化

## 序列化模型與集合

用 `to_array()` 取得可編碼為 JSON 的映射，用 `to_json()` 取得 JSON 字串。兩者都包含已載入的關聯，輸出中的關聯名稱使用底線命名。用 `attributes_to_array()` 僅輸出屬性。集合輸出各個模型組成的陣列。

## 隱藏 JSON 欄位

在 `ModelMetadata` 宣告 `hidden` 或 `visible`。非空的 `visible` 只允許列出的欄位，`hidden` 排除同名欄位。透過 `make_visible`、`make_hidden`、`set_visible` 或 `set_hidden` 調整單一模型。設定關聯時使用載入時的原始名稱。

## 追加 JSON 欄位

在 `ModelMetadata::appends` 宣告存取器名稱，或呼叫模型的 `append`。追加欄位遵循相同的可見性規則。下例使用模型定義的 `summary` 存取器。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub fn public_json(record: &mut impl Model) -> DatabaseResult<String> {
    record.make_hidden(&["label"]).append(&["summary"]);
    record.to_json()
}
```

## 日期序列化

日期預設輸出為帶微秒的 UTC RFC 3339 字串。覆寫 `serialize_date` 可調整模型輸出。資料庫儲存格式由 `ModelMetadata::date_format` 控制。自訂轉換可實作 `serialize` 調整轉換值的輸出。

使用 `AttributeCast::DateWithFormat("%Y/%m/%d")` 或 `DateTimeWithFormat("%H:%M %:z")` 設定單一欄位的輸出格式。格式採用 Chrono 語法，使用模型連線所屬應用程式的時區。欄位讀取和資料庫儲存仍採用各自的格式。

## 從路由回傳模型

路由可以直接回傳模型、集合或資料庫結果，回應使用模型的序列化規則。剛建立的模型回傳 201，既有模型和集合回傳 200，`find_or_fail` 找不到模型時回傳 404。

```rust
use openrust::{database::eloquent::Model, routing::RoutingError, support::facades::Route};

pub fn register_model_routes<M: Model>() -> Result<(), RoutingError> {
    Route::get("/records", M::all)?;
    Route::get("/records/1", || M::find_or_fail(1))
}
```

## 條件可見性

`make_visible_if` 和 `make_hidden_if` 接受布林值或讀取目前模型的條件函式。條件為假時保留原有可見性。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub fn conditional_json(record: &mut impl Model, may_read: bool) -> DatabaseResult<String> {
    record.make_visible_if(may_read, &["label"]);
    record.make_hidden_if(!may_read, &["label"]);
    record.to_json()
}
```

## 集合可見性與附加欄位

集合上的可見性和附加方法會套用到每個模型。`merge_hidden` 和 `merge_visible` 擴充對應清單。`set_appends` 取代附加清單，`without_appends` 清空清單，包含模型預設的附加項目。附加欄位仍遵循各模型的可見性規則。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model},
};

pub fn public_json<M: Model>(records: &mut Collection<M>) -> DatabaseResult<String> {
    records.merge_hidden(&["internal_note"]).without_appends().to_json()
}
```
