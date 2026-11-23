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

使用 `Uri::of` 解析絕對或相對參照。元件設定方法傳回新的已驗證 URI。`query()` 讀取解碼後的參數並保留字面的加號和帶點鍵。用 `with_query`、`with_query_if_missing`、`replace_query`、`push_onto_query` 和 `without_query` 修改查詢字串。用 JSON 映射傳遞具名參數。應用程式可用 `Uri::to`、`route`、`action`、`signed_route` 或 `temporary_signed_route` 產生位址，再用 `redirect` 建立經過驗證的重新導向回應。

```rust
use openrust::support::Uri;
use serde_json::json;

let uri = Uri::of("/items?label=a+b")?
    .with_query(&json!({"page": 2}))?
    .with_fragment("summary")?;
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

`Number::default()` 預設使用英語和 USD。透過 `use_locale` 和 `use_currency` 設定實例預設值，或透過 `with_locale` 和 `with_currency` 將區域設定傳給回呼。設定屬於實例，可在並行請求中使用各自的設定。

`format`、`percentage` 和 `currency` 使用 CLDR 資料。`parse_int` 保留 64 位元整數，`parse_float` 讀取本地化數字前綴。`file_size`、`abbreviate`、`for_humans`、`clamp`、`pairs` 和 `trim` 沿用框架數字規則。

```rust
use openrust::support::Number;

let number = Number::default();
let localized = number.with_locale("de-DE", |de| de.format(1234.5, Some(2), None))??;
assert_eq!(localized, "1.234,50");
assert_eq!(number.currency(1234.5, None, None)?, "$1,234.50");
assert_eq!(number.spell_ordinal(21)?, "twenty-first");
assert_eq!(number.parse_int("9,223,372,036,854,775,807")?, i64::MAX);
```

### 本地化數字拼讀 {#localized-number-spellout}

使用 `spell` 和 `spell_ordinal` 依區域設定拼讀整數和小數。Rust 實作內建 ICU 76.1／CLDR 46 規則。區域設定沒有專用序數拼讀規則時，`spell_ordinal` 沿用該區域的預設拼讀規則。傳入 `after` 或 `until` 時，等於閾值也會傳回數字格式。有限浮點數超出有號 64 位元範圍時，數字回退使用可往返轉換的原始十進位表示。

```rust
use openrust::support::Number;

let number = Number::default();
let german = number.with_locale("de-DE", |number| number.spell(1.25, None, None))??;
let russian = number.with_locale("ru", |number| number.spell(1.25, None, None))??;
let chinese = number.with_locale("zh-TW", |number| number.spell(1.25, None, None))??;
assert_eq!(german, "eins Komma zwei fünf");
assert_eq!(russian, "одна целый двадцать пять сотых");
assert_eq!(chinese, "一點二五");
```

## 計時工具 {#timing-tools}

`Benchmark::value` 傳回計算結果和經過的毫秒數。`measure` 求非零次執行的平均值，`measure_many` 按順序回報具名回呼。使用 `value_async` 測量 Future。

`Sleep` 接收 `Duration`，在 await 時執行等待。用 `and` 累加時長，`when` 或 `unless` 控制是否等待，`while` 重複同一間隔。`Sleep::fake()` 建立實例內的記錄，透過 `using` 傳給等待後即可核對記錄的時長。

`Timebox::call` 執行非同步回呼，並將傳回結果等到最短時限，傳回的錯誤同樣適用。`return_early` 允許該實例提前傳回，直到呼叫 `dont_return_early` 重設。

```rust
use openrust::support::{Benchmark, Sleep, Timebox};
use std::time::Duration;

let (value, milliseconds) = Benchmark::value(|| 6 * 7);
assert_eq!(value, 42);
assert!(milliseconds >= 0.0);

let duration = Duration::from_millis(5);
let fake = Sleep::fake();
Sleep::r#for(duration).using(&fake).wait().await;
fake.assert_sequence(&[Some(duration)]);

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

`Date::timezone(name)` 使用框架內建的 IANA 2025b 資料解析時區識別碼，不受作業系統時區資料影響。`DateTimezone` 是 Jiff 原生 `TimeZone`；明確傳入其他來源的時區時使用該來源。`Date::from_utc` 接收 `jiff::Timestamp`，`from_local` 接收 `jiff::civil::DateTime`，`as_date_time` 借用 `jiff::Zoned`。重複使用日期或時區時可以複製。原生型別支援 −9999 至 9999 年；PHP 日期運算式解析器繼續支援其更寬的年份範圍。

```rust
use openrust::support::{Date, DateTimezone, MonthOverflow, months, now};

let zone = now(None)?.as_date_time().time_zone().clone();
assert_eq!(now(Some(zone.clone()))?.as_date_time().time_zone().clone(), zone);
let start = Date::from_local("2024-01-31T09:00:00".parse()?, DateTimezone::UTC)?;
let end = start.plus(months(1), MonthOverflow::Clamp)?;
let formatted = end.as_date_time().strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
assert_eq!(formatted, "2024-02-29T09:00:00+00:00");
```

## 管線 {#pipeline}

`Pipeline::send` 建立型別化非同步管線。`through` 取代階段，`pipe` 附加階段。呼叫 `next.run` 繼續執行，或直接傳回結果以短路。用 `then` 做最終投影，用 `then_return` 傳回傳遞值，用 `finally` 讀取原始輸入。

資料庫操作使用 `Pipeline<T, DatabaseError, R, Transaction>` 和 `within_transaction`。階段透過 `next.context()` 在同一交易中執行查詢，傳回錯誤時回復。可傳入型別化回呼，或明確解析服務。

`.within_transaction(None)` 使用預設連線，`.within_transaction(Some("audit"))` 選擇具名連線。此呼叫僅設定管線；`.then(...)` 或 `.then_return()` 才開啟交易並執行。終點回呼接收 `(value, &mut Transaction)`。`finally` 在提交或回復後執行一次，執行中恐慌或被取消時也會執行。

```rust
use openrust::support::facades::Pipeline;
use std::convert::Infallible;

let result = Pipeline::<_, Infallible>::send(20)
    .pipe(|value, next| Box::pin(async move { next.run(value + 1).await }))
    .pipe(|value, next| Box::pin(async move { next.run(value * 2).await }))
    .then_return()
    .await?;
assert_eq!(result, 42);
```

## 空值判斷與重試 {#blank-and-retry}

`blank` 和 `filled` 判斷 JSON 值是否為空。零和 false 都視為已填入。`Retry::backoff` 指定重試間隔，`Retry::times` 指定固定間隔和嘗試次數。`run_when` 僅在錯誤符合述詞時重試。

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

## 延遲回呼 {#deferred-callbacks}

在 HTTP 處理器、Artisan 命令或佇列任務內用 `defer(callback, name, always)` 註冊擁有所有權的 Future，目前程序會在對應生命週期結束時執行。同一生命週期內同名回呼以後註冊的為準；相互獨立的回呼使用 `None`。

```rust
use openrust::support::defer;

let _ = defer(async move {
    std::fs::write("/tmp/report-ready", b"ready")?;
    Ok::<_, std::io::Error>(())
}, Some("report.ready"), false)?;
```

預設僅在 HTTP 狀態小於400、命令結束碼為0或任務嘗試成功時執行。將 `always` 設為 `true`，或對回傳控制代碼呼叫 `.always()`，可包含失敗情況。回呼錯誤交給應用程式例外處理器回報，其餘回呼繼續執行；回呼中新註冊的回呼也會執行。

註冊時擷取作用域服務的 `Arc`；結束後重新解析可能取得新實例。HTTP 傳輸釋放回應本文時執行回呼，串流回應斷線也會進入此邊界。巢狀 Artisan 呼叫和同步任務共用外層生命週期。硬逾時任務的程序內回呼在子程序回傳結果前執行，仍受該任務逾時限制。完成前取消、程序終止或硬逾時無法保證執行；必須跨故障保留的工作應使用持久佇列。在活躍生命週期外註冊會回傳 `concurrency::Error::OutsideRequest`。

## JSON Schema {#json-schema}

OpenRust 提供 `JsonSchema` 靜態建構入口與實例工廠 `JsonSchemaTypeFactory`。字串、整數、數值、布林、物件、陣列、聯合與 anyOf 型別透過 `Type` 組合。使用 Builder 設定約束，`required`、`nullable`、`unique` 明確傳入布林值；不同屬性型別用 `.into()` 轉換。

```rust
use openrust::json_schema::{JsonSchema, JsonSchemaTypeFactory};
use serde_json::json;

let factory = JsonSchemaTypeFactory;
let schema = factory.object([
    ("name", factory.string().min(1).required(true).into()),
    ("active", factory.boolean().default(true).into()),
]).without_additional_properties();
let encoded = schema.to_array()?;
let restored = JsonSchema::from_array(encoded.clone())?;
assert_eq!(restored.to_array()?, encoded);
assert!(JsonSchema::from_array(json!({"type": "unknown"})).is_err());
```

序列化傳回 `Result`，非有限數值明確失敗。`from_array` 支援本地 `$ref`、必填屬性、可空聯合與 anyOf 組合；遠端或循環引用、布林模式、元組 items、無法推斷的型別、多型別聯合上的專有約束及 null 預設值會失敗。支援子集以外的未知中繼資料不保留；此介面用於建構模式，不負責驗證業務資料。展開最多20,000個片段，巢狀或引用鏈最多128層。


## 機率回呼

`Lottery::odds(1, 100)?` 表示 100 個整數機會中有 1 個中獎機會。浮點機率使用 `Lottery::odds(0.02, None::<u64>)?`。透過 `winner(|| ...)` 和 `loser(|| ...)` 傳回同一種 Rust 型別，兩者預設傳回布林值。`choose()` 傳回一次結果，`choose_times(n)` 傳回結果陣列。機會數超過分母時始終中獎，負數始終不中獎；非有限機率報錯。

測試中透過 `always_win`、`always_lose`、`fix(sequence)` 或 `force_result_with_sequence(sequence, missing_callback)` 控制結果。序列耗盡後恢復正常抽樣，也可提供自訂後備回呼。`set_result_factory` 設定抽樣器，`determine_result_normally()` 或複數別名清除控制。非同步控制必須放在 `Lottery::scope(async { ... }).await` 中，否則在非同步執行環境觸發 panic，防止執行緒狀態串入其他請求。同步測試使用 `scope_sync(|| ...)`。作用域在 panic 或取消後也恢復父級控制。回呼透過閉包擷取參數，不提供動態可變參數呼叫。
