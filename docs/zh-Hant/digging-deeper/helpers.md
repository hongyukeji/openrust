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

### 日期門面與測試時鐘 {#date-facade}

`support::facades::Date` 委託所屬應用的 `support::DateFactory`，`support::Date` 仍為日期值型別。`now`、`today`、`tomorrow` 和 `yesterday` 接收 `Option<DateTimezone>`；`None` 讀取所屬應用的目前時區。當地零點依夏令時間轉換計算，無應用時可使用預設 UTC 工廠。

`set_test_now(Some(value))`、`get_test_now` 和 `has_test_now` 與測試時間旅行、HTTP、Artisan、模型時間戳記共用時鐘。`set_test_now(None)` 清除凍結。`with_test_now` 接收原生 Future，離開時清除測試時間，包括錯誤、panic 和取消；不會恢復先前的凍結值。並行執行域的凍結值互相隔離，應用之間不共用時鐘。需要巢狀恢復時使用測試應用的時間旅行方法。

```rust
use openrust::support::{Date as DateValue, DateTimezone, facades::Date};

let frozen = DateValue::from_utc("2024-03-10T12:00:00Z".parse()?, DateTimezone::UTC);
Date::with_test_now(Some(frozen), async {
    let today = Date::today(None)?;
    let tomorrow = Date::tomorrow(None)?;
    Ok::<_, openrust::support::DateError>((today, tomorrow))
}).await??;
```

`parse` 和 `raw_parse` 接收文字、整數或浮點時間戳記、日期值及 `DateInput::Now`。相對表達式使用同一應用測試時鐘。`DateInput` 也接收 JSON 字串、數字和 null；布林值、陣列和物件回傳 `DateError::InvalidExpression`。非有限浮點數及超出原生日期範圍的值會報錯，超過 20 MiB 的輸入會被拒絕。

```rust
use openrust::support::facades::Date;

let next = Date::parse("tomorrow", None)?;
let text = next.format("Y-m-d H:i:sP")?;
let epoch = Date::parse(0, None)?;
assert_eq!(epoch.format("U.u")?, "0.000000");
```

`format` 使用 PHP 日期格式標記。`timezone_name` 與 `format("e T")` 在當地日期運算後仍保留解析所得的名稱、偏移和縮寫。`as_date_time` 仍為 Jiff 視圖，解析出的縮寫在其中使用固定偏移。明確傳入原生時區時，解析和格式化保留其自身轉換規則。回傳日期值沿用 Jiff 的 −9999…9999 年範圍，請求 `InputDate` 保留現有解析器的較寬範圍。

`create_from_timestamp` 與 `create_from_timestamp_ms` 分別從秒、毫秒建立日期。傳入 `None` 時使用 UTC，不讀取應用預設時區；傳入 `Some(zone)` 時轉換結果時刻的時區。`create_from_timestamp_utc` 和 `create_from_timestamp_ms_utc` 明確選擇 UTC。四個入口均接收 `DateInput` 的數字、文字形式。

```rust
let seconds = Date::create_from_timestamp_utc("1601735792.198956")?;
let milliseconds = Date::create_from_timestamp_ms_utc("1601735792198.956")?;
assert_eq!(seconds.format("U.u")?, milliseconds.format("U.u")?);
```

時間戳記字串依固定數字片段相容規則解讀，不用於驗證日期表達式。負毫秒保留比較版本的歷史拆分行為：`-1` 得到 `1970-01-01 00:00:00.001000+00:00`。非有限數字及超出原生日期範圍的值會回傳錯誤。

`has_format` 檢查固定欄位模式和寬度，`has_format_with_modifiers` 也解讀 `#`、`!`、`|` 等修飾符。這些入口檢查模式，不保證日曆日期有效：`2024-02-30` 仍匹配 `Y-m-d`。`has_relative_keywords` 判斷表達式是否依賴參考日期；null、空值和無效輸入回傳 false。

```rust
assert!(!Date::has_format("2024-2-9", "Y-m-d")?);
assert!(Date::has_format_with_modifiers("31/08/2015", "d#m#Y")?);
assert!(Date::has_relative_keywords("tomorrow")?);
assert!(!Date::has_relative_keywords(None)?);
```

`create_from_format` 和 `raw_create_from_format` 回傳 `Result<Option<Date>, DateError>`。嚴格模式預設開啟，格式不符時回傳錯誤；設定 `use_strict_mode(false)` 後回傳 `None`。時區錯誤、NUL 輸入、大小限制及原生範圍錯誤仍會報錯。`use_strict_mode(None)` 恢復預設開啟。這是應用工廠的共用設定，HTTP 請求和 Worker 使用同一份設定；不同應用與獨立工廠互相隔離。

```rust
Date::use_strict_mode(false)?;
assert!(Date::create_from_format("Y-m-d", "wrong", None)?.is_none());
Date::use_strict_mode(None)?;
let date = Date::raw_create_from_format("!Y-m-d", "2024-01-02", None)?.unwrap();
assert_eq!(date.format("H:i:s")?, "00:00:00");
```

凍結時間且未明確指定時區時，依格式建立沿用 mock 的原時區，格式自帶時區時使用格式中的值；無格式解析仍使用應用時區。`!` 和 `|` 重設未指定欄位。帶格式的 `Request::date` 及其門面使用同一規則，包括寬鬆模式的 `None`。預設輸入正規化先執行，因此被 trim 刪除的尾端 NUL 不會進入日期解析器；直接格式建立仍拒絕 NUL 輸入。

`get_days()` 回傳星期日至星期六的獨立英文名稱副本。`singular_unit`、`plural_unit` 正規化日期單位名稱；`is_modifiable_unit` 嚴格檢查單數名稱及大小寫。`get_time_format_by_precision` 回傳分鐘、秒、毫秒或微秒對應的時間格式，不支援的單位回傳 `DateError::InvalidPrecision`。

```rust
assert_eq!(Date::singular_unit("CENTURIES")?, "century");
assert_eq!(Date::get_time_format_by_precision("milliseconds")?, "H:i:s.v");
assert!(Date::can_be_created_from_format("2024-02-29", "Y-m-d")?);
assert!(!Date::can_be_created_from_format(None, "Y-m-d")?);
```

`can_be_created_from_format` 同時檢查實際格式建立與欄位模式，NUL 輸入仍會報錯。超出原生年份範圍的日期即使能由較寬的日曆表示，也回傳 false。

`Date::sleep(seconds).await` 在凍結時間時推進同一個測試時鐘，保留 mock 時區及原日期值；否則按微秒精度非同步等待，零與負時長不等待。非有限數值或超出可表示範圍的時長回傳錯誤。

```rust
Date::sleep(0.25).await?;
```

`create`、`create_safe` 接收 `DateComponents`；`create_strict` 接收秒分量為整數的 `StrictDateComponents`。`Default` 為 0 年 1 月 1 日零點，`current()` 將全部分量設為 `None`，從目前時鐘讀取。分量明確為 `None` 且未指定時區時沿用 mock 原時區；指定時區則先轉換 mock。

```rust
use openrust::support::{DateComponents, StrictDateComponents};
let value = Date::create(DateComponents {
    year: Some(2024), month: Some(2), day: Some(30),
    ..Default::default()
}, None)?.unwrap(); // 2024-03-01
let safe = Date::create_safe(DateComponents {
    year: Some(2024), month: Some(2), day: Some(29),
    ..DateComponents::current()
}, None)?;
let strict = Date::create_strict(StrictDateComponents::default(), None)?;
```

`create_safe` 拒絕無效日曆分量和日光節約時間間隙，不自動進位；嚴格模式回傳錯誤，寬鬆模式回傳 `None`。`create_strict` 始終驗證，不改變應用的嚴格模式設定；整數分量在型別層面避免 PHP 將小數隱式截斷。超出 -9999 至 9999 的年份回傳 `DateError::OutOfRange`。`create_from_date(year, month, day, timezone)` 保留時鐘時間，`create_midnight_date` 使用零點。`create_from_time(hour, minute, second, timezone)` 保留時鐘日期；用 `Some(0)` 設定零值，用 `None` 繼承分量。

`make(input, timezone)` 複製既有日期物件，或解析看起來像日期的字串。null、數值、空文字、以 `P` 開頭的區間及以 `R` 開頭的重複規則回傳 `None`；無效日期文字回傳錯誤。傳入既有日期時保留原時區，即使同時提供時區參數也不轉換。

`create_from_interface`、`create_from_immutable` 接收擁有所有權或借用的日期值、`jiff::Zoned`，保留其時刻及時區；拒絕其他輸入形狀。

```rust
let copied = Date::create_from_interface(&value)?;
let maybe_date = Date::make("2024-01-02", None)?;
assert!(Date::make("P1D", None)?.is_none());
let from_id = Date::create_from_id("01ARZ3NDEKTSV4RRFFQ69G5FAV")?;
```

`create_from_id` 接收 ULID、時間型 UUID v1/v2/v6/v7 文字，或框架原生 UUID/ULID 值。非時間型 UUID、無效或溢出的識別碼回傳錯誤；UUID v2 按其定義捨棄時間戳低 32 位元。日期使用 UTC 偏移，保留來源轉換器的微秒捨入及錯誤行為；超出原生日期範圍的年份回傳 `DateError::OutOfRange`。

`create_from_time_string(time, timezone)` 從今天零點開始，復用日期表達式解析器修改時間。沒有冒號時補上 `:0`，因此 `"12"` 表示中午。結果保留指定或應用時區，即使文字提及其他時區也不改變；進位、相對表達式及完整日期按日期修改規則處理。無效表達式在嚴格及寬鬆模式下都回傳錯誤。

```rust
let noon = Date::create_from_time_string("12", None)?;
let precise = Date::create_from_time_string("12:34:56.123456", None)?;
```

日期宏使用所屬應用的工廠及時鐘。註冊具型別回呼後，透過 `macro_call` 呼叫，傳入 `None` 移除。參數與回傳值可以是擁有所有權的 Rust 值，包括非 `Send` 型別；儲存的回呼自身須為 `Send + Sync`。

```rust
use openrust::support::DateMacro;
Date::r#macro("stamp", DateMacro::new(|factory, prefix: String| {
    Ok(format!("{prefix}{}", factory.now(None)?.format("Y-m-d")?))
}))?;
let text: String = Date::macro_call("stamp", "date:".to_owned())?;
assert!(Date::has_macro("stamp")?);
Date::r#macro("stamp", None)?;
Date::reset_macros()?;
```

`get_macro` 回傳可保留的回呼控制代碼。`mixin` 註冊 `(名稱, DateMacro)` 迭代器並覆寫既有名稱；`mixin_using` 接收可失敗的註冊提供器。Rust 混入明確註冊已編譯回呼，不反射方法或執行執行期 trait 名稱。

`generic_macro(DateGenericMacro::new(callback), priority)` 處理未知動態名稱，優先級高的先執行，同優先級保留註冊順序。回傳 `DateMacroError::UnknownMethod(name)` 繼續下一個回呼，其他錯誤立即停止；命名宏優先。`reset_macros` 清空兩種註冊表。預設日期工廠會把通用宏結果轉換成日期，因此須回傳日期或 `jiff::Zoned`；純量或空值回傳 `InvalidGeneratedDate`，寬鬆模式下未知名稱也會在此轉換失敗。命名宏可回傳任意型別。內建方法繼續使用正式具名入口。

`instance(&value)` 複製日期值。目前尚未提供本地化建立、自訂工廠與錯誤診斷入口。


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
