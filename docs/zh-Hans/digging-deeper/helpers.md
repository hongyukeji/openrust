# 辅助函数

## 数据路径 {#data-paths}

从 `openrust::support` 导入 `Arr` 和数据辅助函数访问 JSON 数组与对象。`Arr::get` 优先查找完整键，再遍历点路径。`data_get` 直接遍历路径，支持 `*`、`{first}` 和 `{last}`。路径缺失返回 `None`，显式空值返回 `Some(Value::Null)`。

```rust
use openrust::support::{Arr, data_fill, data_get, data_has};
use serde_json::json;

let mut users = json!([{"active": null}, {}]);
data_fill(&mut users, "*.active", json!(true))?;
assert!(data_has(&users, "0.active"));
assert_eq!(data_get(&users, Some("*.active")), Some(json!([null, true])));
assert_eq!(Arr::get(&users, "1.active"), Some(&json!(true)));
```

`data_has` 检查字面路径，`data_set` 覆盖值，`data_fill` 填补缺失值，`data_forget` 移除值。修改会校验嵌套深度，失败返回 `DataPathError`。用 `Arr::dot` 和 `Arr::undot` 展开和还原路径。类型化数组使用 Rust 切片和迭代器，集合流水线使用 `Collection` 或 `LazyCollection`。字符串操作见[字符串](./strings.md)。

## 链式 URI {#fluent-uris}

使用 `Uri::of` 解析绝对或相对引用。组件设置方法返回新的已验证 URI。`query()` 读取解码后的参数并保留字面的加号和带点键。用 `with_query`、`with_query_if_missing`、`replace_query`、`push_onto_query` 和 `without_query` 修改查询串。用 JSON 映射传递具名参数。应用代码可用 `Uri::to`、`route`、`action`、`signed_route` 或 `temporary_signed_route` 生成地址，再用 `redirect` 构建经过验证的重定向响应。

```rust
use openrust::support::Uri;
use serde_json::json;

let uri = Uri::of("/items?label=a+b")?
    .with_query(&json!({"page": 2}))?
    .with_fragment("summary")?;
assert_eq!(uri.value(), "/items?label=a%2Bb&page=2#summary");
```

## 数组类型读取与查询编码 {#typed-arrays}

`Arr::array`、`boolean`、`float`、`integer` 和 `string` 检查已存储的 JSON 类型。路径缺失和类型错误分别返回不同错误。`Arr::add` 填充缺失或为 null 的路径，`pull` 读取并删除值。`wrap` 保留数组和对象，将 null 转为空列表，将标量包装为单元素列表。`Arr::query` 按插入顺序生成 RFC 3986 查询串。

```rust
use openrust::support::Arr;
use serde_json::json;

let mut data = json!({"filter": {"label": "a+b", "enabled": true}});
assert!(Arr::boolean(&data, "filter.enabled")?);
assert_eq!(Arr::pull(&mut data, "filter.enabled")?, Some(json!(true)));
assert_eq!(Arr::query(&data)?, "filter%5Blabel%5D=a%2Bb");
```

## 数字 {#numbers}

`Number::default()` 默认使用英语和 USD。通过 `use_locale` 和 `use_currency` 设置实例默认值，或通过 `with_locale` 和 `with_currency` 将局部设置传给回调。设置归实例所有，可在并发请求中使用各自的设置。

`format`、`percentage` 和 `currency` 使用 CLDR 数据。`parse_int` 保留 64 位整数，`parse_float` 读取本地化数字前缀。`file_size`、`abbreviate`、`for_humans`、`clamp`、`pairs` 和 `trim` 沿用框架数字规则。

```rust
use openrust::support::Number;

let number = Number::default();
let localized = number.with_locale("de-DE", |de| de.format(1234.5, Some(2), None))??;
assert_eq!(localized, "1.234,50");
assert_eq!(number.currency(1234.5, None, None)?, "$1,234.50");
assert_eq!(number.spell_ordinal(21)?, "twenty-first");
assert_eq!(number.parse_int("9,223,372,036,854,775,807")?, i64::MAX);
```

### 本地化数字拼读 {#localized-number-spellout}

使用 `spell` 和 `spell_ordinal` 按区域设置拼读整数和小数。Rust 实现内置 ICU 76.1／CLDR 46 规则。区域设置没有专用序数拼读规则时，`spell_ordinal` 沿用该区域的默认拼读规则。传入 `after` 或 `until` 时，等于阈值也返回数字格式。有限浮点数超出有符号 64 位范围时，数字回退使用可往返转换的原始十进制表示。

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

## 计时工具 {#timing-tools}

`Benchmark::value` 返回计算结果和经过的毫秒数。`measure` 求非零次运行的平均值，`measure_many` 按顺序报告具名回调。使用 `value_async` 测量 Future。

`Sleep` 接收 `Duration`，在 await 时执行等待。用 `and` 累加时长，`when` 或 `unless` 控制是否等待，`while` 重复同一间隔。`Sleep::fake()` 创建实例内的记录，通过 `using` 传给等待后即可核对记录的时长。

`Timebox::call` 运行异步回调，并将返回结果等到最短时限，返回的错误同样适用。`return_early` 允许该实例提前返回，直到调用 `dont_return_early` 重置。

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

## 数组变换与筛选 {#array-transforms}

用 `select` 选取字面字段，用 `pluck` 按点路径提取。过滤、映射、分区和排序保留键。`take` 会重新编号整数键。`flatten` 和 `collapse` 分别按递归深度与单层合并处理数组。

```rust
use openrust::support::Arr;
use serde_json::json;

let items = json!([{"id":"a","meta":{"name":"甲"}}, {"id":"b","meta":{"name":"乙"}}]);
let names = Arr::pluck(&items, "meta.name", Some("id"))?;
assert_eq!(names, json!({"a":"甲","b":"乙"}));
assert_eq!(Arr::take(&items, -1), json!([items[1]]));
assert_eq!(Arr::flatten(&json!([1,[2,[3]]]), Some(1))?, [json!(1),json!(2),json!([3])]);
```

## 日期与区间 {#dates-and-intervals}

`now` 和 `today` 传入 `None` 时读取应用时区。传入 `DateTimezone` 可显式选择 IANA 时区。`today` 取当地零点。

`plus` 和 `minus` 接收有符号的 `DateInterval`。`MonthOverflow::Carry` 将多出的天数进位到下个月，`Clamp` 则取月末。使用 `microseconds` 至 `years` 构造区间。

`Date::timezone(name)` 使用框架内置的 IANA 2025b 数据解析时区标识，不受操作系统时区数据影响。`DateTimezone` 是 Jiff 原生 `TimeZone`；显式传入其他来源的时区时使用该来源。`Date::from_utc` 接收 `jiff::Timestamp`，`from_local` 接收 `jiff::civil::DateTime`，`as_date_time` 借用 `jiff::Zoned`。重复使用日期或时区时可以克隆。原生类型支持 −9999 至 9999 年；PHP 日期表达式解析器继续支持其更宽的年份范围。

```rust
use openrust::support::{Date, DateTimezone, MonthOverflow, months, now};

let zone = now(None)?.as_date_time().time_zone().clone();
assert_eq!(now(Some(zone.clone()))?.as_date_time().time_zone().clone(), zone);
let start = Date::from_local("2024-01-31T09:00:00".parse()?, DateTimezone::UTC)?;
let end = start.plus(months(1), MonthOverflow::Clamp)?;
let formatted = end.as_date_time().strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
assert_eq!(formatted, "2024-02-29T09:00:00+00:00");
```

### 日期门面与测试时钟 {#date-facade}

`support::facades::Date` 委托所属应用的 `support::DateFactory`，`support::Date` 仍为日期值类型。`now`、`today`、`tomorrow` 和 `yesterday` 接收 `Option<DateTimezone>`；`None` 读取所属应用的当前时区。当地零点按夏令时转换计算，无应用时可使用默认 UTC 工厂。

`set_test_now(Some(value))`、`get_test_now` 和 `has_test_now` 与测试时间旅行、HTTP、Artisan、模型时间戳共用时钟。`set_test_now(None)` 清除冻结。`with_test_now` 接收原生 Future，退出时清除测试时间，包括错误、panic 和取消；不会恢复先前的冻结值。并发执行域的冻结值互相隔离，应用之间不共享时钟。需要嵌套恢复时使用测试应用的时间旅行方法。

```rust
use openrust::support::{Date as DateValue, DateTimezone, facades::Date};

let frozen = DateValue::from_utc("2024-03-10T12:00:00Z".parse()?, DateTimezone::UTC);
Date::with_test_now(Some(frozen), async {
    let today = Date::today(None)?;
    let tomorrow = Date::tomorrow(None)?;
    Ok::<_, openrust::support::DateError>((today, tomorrow))
}).await??;
```

`parse` 和 `raw_parse` 接收文本、整数或浮点时间戳、日期值及 `DateInput::Now`。相对表达式使用同一应用测试时钟。`DateInput` 也接收 JSON 字符串、数字和 null；布尔值、数组和对象返回 `DateError::InvalidExpression`。非有限浮点数及超出原生日期范围的值会报错，超过 20 MiB 的输入会被拒绝。

```rust
use openrust::support::facades::Date;

let next = Date::parse("tomorrow", None)?;
let text = next.format("Y-m-d H:i:sP")?;
let epoch = Date::parse(0, None)?;
assert_eq!(epoch.format("U.u")?, "0.000000");
```

`format` 使用 PHP 日期格式标记。`timezone_name` 与 `format("e T")` 在当地日期运算后仍保留解析得到的名称、偏移和缩写。`as_date_time` 仍为 Jiff 视图，解析出的缩写在其中使用固定偏移。显式传入原生时区时，解析和格式化保留其自身转换规则。返回日期值沿用 Jiff 的 −9999…9999 年范围，请求 `InputDate` 保留现有解析器的更宽范围。

`create_from_timestamp` 与 `create_from_timestamp_ms` 分别从秒、毫秒创建日期。传入 `None` 时使用 UTC，不读取应用默认时区；传入 `Some(zone)` 时转换结果时刻的时区。`create_from_timestamp_utc` 和 `create_from_timestamp_ms_utc` 显式选择 UTC。四个入口均接收 `DateInput` 的数字、文本形式。

```rust
let seconds = Date::create_from_timestamp_utc("1601735792.198956")?;
let milliseconds = Date::create_from_timestamp_ms_utc("1601735792198.956")?;
assert_eq!(seconds.format("U.u")?, milliseconds.format("U.u")?);
```

时间戳字符串按固定数字片段兼容规则解释，不用于校验日期表达式。负毫秒保留比较版本的历史拆分行为：`-1` 得到 `1970-01-01 00:00:00.001000+00:00`。非有限数字及超出原生日期范围的值会返回错误。

`has_format` 检查固定字段模式和宽度，`has_format_with_modifiers` 还解释 `#`、`!`、`|` 等修饰符。这些入口检查模式，不保证日历日期有效：`2024-02-30` 仍匹配 `Y-m-d`。`has_relative_keywords` 判断表达式是否依赖参考日期；null、空值和非法输入返回 false。

```rust
assert!(!Date::has_format("2024-2-9", "Y-m-d")?);
assert!(Date::has_format_with_modifiers("31/08/2015", "d#m#Y")?);
assert!(Date::has_relative_keywords("tomorrow")?);
assert!(!Date::has_relative_keywords(None)?);
```

`create_from_format` 和 `raw_create_from_format` 返回 `Result<Option<Date>, DateError>`。严格模式默认开启，格式不匹配时返回错误；设置 `use_strict_mode(false)` 后返回 `None`。时区错误、NUL 输入、大小限制及原生范围错误仍会报错。`use_strict_mode(None)` 恢复默认开启。这是应用工厂的共享设置，HTTP 请求和 Worker 使用同一份设置；不同应用与独立工厂互相隔离。

```rust
Date::use_strict_mode(false)?;
assert!(Date::create_from_format("Y-m-d", "wrong", None)?.is_none());
Date::use_strict_mode(None)?;
let date = Date::raw_create_from_format("!Y-m-d", "2024-01-02", None)?.unwrap();
assert_eq!(date.format("H:i:s")?, "00:00:00");
```

冻结时间且未显式指定时区时，按格式创建沿用 mock 的原时区，格式自带时区时使用格式中的值；无格式解析仍使用应用时区。`!` 和 `|` 重置未指定字段。带格式的 `Request::date` 及其门面使用同一规则，包括宽松模式的 `None`。默认输入规范化先执行，因此被 trim 删除的末尾 NUL 不会进入日期解析器；直接格式创建仍拒绝 NUL 输入。

`get_days()` 返回星期日至星期六的独立英文名称副本。`singular_unit`、`plural_unit` 规范化日期单位名称；`is_modifiable_unit` 严格检查单数名称及大小写。`get_time_format_by_precision` 返回分钟、秒、毫秒或微秒对应的时间格式，不支持的单位返回 `DateError::InvalidPrecision`。

```rust
assert_eq!(Date::singular_unit("CENTURIES")?, "century");
assert_eq!(Date::get_time_format_by_precision("milliseconds")?, "H:i:s.v");
assert!(Date::can_be_created_from_format("2024-02-29", "Y-m-d")?);
assert!(!Date::can_be_created_from_format(None, "Y-m-d")?);
```

`can_be_created_from_format` 同时检查实际格式创建和字段模式，NUL 输入仍会报错。超出原生年份范围的日期即使能由更宽的日历表示，也返回 false。

`Date::sleep(seconds).await` 在冻结时间时推进同一个测试时钟，保留 mock 时区和原日期值；否则按微秒精度异步等待，零与负时长不等待。非有限数值或超出可表示范围的时长返回错误。

```rust
Date::sleep(0.25).await?;
```

`instance(&value)` 克隆日期值。目前尚不提供本地化创建、自定义工厂与错误诊断入口。


## 流水线 {#pipeline}

`Pipeline::send` 创建类型化异步流水线。`through` 替换阶段，`pipe` 追加阶段。调用 `next.run` 继续执行，或直接返回结果以短路。用 `then` 做最终投影，用 `then_return` 返回传递值，用 `finally` 读取原始输入。

数据库操作使用 `Pipeline<T, DatabaseError, R, Transaction>` 和 `within_transaction`。阶段通过 `next.context()` 在同一事务中执行查询，返回错误时回滚。可传入类型化回调，或显式解析服务。

`.within_transaction(None)` 使用默认连接，`.within_transaction(Some("audit"))` 选择具名连接。该调用仅配置管道；`.then(...)` 或 `.then_return()` 才开启事务并执行。终点回调接收 `(value, &mut Transaction)`。`finally` 在提交或回滚后执行一次，执行中恐慌或被取消时也会执行。

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

## 空值判断与重试 {#blank-and-retry}

`blank` 和 `filled` 判断 JSON 值是否为空。零和 false 都算作已填充。`Retry::backoff` 指定重试间隔，`Retry::times` 指定固定间隔和尝试次数。`run_when` 仅在错误满足谓词时重试。

可选值使用 `Option`，回退值使用 `Result::unwrap_or_else`，值缓存可使用作用域明确的 `OnceLock`。

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

## 延迟回调 {#deferred-callbacks}

在 HTTP 处理器、Artisan 命令或队列任务内用 `defer(callback, name, always)` 注册拥有所有权的 Future，当前进程会在对应生命周期结束时执行。同一生命周期内同名回调以后注册的为准；相互独立的回调使用 `None`。

```rust
use openrust::support::defer;

let _ = defer(async move {
    std::fs::write("/tmp/report-ready", b"ready")?;
    Ok::<_, std::io::Error>(())
}, Some("report.ready"), false)?;
```

默认仅在 HTTP 状态小于400、命令退出码为0或任务尝试成功时执行。将 `always` 设为 `true`，或对返回句柄调用 `.always()`，可包含失败情况。回调错误交给应用异常处理器报告，其余回调继续执行；回调中新注册的回调也会执行。

注册时捕获作用域服务的 `Arc`；结束后重新解析可能得到新实例。HTTP 传输释放响应正文时执行回调，流式响应断连也会进入该边界。嵌套 Artisan 调用和同步任务共用外层生命周期。硬超时任务的进程内回调在子进程返回结果前执行，仍受该任务超时限制。完成前取消、进程终止或硬超时无法保证执行；必须跨故障保留的工作应使用持久队列。在活跃生命周期外注册会返回 `concurrency::Error::OutsideRequest`。

## JSON Schema {#json-schema}

OpenRust 提供 `JsonSchema` 静态构造入口和实例工厂 `JsonSchemaTypeFactory`。字符串、整数、数值、布尔、对象、数组、联合与 anyOf 类型通过 `Type` 组合。使用 Builder 设置约束，`required`、`nullable`、`unique` 显式传入布尔值；不同属性类型用 `.into()` 转换。

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

序列化返回 `Result`，非有限数值明确失败。`from_array` 支持本地 `$ref`、必填属性、可空联合及 anyOf 组合；远程或循环引用、布尔模式、元组 items、无法推断的类型、多类型联合上的专有约束及 null 默认值会失败。支持子集以外的未知元数据不保留；该接口用于构造模式，不负责校验业务数据。展开最多20,000个片段，嵌套或引用链最多128层。


## 概率回调

`Lottery::odds(1, 100)?` 表示 100 个整数机会中有 1 个中奖机会。浮点概率使用 `Lottery::odds(0.02, None::<u64>)?`。通过 `winner(|| ...)` 和 `loser(|| ...)` 返回同一种 Rust 类型，两者默认返回布尔值。`choose()` 返回一次结果，`choose_times(n)` 返回结果数组。机会数超过分母时始终中奖，负数始终不中奖；非有限概率报错。

测试中通过 `always_win`、`always_lose`、`fix(sequence)` 或 `force_result_with_sequence(sequence, missing_callback)` 控制结果。序列耗尽后恢复正常抽样，也可提供自定义后备回调。`set_result_factory` 设置抽样器，`determine_result_normally()` 或复数别名清除控制。异步控制必须放在 `Lottery::scope(async { ... }).await` 中，否则在异步运行时触发 panic，防止线程状态串入其他请求。同步测试使用 `scope_sync(|| ...)`。作用域在 panic 或取消后也恢复父级控制。回调通过闭包捕获参数，不提供动态可变参数调用。
