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

`format`、`percentage` 和 `currency` 使用 CLDR 資料。`parseInt` 保留 64 位元整數，`parseFloat` 讀取本地化數字前綴。`fileSize`、`abbreviate`、`forHumans`、`clamp`、`pairs` 和 `trim` 沿用 Laravel 數字規則。

```rust
use openrust::support::Number;

let number = Number::default();
let localized = number.withLocale("de-DE", |de| de.format(1234.5, Some(2), None))??;
assert_eq!(localized, "1.234,50");
assert_eq!(number.currency(1234.5, None, None)?, "$1,234.50");
assert_eq!(number.spellOrdinal(21)?, "twenty-first");
assert_eq!(number.parseInt("9,223,372,036,854,775,807")?, i64::MAX);
```

### 本地化數字拼讀 {#localized-number-spellout}

使用 `spell` 和 `spellOrdinal` 依區域設定拼讀整數和小數。Rust 實作內建 ICU 76.1／CLDR 46 規則。區域設定沒有專用序數拼讀規則時，`spellOrdinal` 沿用該區域的預設拼讀規則。傳入 `after` 或 `until` 時，等於閾值也會傳回數字格式。有限浮點數超出有號 64 位元範圍時，數字回退使用可往返轉換的原始十進位表示。

```rust
use openrust::support::Number;

let number = Number::default();
let german = number.withLocale("de-DE", |number| number.spell(1.25, None, None))??;
let russian = number.withLocale("ru", |number| number.spell(1.25, None, None))??;
let chinese = number.withLocale("zh-TW", |number| number.spell(1.25, None, None))??;
assert_eq!(german, "eins Komma zwei fünf");
assert_eq!(russian, "одна целый двадцать пять сотых");
assert_eq!(chinese, "一點二五");
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

## 陣列轉換與篩選 {#array-transforms}

用 `select` 選取字面欄位，用 `pluck` 按點路徑擷取。篩選、對映、分區和排序保留鍵。`take` 會重新編號整數鍵。`flatten` 和 `collapse` 分別按遞迴深度與單層合併處理陣列。

```rust
use openrust::support::Arr;
use serde_json::json;

let items = json!([{"id":"a","meta":{"name":"甲"}}, {"id":"b","meta":{"name":"乙"}}]);
let names = Arr::pluck(&items, "meta.name", Some("id"))?;
assert_eq!(names, json!({"a":"甲","b":"乙"}));
assert_eq!(Arr::take(&items, -1), json!([items[1]]));
assert_eq!(Arr::flatten(&json!([1,[2,[3]]]), Some(1))?, [json!(1),json!(2),json!([3])]);
```

## 日期與區間 {#dates-and-intervals}

`now` 和 `today` 傳入 `None` 時讀取應用時區。傳入 `DateTimezone` 可明確選擇 IANA 時區。`today` 取當地零點。

`plus` 和 `minus` 接收有號的 `DateInterval`。`MonthOverflow::Carry` 將多出的天數進位到下個月，`Clamp` 則取月底。使用 `microseconds` 至 `years` 建立區間。

```rust
use openrust::support::{Date, DateTimezone, MonthOverflow, months, now};

let zone = now(None)?.asDateTime().timezone();
assert_eq!(now(Some(zone))?.asDateTime().timezone(), zone);
let start = Date::fromLocal("2024-01-31T09:00:00".parse()?, DateTimezone::UTC)?;
let end = start.plus(months(1), MonthOverflow::Clamp)?;
let formatted = end.asDateTime().to_rfc3339();
assert_eq!(formatted, "2024-02-29T09:00:00+00:00");
```

## 管線 {#pipeline}

`Pipeline::send` 建立型別化非同步管線。`through` 取代階段，`pipe` 附加階段。呼叫 `next.run` 繼續執行，或直接傳回結果以短路。用 `then` 做最終投影，用 `thenReturn` 傳回傳遞值，用 `finally` 讀取原始輸入。

資料庫操作使用 `Pipeline<T, DatabaseError, R, Transaction>` 和 `withinTransaction`。階段透過 `next.context()` 在同一交易中執行查詢，傳回錯誤時回復。可傳入型別化回呼，或明確解析服務。

```rust
use openrust::support::facades::Pipeline;
use std::convert::Infallible;

let result = Pipeline::<_, Infallible>::send(20)
    .pipe(|value, next| Box::pin(async move { next.run(value + 1).await }))
    .pipe(|value, next| Box::pin(async move { next.run(value * 2).await }))
    .thenReturn()
    .await?;
assert_eq!(result, 42);
```

## 空值判斷與重試 {#blank-and-retry}

`blank` 和 `filled` 判斷 JSON 值是否為空。零和 false 都視為已填入。`Retry::backoff` 指定重試間隔，`Retry::times` 指定固定間隔和嘗試次數。`runWhen` 僅在錯誤符合述詞時重試。

選用值使用 `Option`，回退值使用 `Result::unwrap_or_else`，值快取可使用範圍明確的 `OnceLock`。

```rust
use openrust::support::{Retry, blank, filled};
use serde_json::json;
use std::time::Duration;

assert!(blank(&json!("  ")));
assert!(filled(&json!(false)));
let attempts = Retry::backoff([Duration::from_millis(2), Duration::from_millis(3)])
    .run(|attempt| async move {
        if attempt == 3 { Ok(attempt) } else { Err("transient") }
    })
    .await?;
assert_eq!(attempts, 3);
```
