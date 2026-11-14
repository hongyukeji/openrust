# Eloquent 工厂

## 定义模型工厂

用 `Factory::new` 定义模型的默认属性。若希望通过 `Flight::factory()` 调用，在模型上实现 `HasFactory`。运行下方命令在 `database/factories` 生成文件，再声明 Rust 模块并填写属性定义。

```sh
./artisan make:factory FlightFactory --model=Flight
```

### 模型与工厂绑定 {#factory-model-binding}

生成的工厂实现 `FactoryDefinition`，通过关联类型 `Model` 在编译期绑定模型。在 `database/factories/mod.rs` 声明生成模块，并在 `database/mod.rs` 声明 `factories`。在模型上实现 `HasFactory`，把 `Flight::factory()` 委托给选定工厂。也可直接使用 `FlightFactory::factory()`。每次创建工厂时先运行 `configure`，调用处追加的状态可以覆盖其默认值。

```rust
use openrust::database::eloquent::{
    Attributes, Model, ModelMetadata, ModelState,
    factories::{Factory, FactoryDefinition, HasFactory},
};

struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("flights");
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

struct FlightFactory;

impl FactoryDefinition for FlightFactory {
    type Model = Flight;

    fn definition() -> Attributes {
        Attributes::from([("name".into(), "Flight".into())])
    }

    fn configure(factory: Factory<Flight>) -> Factory<Flight> {
        factory.set("name", "Scheduled flight")
    }
}

impl HasFactory for Flight {
    fn factory() -> Factory<Self> {
        FlightFactory::factory()
    }
}

fn factory_shortcuts() -> openrust::database::DatabaseResult<()> {
    let factory = FlightFactory::new(Attributes::from([("name".into(), "Initial".into())]));
    assert_eq!(factory.modelName(), std::any::type_name::<Flight>());
    assert!(!factory.newModel(Attributes::new())?.state().exists());
    let _batch = FlightFactory::times(2);
    Ok(())
}
```

用 `FlightFactory::new(attributes)` 或 `FlightFactory::newWithState(callback)` 在 `configure` 之前登记初始状态。`FlightFactory::times(2)` 创建配置好的两条批次。单条和批量构建器的 `modelName()` 返回绑定的 Rust 类型名。`newModel(attributes)` 按模型的批量赋值规则构造未保存的模型，不执行工厂定义和状态。
用 `Factory::<Flight>::factoryForModel()` 取得模型通过 `HasFactory` 绑定的工厂。绑定以编译期模型类型为准。

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

`randomFloat(decimals, min, max)` 先在区间取样，再按指定小数位舍入，中点远离零。传入 `None` 可随机选择零至九位精度或生成上界。上下界可逆序，舍入结果可能超出原区间。非有限边界返回错误。与 PHP 8.5 一样，正精度超过 308 或舍入后的数值溢出时保留取样值。精度小于 −308 时返回零。`unique` 将正零与负零视为同一个值。

用 `unixTime(max)` 在 Unix 纪元与指定端点之间取样整秒，或用 `dateTime(max)` 返回 UTC 日期时间。传入 `None` 时使用当前时间。`date(format, max)` 和 `time(format, max)` 接收 Chrono 格式，默认分别为 `%Y-%m-%d` 和 `%H:%M:%S`，格式无效时返回错误。`iso8601(max)` 包含实际使用的时区偏移量。五种方法均支持 `unique`。

用 `dateTimeInInterval(date, interval)` 在 UTC 起点和该时间加上带符号的 Chrono `TimeDelta` 之间按整秒取样。负时长向过去取样。起点和时长传入 `None` 时，分别默认取 30 年前和向后五天。`dateTimeThisCentury(max)`、`dateTimeThisDecade(max)` 和 `dateTimeThisMonth(max)` 分别从当前时间往前 100 年、10 年和一个月开始。`dateTimeThisYear(max)` 从应用时区当年 1 月 1 日零点开始。其他日历回溯和 `dateTimeInInterval` 的缺省起点也采用应用时区。两个区间参数都省略时，默认五天按本地日历计算。显式传入的 `TimeDelta` 表示实际经过的时长。这四个方法在 `max` 为 `None` 时以上述当前时间为终点，拒绝早于起点的终点，日历溢出按 FakerPHP 规则归一化。五个方法均支持 `unique`。Rust 使用有类型的日期和时长。需要自定义日历区间时，可计算终点后调用 `dateTimeBetween`。

用 `amPm(max)`、`dayOfMonth(max)`、`dayOfWeek(max)`、`month(max)`、`monthName(max)` 和 `year(max)` 按实际使用的时区格式化随机日期的各个部分。日和月的数字以补零字符串返回，星期和月份名称沿用 FakerPHP 的英文日期格式。`dateTimeAD(max)` 从 FakerPHP 的 64 位公元日期下界开始取样。这些方法在 `max` 为 `None` 时使用当前时间。`century()` 随机返回 I 至 XXI 的罗马数字。所有方法均支持 `unique`。

用 `timezone(countryCode)` 随机获取 IANA 时区标识。传入 `None` 从完整列表取样，也可传入 `Some("TW")` 等国家或地区代码来限定范围。代码区分大小写，未知代码返回 `None`。框架内置与 `chrono-tz` 同版的 IANA 数据。该方法支持 `unique`。返回的标识可用于将 UTC 日期转换到目标时区。

日期字符串优先使用 `setDefaultTimezone(Some(zone))` 为当前生成器指定的时区，其次读取应用或工厂连接所属应用的 `app.timezone`，缺省为 UTC。传入 `None` 可清除覆盖，`getDefaultTimezone()` 可查看覆盖值。`dateTimeInTimezone(max, timezone)` 返回带时区的日期时间，传入 `Some(zone)` 可覆盖本次调用。日期时间方法仍返回 UTC 时刻。生成器的时区覆盖用于输出格式化，日历边界继续按应用时区计算。

用 `dateTimeBetweenExpressions(start, end)` 传入 PHP 风格的日期表达式，例如 `"-30 years"` 和 `"now"`。两个端点共用同一参考时刻，并按应用时区解析。表达式中的显式时区优先。数字终点字符串按 Unix 时间戳解析，`"@-1"` 则是显式时间戳表达式。端点解析失败时按 FakerPHP 的数值转换规则使用 Unix 零秒。起点晚于终点仍返回错误。此方法与 `dateTimeBetween` 共享 `unique` 历史。

用 `dateTimeInIntervalExpressions(date, interval)` 传入 PHP 风格的相对区间，例如 `Some("+1 month")` 或 `Some("5 days ago")`。终点按起始日期所在时区的本地时间计算，跨夏令时也遵循该规则。取样结果返回 UTC。省略日期时从 30 年前开始，省略区间时使用五个日历日。区间字符串接受相对表达式，拒绝绝对日期、时间和时区标识。此方法与 `dateTimeInInterval` 共享 `unique` 历史。

用 `resolveDateMax(expression)` 解析 PHP 风格的日期上界，再以 `Some(max)` 传给任何接受 `max` 的日期生成方法。解析一次即可在多个调用中共用同一上界。小数和科学计数法的数字字符串向零截断为 Unix 秒。空字符串使用当前时间。日期文本默认按应用时区解释，文本中的显式时区优先。无效或超出范围的值返回错误。

用 `unixTimeExpression(max)` 从字符串上界直接取样 Unix 秒，支持 Chrono 日期范围之外的时间戳和扩展年份。它与 `unixTime` 共用 `unique` 历史。语法无效的上界使用 Unix 零秒。需要在取样前验证输入时，用 `resolveDateMax`。超过解析限制或数值范围时仍返回错误。有限数字字符串超出有符号 64 位范围时饱和到端点，溢出为无穷大时按 PHP 整数转换规则取零。返回日期的接口仍要求上界可由 Chrono 表示。

```rust
use chrono::{DateTime, TimeDelta};
use openrust::database::{
    DatabaseResult, Value,
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
        let cutoff = faker.resolveDateMax("2024-12-31 23:59:59 UTC")?;
        let published_at = faker.iso8601(Some(cutoff));
        let epoch_seconds = faker.unixTimeExpression("+1000000-01-01 UTC")?;
        Ok(Attributes::from([
            ("score".into(), faker.numberBetween(1, 100).into()),
            ("ratio".into(), Value::Real(faker.randomFloat(Some(2), 0.0, Some(1.0))?)),
            ("description".into(), faker.text(200)?.into()),
            ("reference".into(), faker.bothify(Some("FL-??-%%##")).into()),
            ("ticket".into(), faker.regexify(r"FL-[A-Z]{2}-\d{4}")?.into()),
            ("happened_at".into(), happened_at.to_rfc3339().into()),
            ("reviewed_at".into(), reviewed_at.to_rfc3339().into()),
            ("sampled_this_year".into(), sampled_this_year.to_rfc3339().into()),
            ("historical".into(), historical.to_rfc3339().into()),
            ("archived_at".into(), archived_at.to_rfc3339().into()),
            ("published_at".into(), published_at.into()),
            ("epoch_seconds".into(), epoch_seconds.into()),
            ("weekday".into(), faker.dateTimeInTimezone(Some(DateTime::UNIX_EPOCH), None).format("%A").to_string().into()),
            ("timezone".into(), faker.timezone(Some("TW")).unwrap_or("UTC").into()),
            ("category".into(), faker.randomElement(&["launch", "review"]).copied().unwrap_or("launch").into()),
            ("active".into(), faker.boolean(Some(75)).into()),
            ("sample_locale".into(), faker.randomLocale().into()),
            ("currency".into(), faker.currencyCode().into()),
            ("sample_digest".into(), faker.sha256().into()),
            ("sample_ip".into(), faker.localIpv4().into()),
            ("website".into(), faker.url()?.into()),
            ("contact".into(), faker.safeEmail().into()),
            ("phone".into(), faker.e164PhoneNumber().into()),
            ("address".into(), faker.address().into()),
            ("display_name".into(), faker.nameWithGender(Some("female")).into()),
            ("latitude".into(), openrust::database::Value::Real(faker.latitude(None, None)?)),
        ]))
    })
}
```

### 文本生成 {#text-generation}

用 `words(count)`、`sentences(count)` 和 `paragraphs(count)` 生成指定数量的列表。对应的 `AsText` 方法用空格连接单词或句子，用空行分隔段落。`sentenceWithWordCount(count, variable)` 和 `paragraph(count, variable)` 支持固定数量或按 FakerPHP 规则浮动。原有按范围取样的 `sentence(words)` 也可以继续使用。`text(max)` 以 UTF-8 字节为上限。上限不足五字节或无法容纳生成项时返回错误。这些方法均支持 `unique`。词库由 fake-rs 按语言提供。

### 占位符格式化 {#pattern-formatters}

用 `numerify(Some(pattern))` 将 `#` 替换为数字、将 `%` 替换为非零数字。`lexify` 将 `?` 替换为 ASCII 小写字母。`bothify` 处理这三种占位符，并以相同概率将每个 `*` 替换为数字或小写字母。`asciify` 将 `*` 替换为 `!` 至 `~` 的可打印 ASCII 字符。其余字符按原样保留。传入 `None` 使用 FakerPHP 默认模板。`randomLetter()` 和 `randomAscii()` 返回单个 Rust 字符。六个方法均支持 `unique`。

用 `regexify(pattern)?` 从基本正则表达式生成样本。支持可选的斜杠分隔符和外层锚点、字面量、ASCII 字符类和区间、`\d`、`\w`、点号、普通分组、分支和有限重复。与 FakerPHP 一样，`*` 和 `+` 分别为当前模板选取一至九的重复上限。语法错误、环视和反向引用等未支持特性以及超出生成限制的模式均返回错误。`unique(false).regexify(pattern)?` 可对结果去重。

### 集合采样 {#collection-sampling}

用 `randomElement(values)` 借用一个元素，空切片返回 `None`。`randomKey(values)` 返回从零开始的位置。要从映射键或枚举变体中选择，将键或变体的切片传给 `randomElement`。`randomElements(values, count, allowDuplicates)?` 接受 `Some(n)` 固定抽取数量，或用 `None` 在一到切片长度之间随机选择数量。不放回时每个位置最多抽一次，不同位置上的相同值仍可同时出现。零条返回空向量。非零采样要求切片非空，不放回采样的数量不能超过切片长度。

用 `shuffleArray(values)` 返回打乱后的副本，或用 `shuffleString(value)` 按 Unicode 标量值打乱字符串。`boolean(None)` 有 50% 的概率返回 true，传入 `Some(75)` 可设为 75%。这些方法均支持 `unique`，泛型采样和数组打乱按 JSON 序列化结果判断重复。

### 语言、国家和货币样本 {#locale-and-code-samples}

用 `languageCode()`、`countryCode()`、`countryISOAlpha3()`、`currencyCode()` 和 `emoji()` 从固定 FakerPHP 目录生成测试值。`randomLocale()` 抽取 `en_GB`、`zh_TW` 等语言环境标识，对应 FakerPHP 的 `locale()` 生成器。OpenRust 已有的 `locale()` 读取当前生成器的配置。样本目录包含历史标识。六个生成器均支持 `unique`。

### 摘要样本 {#digest-samples}

用 `md5()`、`sha1()` 和 `sha256()` 生成 32、40 和 64 个字符的小写十六进制样本。每个生成器遵循 FakerPHP 的做法，从 0 到 2,147,483,647 抽取整数，再对其十进制文本计算摘要。三个方法均支持 `unique`，用于测试和填充数据。

### 网络地址样本 {#network-address-samples}

用 `ipv4()`、`localIpv4()`、`ipv6()` 和 `macAddress()` 生成网络测试数据。`ipv4()` 在 `1.0.0.0`–`127.255.255.255` 和 `128.0.0.0`–`255.255.255.254` 两段中等概率选一段，再抽取地址，可能返回私有或特殊用途地址。`localIpv4()` 在 `10/8`、`172.16/12` 和 `192.168/16` 中等概率选一段。IPv6 返回八组未压缩的小写十六进制数，各组省略前导零。MAC 地址由六组以冒号分隔的两位大写十六进制数组成。四个方法均支持 `unique`。

### 域名、URL 和 slug {#domain-url-slug-samples}

用 `domainWord()?` 将生成的姓氏转写为小写 ASCII，用 `tld()` 按受支持语言环境的 FakerPHP 权重抽取后缀，`domainName()?` 将两者连接。`url()?` 使用上游 HTTP 和 HTTPS 模板，可带 `www`、slug 和 `.html`。姓氏使用 OpenRust 的语言数据和转写规则，与 FakerPHP 的 ICU 或回退映射可能不同。姓氏无法转写出可用字符时，域名和 URL 生成器返回错误。`slug(count, variable)` 用连字符拼接生成的词。传入 `None` 使用六个基准词，`Some(0)` 返回空字符串，`false` 固定词数。五个生成器均支持 `unique`。

### 邮件和密码样本 {#email-and-password-samples}

用 `email()?` 生成普通邮件地址，或用 `companyEmail()?` 生成使用随机域名的地址。简体中文的普通邮件使用本地区免费邮箱列表。`safeEmail()` 使用保留的示例域名，`freeEmail()` 使用地区邮箱服务域名。只需域名时用 `safeEmailDomain()` 或 `freeEmailDomain()`。用户名使用地区姓名或词库并转写为 ASCII。若无可用字符，OpenRust 生成一个 ASCII 字母，保留已有用户名及邮件方法的直接返回类型。`password(min, max)?` 使用从 `!` 到 `~` 的可打印 ASCII 字符，长度包含两端，接受反向边界，默认值为 6 和 20。以上生成器均支持 `unique`。

### 电话号码和 IMEI 样本 {#phone-and-imei-samples}

用 `phoneNumber()` 生成地区显示号码。简体中文和繁体中文使用固定 FakerPHP 模板，其他地区沿用已有 fake-rs 格式。`e164PhoneNumber()` 使用固定的国际号模板及地区覆盖，返回以加号开头的数字字符串。`imei()` 返回 15 位数字，含 Luhn 校验位。三个方法均支持 `unique`。

### 邮寄地址和称谓 {#address-and-title-samples}

用 `streetAddress()` 生成街道行，或用 `address()` 生成完整地址样本。美国、法国、简体中文和繁体中文使用各自的 FakerPHP 排列规则。城市、街道和部分地址组件仍使用 fake-rs 数据。单独字段可用 `buildingNumber()`、`secondaryAddress()`、`postcode()`、`state()` 和 `stateAbbr()`。`title(Some("male"))` 和 `title(Some("female"))` 指定称谓组，`title(None)` 在两组中随机选择，也可直接调用 `titleMale()` 和 `titleFemale()`。这些方法均支持 `unique`。

### 按性别生成名字 {#given-names-by-gender}

用 `firstNameMale()` 或 `firstNameFemale()` 从对应的 FakerPHP 词库抽取名字。`firstNameWithGender(Some("male"))` 和 `nameWithGender(Some("female"))` 显式选择分组。传入 `None` 或未识别的分组会在两组抽样。全名按地区模板组合现有姓氏数据，繁体中文名字由一个或两个抽取字符组成。缺少地区词库时使用英文名字及模板。现有无参 `name()` 和 `firstName()` 沿用原有数据来源，并与带分组的方法共享各自的 `unique` 历史。

### 坐标 {#coordinate-samples}

用 `latitude(None, None)?` 和 `longitude(None, None)?` 生成有符号十进制度数，舍入到小数点后六位。任一边界可传入 `Some(value)`，默认范围分别为 −90 到 90 和 −180 到 180。接受反向边界，非有限边界返回错误。`localCoordinates()?` 返回含 `latitude` 和 `longitude` 字段的可序列化 `Coordinates`。匈牙利和黑山采用固定上游地区范围，其他地区采用全球范围。台湾范围在 `zh_TW` 下由独立的 `localLatitude()?` 和 `localLongitude()?` 提供。五个方法均支持 `unique`。

### 软删除状态 {#trashed-state}

对已配置 `softDeletes` 的模型，在工厂或批量工厂上使用 `trashed(None)?`。它追加模型所声明的删除列状态，以选定连接的应用时区计算前一日同一时间。传入 `Some(value)` 可指定删除日期。后续状态和显式创建属性可以覆盖该值。未开启软删除的模型会返回错误。工厂按普通创建生命周期生成处于此状态的新模型。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory},
    seeder::SeederContext,
};

async fn seed_trashed<M: Model>(context: &SeederContext<'_>) -> DatabaseResult<()> {
    Factory::<M>::new(|| Attributes::from([("name".into(), "Archived".into())]))
        .count(2)
        .trashed(None)?
        .createOn(context.connection())
        .await?;
    Ok(())
}
```

### 条件状态 {#conditional-states}

用 `when(condition, transform)` 在布尔条件为真时转换工厂，或用 `unless(condition, transform)` 在条件为假时转换。`whenElse` 和 `unlessElse` 的第三个参数是备用转换。选中的闭包在配置工厂时运行一次，并返回工厂以继续链式调用。这些方法可在 `count` 前后使用。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn conditional_factory<M: Model>(featured: bool, draft: bool) {
    let _batch = Factory::<M>::new(Attributes::new)
        .when(featured, |factory| factory.set("featured", true))
        .count(2)
        .unlessElse(
            draft,
            |batch| batch.set("status", "published"),
            |batch| batch.set("status", "draft"),
        );
}
```

## 使用工厂创建模型

`state` 按声明顺序叠加。`sequence` 接收批次内从零开始的索引。`make` 创建内存模型并运行 `afterMaking`。`create` 保存模型、创建已声明的子关系，然后运行 `afterCreating`。调用 `count` 后返回 Eloquent 集合。工厂属性绕过批量赋值过滤，并继续执行模型修改器与属性转换。

用 `prependState` 将转换放到当前所有已登记状态之前。每个转换都能读取前面生成的属性。用 `set(name, value)` 在末尾追加一个属性覆盖。两种方法都能在 `count` 前后使用。直接传给创建方法的属性在已登记状态之后应用。

批量工厂先构造全部模型，再运行 `afterMaking`。`create` 逐个保存模型及其声明的关系，写入完成后才运行该批次的 `afterCreating`。`afterMaking` 失败时，该批模型尚未写入，构造期间解析的上级工厂可能已写入记录。

使用 `raw()` 或 `rawOn(connection)` 获取应用模型修改器和转换前的展开属性。指定数量时返回属性映射列表。原始属性展开跳过目标工厂的回调和子关系，仍可能保存声明的上级以解析关联键。使用 `createQuietly()` 或 `createQuietlyOn(connection)` 可保存模型及关系，同时抑制 Eloquent 模型事件。工厂回调照常执行。

用 `rawWith(attributes)` 或 `rawWithState(callback)` 追加仅本次生效的状态。闭包接收每条记录在解析关系键前的属性。覆盖外键时会跳过该键的解析，包括显式设为 null。对应的 `On` 方法可指定连接。

单条和批量构建器都可用 `rawWithParent(parent, callback)` 或 `rawWithParentOn(connection, parent, callback)` 借用父模型。类型匹配的 `stateWithParent` 和本次闭包都可以读取父模型。传入父模型本身不会添加关系属性。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, factories::{Factory, HasFactory}}};

async fn preview_with_state<M: Model + HasFactory>() -> DatabaseResult<Attributes> {
    Factory::<M>::factoryForModel()
        .rawWithState(|_| Attributes::from([("name".into(), "Preview".into())]))
        .await
}

async fn preview_for_parent<M: HasFactory, P: Model>(parent: &P) -> DatabaseResult<Attributes> {
    Factory::<M>::factoryForModel()
        .rawWithParent(parent, |_, parent| {
            Attributes::from([("parent_key".into(), parent.getKey().cloned().unwrap_or(openrust::database::Value::Null))])
        })
        .await
}
```

用 `createOne(attributes)` 创建单个模型，即使工厂已指定批次数量。用 `createMany(records)` 逐条按顺序创建。传入 `FactoryRecords::Default` 使用工厂数量，未指定时创建一条。传入 `FactoryRecords::Count(n)` 指定数量，或用 `FactoryRecords::Attributes(vec![…])` 为每条记录指定覆盖属性。覆盖值在工厂状态之后应用。每条记录完成两个工厂回调后才开始下一条。后续记录失败时，先前写入可能保留。用 `createOneQuietly` 和 `createManyQuietly` 抑制模型事件，或用对应的 `On` 方法指定连接。

用 `makeOne(attributes)` 构造一个未保存的模型，或用 `makeMany(records)` 按顺序构造多个未保存的模型。两者都接受显式属性并执行 `afterMaking`。`makeMany` 使用同样的 `FactoryRecords` 选项，调用后保留已配置数量，数量为零时返回空集合。两种方法均不保存返回的模型，解析已声明的上级关系时仍可能写入上级模型。对应的 `On` 方法可指定连接。`lazy(attributes)` 返回异步回调，调用并等待后才解析连接并创建单个模型或指定数量的批次。重复调用会重新创建。

下面演示如何调用 `lazy` 返回的回调。创建回调时目标表没有写入，每次调用并等待都会写入一批新模型。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{
        Attributes, Model,
        factories::{Factory, FactoryRecords},
    },
};

async fn preview_and_seed<M: Model>() -> DatabaseResult<()> {
    let batch = Factory::<M>::new(|| {
        Attributes::from([("name".into(), "Flight".into())])
    })
    .count(2);

    let _one = batch.makeOne(Attributes::new()).await?;
    let _previews = batch.makeMany(FactoryRecords::Default).await?;
    let seed = batch.lazy(Attributes::new());
    let _created = seed().await?;
    Ok(())
}
```

用 `forEachSequence(states)` 为每个模型应用一组属性，并按属性组数确定批次数量。`crossJoinSequence(sequences)` 按声明顺序组合各序列，最后一个序列变化最快，同名属性以后面的序列为准。单独设置批次数量，超过组合数时从头循环。传入空序列会返回错误。

在单模型或批量工厂上使用 `insert(attributes)` 批量写入生成的行，或用 `insertOn(connection, attributes)` 指定连接。工厂会执行 `afterMaking` 并解析所需的上级键。批量写入跳过目标模型的持久化事件、子关系创建及 `afterCreating`。写入使用存储属性，包含隐藏列，跳过追加字段和输出访问器。

单条和批量工厂都可以用 `makeWithParent(parent, attributes)` 和 `insertWithParent(parent, attributes)` 把父模型交给匹配的 `stateWithParent` 闭包。对应的 `On` 方法可指定连接。父模型作为状态上下文传入，需要建立关联时，由状态或调用属性赋予外键。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::FactoryBatch},
};

async fn insert_for_parent<M: Model, P: Model>(
    factory: &FactoryBatch<M>,
    parent: &P,
    attributes: Attributes,
) -> DatabaseResult<()> {
    factory.insertWithParent(parent, attributes).await
}
```

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

### 状态与回调的父模型 {#factory-parent-context}

当子工厂需要读取通过 `has` 或 `hasAttached` 创建它的模型时，使用 `stateWithParent::<Parent>` 和 `afterCreatingWithParent::<Parent>`。类型匹配时回调接收 `Some(&Parent)`，独立创建或父模型类型不同时接收 `None`。`afterMaking` 构造回调仍只接收子模型。

```rust
use openrust::database::eloquent::{
    Attributes, Model,
    factories::{Factory, FactoryBatch},
};

fn children_with_parent<P: Model, C: Model>() -> FactoryBatch<C> {
    Factory::<C>::new(Attributes::new)
        .count(2)
        .stateWithParent::<P>(|_, parent| {
            parent
                .and_then(|model| model.getAttribute("name"))
                .map(|name| Attributes::from([("label".into(), name.clone())]))
                .unwrap_or_default()
        })
        .afterCreatingWithParent::<P>(|_, _| Ok(()))
}
```

`has` 接收 `HasOne`、`HasMany`、`MorphOne` 或 `MorphMany`。用 `r#for` 创建 `BelongsTo` 或 `MorphTo` 的上级模型，用 `forExisting` 关联已有模型。每个 `for` 声明按子模型与上级模型的数据库连接对解析一次所需的上级。同一构建器重复执行时复用该上级，新构建器独立解析。使用 `MorphTo` 时，描述符需要登记目标模型和映射别名。`hasAttached` 接收 `BelongsToMany` 或 `MorphToMany`，并写入中间表属性。写入按顺序执行。后续关联失败时，之前的写入可能保留。

对 `BelongsTo` 和 `MorphTo`，同一外键以最后一次 `r#for` 或 `forExisting` 声明覆盖工厂定义。随后可以用状态或操作参数覆盖外键，包括传入 `Value::Null`。此时不会创建或关联上级模型。`MorphTo` 仍单独提供类型默认值，状态也可以覆盖类型。

```rust
use openrust::database::{Value, eloquent::{Model, factories::Factory, relations::BelongsTo}};

fn override_factory_owner<Child: Model, Owner: Model>(
    children: Factory<Child>,
    owners: Factory<Owner>,
) -> Factory<Child> {
    children
        .r#for(BelongsTo::new("owner_id", "id"), owners)
        .set("owner_id", Value::Null)
}
```

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

进程默认开启展开。`Factory::<Owner>::dontExpandRelationshipsByDefault()` 会禁用随后构造的工厂的展开，适用于所有模型类型。`expandRelationshipsByDefault()` 或 `flushState()` 可恢复默认值。已有工厂保留构造时的设置。完全禁用后，再调用带选择器的 `withoutParents` 仍保持完全禁用。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn factory_without_parent_expansion<Owner: Model>() -> Factory<Owner> {
    Factory::<Owner>::dontExpandRelationshipsByDefault();
    let factory = Factory::<Owner>::new(Attributes::new);
    Factory::<Owner>::expandRelationshipsByDefault();
    factory
}
```

### 复用已有模型 {#recycling-existing-models}

用 `recycle` 传入一个已保存的模型，或用 `recycleMany` 传入同类型模型的迭代器。这两个方法可在 `count` 前后调用。模型池会传给嵌套关系工厂。解析上级时，工厂在同一连接和类型的候选模型中随机选取一个，缺少匹配项时创建新上级。

单条或批量构建器都可用 `getRandomRecycledModel::<Owner>()` 按模型类型查询复用池，无需访问数据库。返回模型借用或 `None`。关系解析仍按上级模型的连接筛选候选。

```rust
use openrust::database::eloquent::{Model, factories::Factory};

fn recycled_model<Child: Model, Owner: Model>(factory: &Factory<Child>) -> Option<&Owner> {
    factory.getRandomRecycledModel::<Owner>()
}
```

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

传入非空中间表属性序列时，未指定数量的关联工厂按序列长度生成模型。显式 `count(n)` 优先，包括零。共享属性和空序列默认生成一个关联模型。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory, relations::BelongsToMany};

fn attach_with_pivot_count<P: Model, R: Model>(
    parents: Factory<P>,
    relation: BelongsToMany<P, R>,
    related: Factory<R>,
) -> Factory<P> {
    parents.hasAttached(relation, related, vec![
        Attributes::from([("active".into(), true.into())]),
        Attributes::from([("active".into(), false.into())]),
    ])
}
```

可以传入已有模型、模型列表或 Eloquent 集合，直接附加这些模型。模型必须已持久化、具有关系所需的键，并属于该关系使用的连接。目标无效时，在写入本次附加的中间表记录前返回错误。
