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

使用 `Uri::of` 解析绝对或相对引用。组件设置方法返回新的已验证 URI。`query()` 读取解码后的参数并保留字面的加号和带点键。用 `withQuery`、`withQueryIfMissing`、`replaceQuery`、`pushOntoQuery` 和 `withoutQuery` 修改查询串。用 JSON 映射传递具名参数。应用代码可用 `Uri::to`、`route`、`action`、`signedRoute` 或 `temporarySignedRoute` 生成地址，再用 `redirect` 构建经过验证的重定向响应。

```rust
use openrust::support::Uri;
use serde_json::json;

let uri = Uri::of("/items?label=a+b")?
    .withQuery(&json!({"page": 2}))?
    .withFragment("summary")?;
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

`Number::default()` 默认使用英语和 USD。通过 `useLocale` 和 `useCurrency` 设置实例默认值，或通过 `withLocale` 和 `withCurrency` 将局部设置传给回调。设置归实例所有，可在并发请求中使用各自的设置。

`format`、`percentage` 和 `currency` 使用 CLDR 数据。`parseInt` 保留 64 位整数，`parseFloat` 读取本地化数字前缀。`fileSize`、`abbreviate`、`forHumans`、`clamp`、`pairs` 和 `trim` 沿用 Laravel 数字规则。

```rust
use openrust::support::Number;

let number = Number::default();
let localized = number.withLocale("de-DE", |de| de.format(1234.5, Some(2), None))??;
assert_eq!(localized, "1.234,50");
assert_eq!(number.currency(1234.5, None, None)?, "$1,234.50");
assert_eq!(number.spellOrdinal(21)?, "twenty-first");
assert_eq!(number.parseInt("9,223,372,036,854,775,807")?, i64::MAX);
```

### 本地化数字拼读 {#localized-number-spellout}

使用 `spell` 和 `spellOrdinal` 按区域设置拼读整数和小数。Rust 实现内置 ICU 76.1／CLDR 46 规则。区域设置没有专用序数拼读规则时，`spellOrdinal` 沿用该区域的默认拼读规则。传入 `after` 或 `until` 时，等于阈值也返回数字格式。有限浮点数超出有符号 64 位范围时，数字回退使用可往返转换的原始十进制表示。

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

## 计时工具 {#timing-tools}

`Benchmark::value` 返回计算结果和经过的毫秒数。`measure` 求非零次运行的平均值，`measureMany` 按顺序报告具名回调。使用 `valueAsync` 测量 Future。

`Sleep` 接收 `Duration`，在 await 时执行等待。用 `and` 累加时长，`when` 或 `unless` 控制是否等待，`while` 重复同一间隔。`Sleep::fake()` 创建实例内的记录，通过 `using` 传给等待后即可核对记录的时长。

`Timebox::call` 运行异步回调，并将返回结果等到最短时限，返回的错误同样适用。`returnEarly` 允许该实例提前返回，直到调用 `dontReturnEarly` 重置。

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

```rust
use openrust::support::{Date, DateTimezone, MonthOverflow, months, now};

let zone = now(None)?.asDateTime().timezone();
assert_eq!(now(Some(zone))?.asDateTime().timezone(), zone);
let start = Date::fromLocal("2024-01-31T09:00:00".parse()?, DateTimezone::UTC)?;
let end = start.plus(months(1), MonthOverflow::Clamp)?;
let formatted = end.asDateTime().to_rfc3339();
assert_eq!(formatted, "2024-02-29T09:00:00+00:00");
```

## 流水线 {#pipeline}

`Pipeline::send` 创建类型化异步流水线。`through` 替换阶段，`pipe` 追加阶段。调用 `next.run` 继续执行，或直接返回结果以短路。用 `then` 做最终投影，用 `thenReturn` 返回传递值，用 `finally` 读取原始输入。

数据库操作使用 `Pipeline<T, DatabaseError, R, Transaction>` 和 `withinTransaction`。阶段通过 `next.context()` 在同一事务中执行查询，返回错误时回滚。可传入类型化回调，或显式解析服务。

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

## 空值判断与重试 {#blank-and-retry}

`blank` 和 `filled` 判断 JSON 值是否为空。零和 false 都算作已填充。`Retry::backoff` 指定重试间隔，`Retry::times` 指定固定间隔和尝试次数。`runWhen` 仅在错误满足谓词时重试。

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
