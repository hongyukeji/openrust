# 輔助函式

## 資料路徑 {#data-paths}

從 `openrust::support` 匯入 `Arr` 和資料輔助函式以存取 JSON 陣列與物件。`Arr::get` 優先查找完整鍵，再走訪點路徑。`data_get` 直接走訪路徑，支援 `*`、`{first}` 和 `{last}`。路徑缺少時回傳 `None`，明確空值回傳 `Some(Value::Null)`。

```rust
use openrust::support::{Arr, data_fill, data_get, data_has};
use serde_json::json;

let mut users = json!([{"active": null}, {}]);
data_fill(&mut users, "*.active", json!(true))?;
assert!(data_has(&users, "0.active"));
assert_eq!(data_get(&users, Some("*.active")), Some(json!([null, true])));
assert_eq!(Arr::get(&users, "1.active"), Some(&json!(true)));
```

`data_has` 檢查字面路徑，`data_set` 覆寫值，`data_fill` 填補缺少的值，`data_forget` 移除值。修改會驗證巢狀深度，失敗回傳 `DataPathError`。用 `Arr::dot` 和 `Arr::undot` 展開及還原路徑。型別化陣列使用 Rust 切片和迭代器，集合管線使用 `Collection` 或 `LazyCollection`。字串操作見[字串](./strings.md)。

## 鏈式 URI {#fluent-uris}

使用 `Uri::of` 解析絕對或相對參照。元件設定方法傳回新的已驗證 URI。`query()` 讀取解碼後的參數並保留字面的加號和帶點鍵。用 `withQuery`、`withQueryIfMissing`、`replaceQuery`、`pushOntoQuery` 和 `withoutQuery` 修改查詢字串。用 JSON 映射傳遞具名參數。應用程式可用 `Uri::to`、`route`、`action`、`signedRoute` 或 `temporarySignedRoute` 產生位址，再用 `redirect` 建立經過驗證的重新導向回應。

```rust
use openrust::support::Uri;
use serde_json::json;

let uri = Uri::of("/items?label=a+b")?
    .withQuery(&json!({"page": 2}))?
    .withFragment("summary")?;
assert_eq!(uri.value(), "/items?label=a%2Bb&page=2#summary");
```

## 陣列型別讀取與查詢編碼 {#typed-arrays}

`Arr::array`、`boolean`、`float`、`integer` 和 `string` 檢查已儲存的 JSON 型別。路徑缺少和型別錯誤會傳回不同錯誤。`Arr::add` 填入缺少或為 null 的路徑，`pull` 讀取並移除值。`wrap` 保留陣列和物件，將 null 轉為空清單，將純量包成單元素清單。`Arr::query` 依插入順序產生 RFC 3986 查詢字串。

```rust
use openrust::support::Arr;
use serde_json::json;

let mut data = json!({"filter": {"label": "a+b", "enabled": true}});
assert!(Arr::boolean(&data, "filter.enabled")?);
assert_eq!(Arr::pull(&mut data, "filter.enabled")?, Some(json!(true)));
assert_eq!(Arr::query(&data)?, "filter%5Blabel%5D=a%2Bb");
```

## 數字 {#numbers}

`Number::default()` 預設使用英語和 USD。透過 `useLocale` 和 `useCurrency` 設定實例預設值，或透過 `withLocale` 和 `withCurrency` 將區域設定傳給回呼。設定屬於實例，可在並行請求中使用各自的設定。

`format`、`percentage` 和 `currency` 使用 CLDR 資料。`parseInt` 保留 64 位元整數，`parseFloat` 讀取本地化數字前綴。`fileSize`、`abbreviate`、`forHumans`、`clamp`、`pairs` 和 `trim` 沿用 Laravel 數字規則。數字拼讀使用所支援語言的內建 CLDR 規則，小數拼讀支援英語。

```rust
use openrust::support::Number;

let number = Number::default();
let localized = number.withLocale("de-DE", |de| de.format(1234.5, Some(2), None))??;
assert_eq!(localized, "1.234,50");
assert_eq!(number.currency(1234.5, None, None)?, "$1,234.50");
assert_eq!(number.spellOrdinal(21)?, "twenty-first");
assert_eq!(number.parseInt("9,223,372,036,854,775,807")?, i64::MAX);
```

## 計時工具 {#timing-tools}

`Benchmark::value` 傳回計算結果和經過的毫秒數。`measure` 求非零次執行的平均值，`measureMany` 按順序回報具名回呼。使用 `valueAsync` 測量 Future。

`Sleep` 接收 `Duration`，在 await 時執行等待。用 `and` 累加時長，`when` 或 `unless` 控制是否等待，`while` 重複同一間隔。`Sleep::fake()` 建立實例內的記錄，透過 `using` 傳給等待後即可核對記錄的時長。

`Timebox::call` 執行非同步回呼，並將傳回結果等到最短時限，傳回的錯誤同樣適用。`returnEarly` 允許該實例提前傳回，直到呼叫 `dontReturnEarly` 重設。

```rust
use openrust::support::{Benchmark, Sleep, Timebox};
use std::time::Duration;

let (value, milliseconds) = Benchmark::value(|| 6 * 7);
assert_eq!(value, 42);
assert!(milliseconds >= 0.0);

let duration = Duration::from_millis(5);
let fake = Sleep::fake();
Sleep::r#for(duration).using(&fake).wait().await;
fake.assertSequence(&[Some(duration)]);

let mut timebox = Timebox::default();
assert_eq!(timebox.call(async |_| Ok::<_, ()>(42), duration).await, Ok(42));
```
