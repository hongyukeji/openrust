# Eloquent 工厂

## 定义模型工厂

用 `Factory::new` 定义模型的默认属性。若希望通过 `Flight::factory()` 调用，在模型上实现 `HasFactory`。运行下方命令在 `database/factories` 生成文件，再声明 Rust 模块并填写属性定义。

```sh
./artisan make:factory FlightFactory --model=Flight
```

### 本地化测试数据 {#localized-fake-data}

使用 `Factory::withFaker` 定义生成的模型属性。它读取所选连接所属应用的 `app.faker_locale`。使用 `fake(Some("fr_FR"))` 显式指定语言，或用 `fake(None)` 读取当前应用配置。Rust 生成器使用 fake-rs 的数据，提供 `name`、`safeEmail`、`city` 和 `sentence`。未支持的语言使用英文数据。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

pub fn profile_factory<M: Model>() -> Factory<M> {
    Factory::withFaker(|faker| {
        Attributes::from([
            ("name".into(), faker.name().into()),
            ("email".into(), faker.safeEmail().into()),
        ])
    })
}
```


用 `faker.unique(false)` 让同一个生成器按方法去重，用 `unique(true)` 清空历史。`Factory::tryWithFaker` 的同一批次共享生成器，重试耗尽会在该批模型写入前返回错误。独立工厂调用各有自己的历史。例如，可在工厂定义中使用 `faker.unique(false).safeEmail()?`。

内置生成方法还包括 `firstName`、`lastName`、`freeEmail`、`userName`、`streetName`、`postcode`、`phoneNumber`、`company` 和 `word`。这些方法也可通过 `unique` 调用。样本取自 fake-rs 中相应语言的数据集。

用 `numberBetween(first, second)` 生成整数，包含两个端点，端点可以任意排序。`randomDigit()` 返回 0 至 9，`randomDigitNotNull()` 返回 1 至 9。`dateTimeBetween(start, end)` 接收显式 UTC 日期时间，按整秒取样并包含两端，起始秒晚于结束秒时返回错误。四种方法都支持 `unique`。

`randomDigitNot(except)` 排除指定数字。`randomNumber(digits, strict)` 的位数传入 `None` 时随机选择一至九位。严格模式生成确切位数，非严格模式也接受零位并返回零。显式位数超过九位或严格模式指定零位时返回错误。两种方法均支持 `unique`。

`randomFloat(decimals, min, max)` 先在区间取样，再按指定小数位舍入，中点远离零。传入 `None` 可随机选择零至九位精度或生成上界。上下界可逆序，舍入结果可能超出原区间。Rust 对非有限边界或舍入溢出返回错误。`unique` 将正零与负零视为同一个值。

用 `unixTime(max)` 在 Unix 纪元与指定端点之间取样整秒，或用 `dateTime(max)` 返回 UTC 日期时间。传入 `None` 时使用当前时间。`date(format, max)` 和 `time(format, max)` 接收 Chrono 格式，默认分别为 `%Y-%m-%d` 和 `%H:%M:%S`，格式无效时返回错误。`iso8601(max)` 包含实际使用的时区偏移量。五种方法均支持 `unique`。

用 `dateTimeInInterval(date, interval)` 在 UTC 起点和该时间加上带符号的 Chrono `TimeDelta` 之间按整秒取样。负时长向过去取样。起点和时长传入 `None` 时，分别默认取 30 年前和向后五天。`dateTimeThisCentury(max)`、`dateTimeThisDecade(max)` 和 `dateTimeThisMonth(max)` 分别从当前时间往前 100 年、10 年和一个月开始。`dateTimeThisYear(max)` 从应用时区当年 1 月 1 日零点开始。其他日历回溯和 `dateTimeInInterval` 的缺省起点也采用应用时区。两个区间参数都省略时，默认五天按本地日历计算。显式传入的 `TimeDelta` 表示实际经过的时长。这四个方法在 `max` 为 `None` 时以上述当前时间为终点，拒绝早于起点的终点，日历溢出按 FakerPHP 规则归一化。五个方法均支持 `unique`。Rust 使用有类型的日期和时长。需要自定义日历区间时，可计算终点后调用 `dateTimeBetween`。

用 `amPm(max)`、`dayOfMonth(max)`、`dayOfWeek(max)`、`month(max)`、`monthName(max)` 和 `year(max)` 按实际使用的时区格式化随机日期的各个部分。日和月的数字以补零字符串返回，星期和月份名称沿用 FakerPHP 的英文日期格式。`dateTimeAD(max)` 从 FakerPHP 的 64 位公元日期下界开始取样。这些方法在 `max` 为 `None` 时使用当前时间。`century()` 随机返回 I 至 XXI 的罗马数字。所有方法均支持 `unique`。

用 `timezone(countryCode)` 随机获取 IANA 时区标识。传入 `None` 从完整列表取样，也可传入 `Some("TW")` 等国家或地区代码来限定范围。代码区分大小写，未知代码返回 `None`。框架内置与 `chrono-tz` 同版的 IANA 数据。该方法支持 `unique`。返回的标识可用于将 UTC 日期转换到目标时区。

日期字符串优先使用 `setDefaultTimezone(Some(zone))` 为当前生成器指定的时区，其次读取应用或工厂连接所属应用的 `app.timezone`，缺省为 UTC。传入 `None` 可清除覆盖，`getDefaultTimezone()` 可查看覆盖值。`dateTimeInTimezone(max, timezone)` 返回带时区的日期时间，传入 `Some(zone)` 可覆盖本次调用。日期时间方法仍返回 UTC 时刻。生成器的时区覆盖用于输出格式化，日历边界继续按应用时区计算。

用 `dateTimeBetweenExpressions(start, end)` 传入 PHP 风格的日期表达式，例如 `"-30 years"` 和 `"now"`。两个端点共用同一参考时刻，并按应用时区解析。表达式中的显式时区优先。数字终点字符串按 Unix 时间戳解析，`"@-1"` 则是显式时间戳表达式。无效表达式返回错误。此方法与 `dateTimeBetween` 共享 `unique` 历史。

用 `dateTimeInIntervalExpressions(date, interval)` 传入 PHP 风格的相对区间，例如 `Some("+1 month")` 或 `Some("5 days ago")`。终点按起始日期所在时区的本地时间计算，跨夏令时也遵循该规则。取样结果返回 UTC。省略日期时从 30 年前开始，省略区间时使用五个日历日。区间字符串接受相对表达式，拒绝绝对日期、时间和时区标识。此方法与 `dateTimeInInterval` 共享 `unique` 历史。

```rust
use chrono::{DateTime, TimeDelta};
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
};

pub fn activity_factory<M: Model>() -> Factory<M> {
    Factory::tryWithFaker(|faker| -> DatabaseResult<Attributes> {
        let start = DateTime::UNIX_EPOCH;
        let end = DateTime::from_timestamp(86_400, 0).unwrap();
        let happened_at = faker.dateTimeBetween(start, end)?;
        let reviewed_at = faker.dateTimeInInterval(Some(happened_at), Some(TimeDelta::days(5)))?;
        let sampled_this_year = faker.dateTimeThisYear(None)?;
        let historical = faker.dateTimeBetweenExpressions("-30 years", "now")?;
        let archived_at = faker.dateTimeInIntervalExpressions(Some("2024-01-31"), Some("+1 month"))?;
        Ok(Attributes::from([
            ("score".into(), faker.numberBetween(1, 100).into()),
            ("happened_at".into(), happened_at.to_rfc3339().into()),
            ("reviewed_at".into(), reviewed_at.to_rfc3339().into()),
            ("sampled_this_year".into(), sampled_this_year.to_rfc3339().into()),
            ("historical".into(), historical.to_rfc3339().into()),
            ("archived_at".into(), archived_at.to_rfc3339().into()),
            ("weekday".into(), faker.dateTimeInTimezone(Some(DateTime::UNIX_EPOCH), None).format("%A").to_string().into()),
            ("timezone".into(), faker.timezone(Some("TW")).unwrap_or("UTC").into()),
        ]))
    })
}
```

## 使用工厂创建模型

`state` 按声明顺序叠加。`sequence` 接收批次内从零开始的索引。`make` 创建内存模型并运行 `afterMaking`。`create` 保存模型、创建已声明的子关系，然后运行 `afterCreating`。调用 `count` 后返回 Eloquent 集合。工厂属性绕过批量赋值过滤，并继续执行模型修改器与属性转换。

用 `prependState` 将转换放到当前所有已登记状态之前。每个转换都能读取前面生成的属性。用 `set(name, value)` 在末尾追加一个属性覆盖。两种方法都能在 `count` 前后使用。直接传给创建方法的属性在已登记状态之后应用。

批量工厂先构造全部模型，再运行 `afterMaking`。`create` 逐个保存模型及其声明的关系，写入完成后才运行该批次的 `afterCreating`。`afterMaking` 失败时，该批模型尚未写入，构造期间解析的上级工厂可能已写入记录。

使用 `raw()` 或 `rawOn(connection)` 获取应用模型修改器和转换前的展开属性。指定数量时返回属性映射列表。原始属性展开跳过目标工厂的回调和子关系，仍可能保存声明的上级以解析关联键。使用 `createQuietly()` 或 `createQuietlyOn(connection)` 可保存模型及关系，同时抑制 Eloquent 模型事件。工厂回调照常执行。

用 `createOne(attributes)` 创建单个模型，即使工厂已指定批次数量。用 `createMany(records)` 逐条按顺序创建。传入 `FactoryRecords::Default` 使用工厂数量，未指定时创建一条。传入 `FactoryRecords::Count(n)` 指定数量，或用 `FactoryRecords::Attributes(vec![…])` 为每条记录指定覆盖属性。覆盖值在工厂状态之后应用。每条记录完成两个工厂回调后才开始下一条。后续记录失败时，先前写入可能保留。用 `createOneQuietly` 和 `createManyQuietly` 抑制模型事件，或用对应的 `On` 方法指定连接。

用 `makeOne(attributes)` 构造一个未保存的模型，或用 `makeMany(records)` 按顺序构造多个未保存的模型。两者都接受显式属性并执行 `afterMaking`。`makeMany` 使用同样的 `FactoryRecords` 选项，调用后保留已配置数量，数量为零时返回空集合。两种方法均不保存返回的模型，解析已声明的上级关系时仍可能写入上级模型。对应的 `On` 方法可指定连接。`lazy(attributes)` 返回异步回调，调用并等待后才解析连接并创建单个模型或指定数量的批次。重复调用会重新创建。

用 `forEachSequence(states)` 为每个模型应用一组属性，并按属性组数确定批次数量。`crossJoinSequence(sequences)` 按声明顺序组合各序列，最后一个序列变化最快，同名属性以后面的序列为准。单独设置批次数量，超过组合数时从头循环。传入空序列会返回错误。

在单模型或批量工厂上使用 `insert(attributes)` 批量写入生成的行，或用 `insertOn(connection, attributes)` 指定连接。工厂会执行 `afterMaking` 并解析所需的上级键。批量写入跳过目标模型的持久化事件、子关系创建及 `afterCreating`。写入使用存储属性，包含隐藏列，跳过追加字段和输出访问器。

在单模型或批量工厂上使用 `withoutAfterMaking()` 或 `withoutAfterCreating()`，清除该工厂已登记的对应回调。随后可以重新登记回调。另一阶段的回调及 Eloquent 模型事件照常执行。

在单模型或批量工厂上使用 `connection(Some("archive"))` 指定命名连接。`getConnectionName()` 返回工厂配置的覆盖值，`connection(None)` 清除该值。执行时依次查找工厂覆盖、模型连接和当前应用或 Seeder 的默认连接。`On` 方法对目标工厂使用显式传入的连接。嵌套工厂优先使用自身覆盖或模型连接，再继承外层连接。关系连接校验照常执行。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
    seeder::SeederContext,
};

async fn seed_flights<M: Model>(context: &SeederContext<'_>) -> DatabaseResult<()> {
    Factory::<M>::new(|| Attributes::from([("name".into(), "Flight".into())]))
        .sequence(|index| Attributes::from([("name".into(), format!("Flight {index}").into())]))
        .count(3)
        .createOn(context.connection())
        .await?;
    Ok(())
}
```

## 工厂关系

`has` 接收 `HasOne`、`HasMany`、`MorphOne` 或 `MorphMany`。用 `r#for` 创建 `BelongsTo` 或 `MorphTo` 的上级模型，用 `forExisting` 关联已有模型。每次批量调用只解析一次上级模型，`make` 也遵循这条规则。再次调用会生成新的上级。使用 `MorphTo` 时，描述符需要登记目标模型和映射别名。`hasAttached` 接收 `BelongsToMany` 或 `MorphToMany`，并写入中间表属性。写入按顺序执行。后续关联失败时，之前的写入可能保留。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, factories::Factory, relations::MorphTo},
    seeder::SeederContext,
};

pub async fn seed_comments<C: Model, P: Model>(
    context: &SeederContext<'_>,
    comments: Factory<C>,
    posts: Factory<P>,
    subject: &str,
) -> DatabaseResult<()> {
    comments.forNamed::<_, MorphTo<C>>(subject, posts).count(3)
        .createOn(context.connection()).await?;
    Ok(())
}
```

子工厂先读取工厂定义，再应用关系查询的默认属性和显式状态。直接及多态子关系随后补入外键和类型字段，再执行 `afterMaking`。附加模型工厂也会在应用显式状态前读取关系查询的默认属性。

用 `hasNamed`、`forNamed` 或 `hasAttachedNamed` 按名称解析关系。工厂使用 `Model::namedRelations` 或应用 `resolveRelationUsing` 登记的描述符。显式传入 Rust 描述符类型，例如 `forNamed::<_, MorphTo<Comment>>("subject", posts)`。名称不存在或类型不匹配时返回错误。

用 `parentAttribute("owner_id", owners)` 声明由另一工厂生成的外键属性。它展开为已保存模型的键，也可复用匹配的模型。定义、状态或调用传入的显式值优先。`withoutParents(vec![])` 禁用所有工厂外键属性。传入 `FactoryParentSelector::attribute("owner_id")` 或 `FactoryParentSelector::model::<Owner>()` 可按属性名或模型类型禁用。禁用的属性展开为 null，显式 `for` 和 `forExisting` 关系仍会解析。

### 复用已有模型 {#recycling-existing-models}

用 `recycle` 传入一个已保存的模型，或用 `recycleMany` 传入同类型模型的迭代器。这两个方法可在 `count` 前后调用。模型池会传给嵌套关系工厂。解析上级时，工厂在同一连接和类型的候选模型中随机选取一个，缺少匹配项时创建新上级。同一批次复用已选中的上级。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Collection, Model, factories::Factory, relations::BelongsTo},
    seeder::SeederContext,
};

pub async fn reuse_owner<C: Model, P: Model>(
    context: &SeederContext<'_>,
    children: Factory<C>,
    owner: P,
    relation: BelongsTo<C, P>,
    owners: Factory<P>,
) -> DatabaseResult<Collection<C>> {
    children.r#for(relation, owners).count(3).recycle(owner)
        .createOn(context.connection()).await
}
```

向 `hasAttached` 或 `hasAttachedNamed` 传入属性映射列表，即可按关联模型顺序设置中间表属性。关联工厂数量超过列表长度时会循环。用 `FactoryPivot::resolve` 根据已保存的父模型计算每次附加的属性。单个属性映射会应用于全部关联。

可以传入已有模型、模型列表或 Eloquent 集合，直接附加这些模型。模型必须已持久化、具有关系所需的键，并属于该关系使用的连接。目标无效时，在写入本次附加的中间表记录前返回错误。
