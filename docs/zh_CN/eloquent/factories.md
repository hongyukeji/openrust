# Eloquent 工厂

## 定义模型工厂

用 `Factory::new` 定义模型的默认属性。若希望通过 `Flight::factory()` 调用，在模型上实现 `HasFactory`。运行下方命令在 `database/factories` 生成文件，再声明 Rust 模块并填写属性定义。

应用入口及数据库连接选择见 [Seeder 中使用模型工厂](../database/seeding.md#using-model-factories)。

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

生成 `Fleet/FlightFactory` 等嵌套工厂时，Artisan 会推断对应的 `app::models::fleet` 模型。用 `--model` 或 `-m` 可选择其他模型。编译前声明生成的模块，模型与工厂仍通过 `FactoryDefinition::Model` 和 `HasFactory` 显式绑定。

### 工厂扩展 {#factory-extensions}

用 Rust 扩展 trait 为工厂提供可复用的方法。调用处导入 trait 后，可以用方法为构建器应用命名状态。

```rust
use openrust::database::eloquent::{Model, factories::Factory};

trait FlightFactoryExt {
    fn scheduled(self) -> Self;
}

impl<M: Model> FlightFactoryExt for Factory<M> {
    fn scheduled(self) -> Self {
        self.set("status", "scheduled")
    }
}
```

### 本地化测试数据 {#localized-fake-data}

下文列出已支持的生成器。

下文涉及的上游格式以[上游 Faker 词库与生成器](https://github.com/FakerPHP/Faker/tree/v1.24.1)为准。

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


在应用内，`fake()` 返回 `Arc<Faker>`，同一规范化语言的调用共享生成器。`unique(false)` 的历史和时区覆盖会跨调用保留，`unique(true)` 会清空共享历史。工厂使用由所选连接所属应用持有的独立生成器，同一应用内的批次和后续工厂调用共享其历史。生成器在构造批次时耗尽会在写入该批模型前返回错误。应用之外的 `fake()` 每次创建新生成器。用 `Faker::new` 可单独创建生成器。

内置生成方法还包括 `firstName`、`lastName`、`freeEmail`、`userName`、`streetName`、`postcode`、`phoneNumber`、`company` 和 `word`。这些方法也可通过 `unique` 调用。样本取自 fake-rs 中相应语言的数据集。

用 `numberBetween(first, second)` 生成整数，包含两个端点，端点可以任意排序。`randomDigit()` 返回 0 至 9，`randomDigitNotNull()` 返回 1 至 9。`dateTimeBetween(start, end)` 接收显式 UTC 日期时间，按整秒取样并包含两端，起始秒晚于结束秒时返回错误。四种方法都支持 `unique`。

`randomDigitNot(except)` 排除指定数字。`randomNumber(digits, strict)` 的位数传入 `None` 时随机选择一至九位。严格模式生成确切位数，非严格模式也接受零位并返回零。显式位数超过九位或严格模式指定零位时返回错误。两种方法均支持 `unique`。

`randomFloat(decimals, min, max)` 先在区间取样，再按指定小数位舍入，中点远离零。传入 `None` 可随机选择零至九位精度或生成上界。上下界可逆序，舍入结果可能超出原区间。非有限边界返回错误。与 PHP 8.5 一样，正精度超过 308 或舍入后的数值溢出时保留取样值。精度小于 −308 时返回零。`unique` 将正零与负零视为同一个值。

用 `unixTime(max)` 在 Unix 纪元与指定端点之间取样整秒，或用 `dateTime(max)` 返回 UTC 日期时间。传入 `None` 时使用当前时间。`date(format, max)` 和 `time(format, max)` 接收 Chrono 格式，默认分别为 `%Y-%m-%d` 和 `%H:%M:%S`，格式无效时返回错误。`iso8601(max)` 包含实际使用的时区偏移量。五种方法均支持 `unique`。

用 `dateTimeInInterval(date, interval)` 在 UTC 起点和该时间加上带符号的 Chrono `TimeDelta` 之间按整秒取样。负时长向过去取样。起点和时长传入 `None` 时，分别默认取 30 年前和向后五天。`dateTimeThisCentury(max)`、`dateTimeThisDecade(max)` 和 `dateTimeThisMonth(max)` 分别从当前时间往前 100 年、10 年和一个月开始。`dateTimeThisYear(max)` 从应用时区当年 1 月 1 日零点开始。其他日历回溯和 `dateTimeInInterval` 的缺省起点也采用应用时区。两个区间参数都省略时，默认五天按本地日历计算。显式传入的 `TimeDelta` 表示实际经过的时长。这四个方法在 `max` 为 `None` 时以上述当前时间为终点，拒绝早于起点的终点，日历溢出按上游 Faker 规则归一化。五个方法均支持 `unique`。Rust 使用有类型的日期和时长。需要自定义日历区间时，可计算终点后调用 `dateTimeBetween`。

用 `amPm(max)`、`dayOfMonth(max)`、`dayOfWeek(max)`、`month(max)`、`monthName(max)` 和 `year(max)` 按实际使用的时区格式化随机日期的各个部分。日和月的数字以补零字符串返回，星期和月份名称沿用上游 Faker 的英文日期格式。`dateTimeAD(max)` 从上游 Faker 的 64 位公元日期下界开始取样。这些方法在 `max` 为 `None` 时使用当前时间。`century()` 随机返回 I 至 XXI 的罗马数字。所有方法均支持 `unique`。

用 `timezone(countryCode)` 随机获取 IANA 时区标识。传入 `None` 从完整列表取样，也可传入 `Some("TW")` 等国家或地区代码来限定范围。代码区分大小写，未知代码返回 `None`。框架内置与 `chrono-tz` 同版的 IANA 数据。该方法支持 `unique`。返回的标识可用于将 UTC 日期转换到目标时区。

日期字符串优先使用 `setDefaultTimezone(Some(zone))` 为当前生成器指定的时区，其次读取应用或工厂连接所属应用的 `app.timezone`，缺省为 UTC。传入 `None` 可清除覆盖，`getDefaultTimezone()` 可查看覆盖值。`dateTimeInTimezone(max, timezone)` 返回带时区的日期时间，传入 `Some(zone)` 可覆盖本次调用。日期时间方法仍返回 UTC 时刻。生成器的时区覆盖用于输出格式化，日历边界继续按应用时区计算。

用 `dateTimeBetweenExpressions(start, end)` 传入 PHP 风格的日期表达式，例如 `"-30 years"` 和 `"now"`。两个端点共用同一参考时刻，并按应用时区解析。表达式中的显式时区优先。数字终点字符串按 Unix 时间戳解析，`"@-1"` 则是显式时间戳表达式。端点解析失败时按上游 Faker 的数值转换规则使用 Unix 零秒。起点晚于终点仍返回错误。此方法与 `dateTimeBetween` 共享 `unique` 历史。

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

用 `words(count)`、`sentences(count)` 和 `paragraphs(count)` 生成指定数量的列表。对应的 `AsText` 方法用空格连接单词或句子，用空行分隔段落。`sentenceWithWordCount(count, variable)` 和 `paragraph(count, variable)` 支持固定数量或按上游 Faker 规则浮动。原有按范围取样的 `sentence(words)` 也可以继续使用。`text(max)` 以 UTF-8 字节为上限。上限不足五字节或无法容纳生成项时返回错误。这些方法均支持 `unique`。词库由 fake-rs 按语言提供。

### 占位符格式化 {#pattern-formatters}

用 `numerify(Some(pattern))` 将 `#` 替换为数字、将 `%` 替换为非零数字。`lexify` 将 `?` 替换为 ASCII 小写字母。`bothify` 处理这三种占位符，并以相同概率将每个 `*` 替换为数字或小写字母。`asciify` 将 `*` 替换为 `!` 至 `~` 的可打印 ASCII 字符。其余字符按原样保留。传入 `None` 使用上游 Faker 默认模板。`randomLetter()` 和 `randomAscii()` 返回单个 Rust 字符。六个方法均支持 `unique`。

用 `regexify(pattern)?` 从基本正则表达式生成样本。支持可选的斜杠分隔符和外层锚点、字面量、ASCII 字符类和区间、`\d`、`\w`、点号、普通分组、分支和有限重复。与上游 Faker 一样，`*` 和 `+` 分别为当前模板选取一至九的重复上限。语法错误、环视和反向引用等未支持特性以及超出生成限制的模式均返回错误。`unique(false).regexify(pattern)?` 可对结果去重。

### 集合采样 {#collection-sampling}

用 `randomElement(values)` 借用一个元素，空切片返回 `None`。`randomKey(values)` 返回从零开始的位置。要从映射键或枚举变体中选择，将键或变体的切片传给 `randomElement`。`randomElements(values, count, allowDuplicates)?` 接受 `Some(n)` 固定抽取数量，或用 `None` 在一到切片长度之间随机选择数量。不放回时每个位置最多抽一次，不同位置上的相同值仍可同时出现。零条返回空向量。非零采样要求切片非空，不放回采样的数量不能超过切片长度。

用 `shuffleArray(values)` 返回打乱后的副本，或用 `shuffleString(value)` 按 Unicode 标量值打乱字符串。`boolean(None)` 有 50% 的概率返回 true，传入 `Some(75)` 可设为 75%。这些方法均支持 `unique`，泛型采样和数组打乱按 JSON 序列化结果判断重复。

### 语言、国家和货币样本 {#locale-and-code-samples}

用 `languageCode()`、`countryCode()`、`countryISOAlpha3()`、`currencyCode()` 和 `emoji()` 从固定上游 Faker 目录生成测试值。`randomLocale()` 抽取 `en_GB`、`zh_TW` 等语言环境标识，对应上游 Faker 的 `locale()` 生成器。OpenRust 已有的 `locale()` 读取当前生成器的配置。样本目录包含历史标识。六个生成器均支持 `unique`。

### 摘要样本 {#digest-samples}

用 `md5()`、`sha1()` 和 `sha256()` 生成 32、40 和 64 个字符的小写十六进制样本。每个生成器遵循上游 Faker 的做法，从 0 到 2,147,483,647 抽取整数，再对其十进制文本计算摘要。三个方法均支持 `unique`，用于测试和填充数据。

### 网络地址样本 {#network-address-samples}

用 `ipv4()`、`localIpv4()`、`ipv6()` 和 `macAddress()` 生成网络测试数据。`ipv4()` 在 `1.0.0.0`–`127.255.255.255` 和 `128.0.0.0`–`255.255.255.254` 两段中等概率选一段，再抽取地址，可能返回私有或特殊用途地址。`localIpv4()` 在 `10/8`、`172.16/12` 和 `192.168/16` 中等概率选一段。IPv6 返回八组未压缩的小写十六进制数，各组省略前导零。MAC 地址由六组以冒号分隔的两位大写十六进制数组成。四个方法均支持 `unique`。

### 域名、URL 和 slug {#domain-url-slug-samples}

用 `domainWord()?` 将生成的姓氏转写为小写 ASCII，用 `tld()` 按受支持语言环境的上游 Faker 权重抽取后缀，`domainName()?` 将两者连接。`url()?` 使用上游 HTTP 和 HTTPS 模板，可带 `www`、slug 和 `.html`。姓氏使用 OpenRust 的语言数据和转写规则，与上游 Faker 的 ICU 或回退映射可能不同。姓氏无法转写出可用字符时，域名和 URL 生成器返回错误。`slug(count, variable)` 用连字符拼接生成的词。传入 `None` 使用六个基准词，`Some(0)` 返回空字符串，`false` 固定词数。五个生成器均支持 `unique`。

### 邮件和密码样本 {#email-and-password-samples}

用 `email()?` 生成普通邮件地址，或用 `companyEmail()?` 生成使用随机域名的地址。简体中文的普通邮件使用本地区免费邮箱列表。`safeEmail()` 使用保留的示例域名，`freeEmail()` 使用地区邮箱服务域名。只需域名时用 `safeEmailDomain()` 或 `freeEmailDomain()`。用户名使用地区姓名或词库并转写为 ASCII。若无可用字符，OpenRust 生成一个 ASCII 字母，保留已有用户名及邮件方法的直接返回类型。`password(min, max)?` 使用从 `!` 到 `~` 的可打印 ASCII 字符，长度包含两端，接受反向边界，默认值为 6 和 20。以上生成器均支持 `unique`。

### 地区 URL 模板 {#regional-url-templates}

`ne-NP` 的 `url()` 使用固定 HTTP 模板，按模板添加 `www`、`.np` 或路径。`uk-UA` 的 `companyUrl()` 返回原始西里尔字母公司标签，可附加连字符和公司后缀。标签没有协议和域名后缀。两个生成器均支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_url_sample() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("ne-NP"))?.url()?, fake(Some("uk-UA"))?.companyUrl()?))
}
```

### 地区域名与邮箱 {#regional-domains-and-email}

`tld()` 和 `freeEmailDomain()` 从固定 Internet 词表抽样，重复条目保留原权重。沙特、约旦、埃及、伊朗、韩国和日本的 `domainName()` 使用罗马字姓氏词表。其他地区组合 `domainWord()` 和域名后缀。尼泊尔的通用邮箱模板包含两个公司域名、一个免费域名、三个附加 `.np` 的公司域名。直接调用 `companyEmail()` 使用普通域名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_email_sample() -> DatabaseResult<String> {
    fake(Some("ne-NP"))?.unique(false).email()
}
```

### 地区用户名 {#regional-usernames}

`userName()` 在沙特、约旦、埃及、伊朗、韩国和日本使用固定 FakerPHP 罗马字姓名词表。立陶宛模板从同一性别分组取名字和姓氏。简体中文使用固定 Lorem 词表和含下划线的模板。`safeEmail()` 和 `freeEmail()` 共用此生成器作为邮箱本地部分。通用路径使用 Rust 音译，音译后只剩分隔符时生成一个字母，保留用户名和邮箱的无错误签名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn username_sample() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).userName()
}
```

### 电话号码和 IMEI 样本 {#phone-and-imei-samples}

用 `phoneNumber()` 生成地区显示号码。简体中文和繁体中文使用固定上游 Faker 模板，其他地区沿用已有 fake-rs 格式。`e164PhoneNumber()` 使用固定的国际号模板及地区覆盖，返回以加号开头的数字字符串。`imei()` 返回 15 位数字，含 Luhn 校验位。三个方法均支持 `unique`。

### 条码与 ISBN {#barcode-samples}

使用 `ean8()`、`ean13()`、`isbn10()` 和 `isbn13()` 生成带校验位的样本。返回字符串保留前导零。ISBN-10 的末位可以是 `X`，ISBN-13 以 `978` 或 `979` 开头。各生成器独立维护 `unique` 历史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn catalog_codes() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en"))?;
    Ok((faker.ean13(), faker.unique(false).isbn13()?))
}
```

### 文件样本 {#file-samples}

`mimeType()` 从固定 FakerPHP 目录选择 MIME 类型。`fileExtension()` 先选 MIME 分组，再返回不带点的扩展名。两种方法及版本 3 的 `uuid()` 均支持 `unique`。`file(sourceDirectory, targetDirectory, fullPath)` 从现有目录的顶层选取可读文件，复制到另一个现有目录，生成的文件名保留源扩展名。Rust 使用显式路径，失败时返回 I/O 错误。

```rust
use std::{io, path::{Path, PathBuf}};
use openrust::database::eloquent::factories::Faker;

fn copy_sample(faker: &Faker, source: &Path, target: &Path) -> io::Result<PathBuf> {
    faker.file(source, target, false)
}
```

### 占位图片 {#placeholder-images}

用 `imageUrl(&ImageOptions)` 生成 FakerPHP 占位图片 URL。默认是 640 × 480 的 PNG，随机生成文字和背景，也支持 JPG 和 JPEG。`image(directory, fullPath, options).await` 下载图片，省略目录时使用系统临时目录。HTTP 请求失败或响应中断时返回空路径，无效选项及本地写入失败返回 I/O 错误。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::{ImageOptions, fake}};

fn placeholder_sample() -> DatabaseResult<String> {
    fake(None)?.imageUrl(&ImageOptions {
        width: 320,
        height: 200,
        gray: true,
        randomize: false,
        ..ImageOptions::default()
    })
}
```

### 地区身份编号样本 {#regional-identity-samples}

用 `en-US` 的 `ssn()`、`en-GB` 的 `nino()`，以及 `zh-TW` 或 `zh-Hant` 的 `personalIdentityNumber(gender)` 生成测试数据。美国和英国编号沿用固定 FakerPHP 的范围与前缀规则。台湾编号计算校验位，性别可传 `Some("male")`、`Some("female")` 或 `None` 随机生成。三个方法均支持 `unique`，不支持的语言会返回错误。

`ssn()` 也支持 `th-TH`、`de-AT` 与 `is-IS`。泰语使用和美国英语相同的 FakerPHP 格式。奥地利和冰岛格式从最近 100 年内采样日期并计算校验位。使用 `de-AT` 时，可调用 `ssnWithBirthdate(Some(date))` 传入 `chrono::NaiveDate`，或用 `None` 采样日期。两个 SSN 方法共用 `unique` 历史。

`personalIdentityNumber(gender)` 也支持 `lt-LT`、`lv-LV`、`fi-FI`、`nb-NO`、`sv-SE` 和 `pl-PL`。前五种可通过 `personalIdentityNumberWithBirthdate(birthdate, gender)` 传入出生日期。立陶宛可用 `personalIdentityNumberWithSerial(birthdate, gender, serial)` 指定序号前缀。波兰采用身份证格式。固定版挪威 provider 的最后两位随机生成。

使用 `pl-PL` 时，`pesel(birthdate, sex)` 生成 11 位 PESEL。可传入 1800 至 2299 年的 `chrono::NaiveDate`，或传入 `None` 采样出生日期。`Some("F")` 使用偶数性别位，`Some("M")` 和 `None` 沿用 FakerPHP 的奇数性别位。`taxpayerIdentificationNumber()` 生成 10 位 NIP。两个方法均计算校验位并支持 `unique`。

使用 `pt-BR` 时，可用 `cpf(formatted)`、`rg(formatted)` 和 `cnpj(formatted)`。传入 `None` 使用默认标点，传入 `Some(false)` 只返回数字。RG 沿用固定的圣保罗 provider，校验位为数字。CNPJ 使用分支号 `0001`。三个方法均支持 `unique`。

`dni()` 在 `es-ES` 生成八位数字和模 23 校验字母，在 `es-PE` 生成八位数字。秘鲁的 `ruc(isNaturalPerson)` 默认使用 `20` 前缀，传入 `Some(true)` 则使用 `10`，固定版生成器随后添加九位随机数字。`licenceCode()` 按 `es-ES` 或 `en-ZA` 采样驾驶证类别。上述方法均支持 `unique`。

`idNumber()` 支持 `nl-NL`、`en-ZA`、`ar-SA` 和 `mn-MN`。南非可用 `idNumberWithBirthdate(birthdate, citizen, gender)` 指定出生日期、公民身份和性别。沙特的 `nationalIdNumber()` 固定使用前缀 `1`，`foreignerIdNumber()` 固定使用前缀 `2`。埃及可用 `nationalIdNumberWithGender(gender)` 控制登记序号奇偶，或用 `nationalIdNumber()` 采用默认规则。埃及生成器的末位随机生成。蒙古语的 `alphabet()` 返回一个西里尔字母，埃及的 `governorateId()` 返回两位省份代码。上述方法均支持 `unique`。

`passportNumber()` 在 `lt-LT` 返回八位数字，在 `lv-LV` 返回两个小写字母和七位数字。`tcNo()` 沿用固定土耳其生成器，允许前导零，并保留可能在后缀产生负号的上游带符号余数。`Faker::tcNoChecksum(prefix)` 接收九位 ASCII 数字，`Faker::tcNoIsValid(number)` 比较两字符后缀，前缀无效时返回错误。两个生成方法均支持 `unique`。

`en-US` 的 `ein()` 返回两位前缀、连字符和七位后缀。`ru-RU` 的 `inn10(area)` 和旧名 `inn(area)` 生成十位 INN。传入 `None`、空字符串或零时，从 01 至 91 抽样地区编号。显式地区编号须用 ASCII 数字表示 1 至 99。`kpp(inn)` 取 INN 前四位并附加 `01001`，缺省或不足四字节时生成新 INN。`Faker::inn10Checksum` 根据九位前缀计算校验位，`Faker::inn10IsValid` 验证完整号码。四个生成方法均支持 `unique`。

`fr-FR` 的 `siret(None)` 按 3–3–3–5 分组生成法国机构编号，传入 `Some(false)` 可取得无空格形式。九位 SIREN 前缀和十四位完整编号分别通过 Luhn 校验。`pl-PL` 的 `regon()` 生成九位企业登记号，`regonLocal()` 以有效的 REGON 为前缀生成十四位本地登记号。两者采用加权模 11 校验，余数为 10 时校验位为零。三个生成方法均支持 `unique`。

`cs-CZ` 的 `ico()` 生成带校验位的八位捷克企业编号。`da-DK` 的 `cvr()` 返回八位数字，`p()` 返回十位数字。`is-IS` 的 `vsk()` 返回五位数字。丹麦和冰岛的格式沿用上游样本规则，首位为非零数字。四个方法均支持 `unique`。

`ar-SA` 的 `companyIdNumber()` 生成以 `700` 开头的十位沙特企业编号。`ar-EG` 的 `companyTaxIdNumber()` 返回十位企业税号，`companyTradeRegisterNumber()` 返回七位商业登记号。埃及两种编号允许前导零。三种格式都附加 Luhn 校验位并支持 `unique`。

`kk-KZ` 的 `businessIdentificationNumber(date)` 返回十二位 BIN。可以传入登记日期指定年月，或传入 `None` 从本年年初至当前时间取样。`en-ZA` 的 `companyNumber()` 从过去五十年至当前时间取样登记年份，返回 `YYYY/NNNNNN/TT`。两者均支持 `unique`。

`zh-Hant` 的 `VAT()` 返回八位台湾税号样本，类型为整数。现有 `vat(...)` 在该地区返回相同格式的数字字符串，不加国家前缀。`el-GR` 的 `doy()` 从 21 个税务办公室抽样，`object()` 从 19 种经营类别抽样。每个方法均支持 `unique`。

`en-US`、`fr-FR`、`it-IT` 和 `pl-PL` 的 `company()` 采用 FakerPHP 公司名称模板，`companySuffix()` 从相应的后缀列表抽样。波兰公司名称还使用固定版上游的波兰姓氏和机构前缀，其余三个地区使用固定 Person 姓氏词表。没有固定公司模板的地区沿用 fake-rs 的公司数据和回退规则。两个方法均支持 `unique`。

`zh-Hans`、`zh-Hant`、`ja-JP` 和 `ko-KR` 的 `company()` 使用对应的 FakerPHP 模板。这四个地区及 `pl-PL` 可用 `companyPrefix()` 抽样机构前缀。用 `zh-Hant` 的 `companyModifier()` 或 `companyEnSuffix()` 抽样名称修饰词或英文后缀。这三个方法对不支持的地区返回错误，并支持 `unique`。日本公司名使用固定日本姓氏词表，`companySuffix()` 返回继承的 `Ltd`，公司名称模板使用前缀。

`ar-EG`、`ar-JO` 和 `ar-SA` 的公司名称使用固定上游的阿拉伯语前缀、地区姓氏与后缀。埃及模板还会使用城市名称。生成器保留后缀词表中的空格。三个地区的 `companyPrefix()` 和 `companySuffix()` 均支持 `unique`。

`de-DE`、`de-AT`、`de-CH` 和 `cs-CZ` 的 `company()` 使用固定上游的地区姓氏、模板和后缀。捷克名称始终带后缀，可以用空格、连字符或 `a` 连接两个姓氏。三个德语地区也会生成仅含姓氏的公司名称。`companySuffix()` 支持 `unique`，保留后缀拼写及原有抽样权重。

`da-DK`、`is-IS`、`fi-FI`、`sv-SE` 和 `nb-NO` 的 `company()` 使用固定上游的地区姓名与后缀词表，保留模板中的连接词、标点和抽样权重。可用 `companySuffix()` 单独抽样后缀。两个方法均支持 `unique`。

`es-AR`、`es-ES`、`es-PE`、`es-VE`、`pt-BR` 和 `pt-PT` 的 `company()` 使用固定上游的地区姓氏和公司模板。`companySuffix()` 使用相应的后缀词表，例如巴西的 `Ltda.` 和葡萄牙的 `Lda.`。`companyPrefix()` 新增西班牙和委内瑞拉。三个方法均支持 `unique`。

`nl-NL` 的 `company()` 组合商品和店铺类型，或在行业名称后接姓氏，并可附加荷兰公司后缀。`nl-BE` 使用比利时姓氏，可附加当地后缀。荷兰姓氏保留原有大小写，包括 `van` 等词。两个地区都支持 `companySuffix()` 和 `unique`。

`hr-HR`、`sk-SK` 和 `hu-HU` 的 `company()` 使用固定上游的地区姓名与模板。克罗地亚名称可使用行业前缀或后缀，`companyPrefix()` 可单独抽样行业前缀。斯洛伐克模板保留连字符和原有的 `and`。匈牙利名称可以仅由姓氏组成。三地均支持 `companySuffix()` 和 `unique`。

`el-GR` 的 `company()` 将希腊姓氏与法定后缀组合。`me-ME` 从固定黑山词表组合企业名称和法定类别，可用 `companyName()` 与 `companyType()` 分别抽样。黑山 `companySuffix()` 返回继承的 `Ltd`。四个方法都支持 `unique`。

`en-GB` 和 `en-ZA` 的 `company()` 组合地区姓氏与继承的 `Ltd` 后缀。`el-CY` 使用姓氏加塞浦路斯法定后缀，或用连字符连接两个姓氏。三地均支持 `companySuffix()` 和 `unique`。

`fr-BE`、`fr-CA`、`fr-CH` 和 `it-CH` 的 `company()` 使用固定上游的地区姓氏和模板。加拿大法语继承法国公司后缀，瑞士法语和意大利语分别使用自己的后缀，例如 `Sàrl` 和 `Sarl`。`company()` 和 `companySuffix()` 均支持 `unique`。

`lt-LT`、`sl-SI` 和 `tr-TR` 的 `company()` 使用固定上游的地区模板。立陶宛将法定类别放在名称前，斯洛文尼亚个体经营名称以 `s.p.` 结尾，土耳其模板可包含经营领域。`tr-TR` 的 `companyField()` 可单独抽样经营领域，并支持 `unique`。

`ru-RU` 和 `kk-KZ` 的公司名组合了法定前缀、名称词素和可选的名称尾部。可用 `companyNameElement()` 与 `companyNameSuffix()` 单独抽样，`companySuffix()` 仍返回继承的 `Ltd`。`uk-UA` 的 `companyName()` 抽样基础名称，`company()` 可加入前缀、引号或连字符后缀。这些地区方法支持 `unique`。

`fa-IR`、`he-IL`、`hy-AM` 和 `ka-GE` 的 `company()` 使用固定上游的地区模板和词表。伊朗公司名称可包含前缀和经营领域，该地区的 `contract()` 抽样劳动合同类型。格鲁吉亚公司名称由前缀、词素和名称尾部组成，也可分别抽样。这些方法支持 `unique`。

`bn-BD` 的 `company()` 组合名称和类别，也可通过 `companyName()` 和 `companyType()` 分别抽样。印度尼西亚（`id-ID`）公司名称由法定前缀和一到两个姓氏组成，部分模板附带后缀。马来西亚（`ms-MY`）使用公司名称词表，或组合行业与地区姓名，并保留上游后缀权重。可单独调用 `companyName()` 和 `industry()`。泰国（`th-TH`）使用本地姓氏和继承的 `Ltd` 后缀，`slogan()` 组合三段泰语词组。这些方法支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn identity_samples() -> DatabaseResult<[String; 73]> {
    let birthdate = chrono::NaiveDate::from_ymd_opt(2000, 2, 29).unwrap();
    let polish = fake(Some("pl-PL"))?;
    let brazilian = fake(Some("pt-BR"))?;
    let russian = fake(Some("ru-RU"))?;
    let inn = russian.inn10(Some("77"))?;
    let kpp = russian.kpp(Some(&inn))?;
    let danish = fake(Some("da-DK"))?;
    let egypt = fake(Some("ar-EG"))?;
    let greek = fake(Some("el-GR"))?;
    let taiwan = fake(Some("zh-Hant"))?;
    let montenegrin = fake(Some("me-ME"))?;
    Ok([
        fake(Some("fa-IR"))?.contract()?.to_owned(),
        fake(Some("ka-GE"))?.companyNameElement()?.to_owned(),
        fake(Some("he-IL"))?.company(),
        fake(Some("ru-RU"))?.companyNameElement()?.to_owned(),
        fake(Some("kk-KZ"))?.companyNameSuffix()?.to_owned(),
        fake(Some("uk-UA"))?.companyName()?.to_owned(),
        fake(Some("lt-LT"))?.company(),
        fake(Some("tr-TR"))?.unique(false).companyField()?.to_owned(),
        fake(Some("ms-MY"))?.unique(false).industry()?.to_owned(),
        fake(Some("th-TH"))?.unique(false).slogan()?,
        fake(Some("fr-CA"))?.company(),
        fake(Some("it-CH"))?.companySuffix(),
        fake(Some("en-GB"))?.company(),
        fake(Some("el-CY"))?.companySuffix(),
        greek.company(),
        montenegrin.companyName()?.to_owned(),
        montenegrin.companyType()?.to_owned(),
        fake(Some("hr-HR"))?.companyPrefix()?.to_owned(),
        fake(Some("hu-HU"))?.company(),
        fake(Some("nl-NL"))?.company(),
        fake(Some("nl-BE"))?.unique(false).companySuffix()?,
        fake(Some("es-VE"))?.companyPrefix()?.to_owned(),
        fake(Some("pt-PT"))?.company(),
        fake(Some("is-IS"))?.company(),
        fake(Some("nb-NO"))?.unique(false).companySuffix()?,
        fake(Some("cs-CZ"))?.company(),
        fake(Some("de-AT"))?.unique(false).companySuffix()?,
        egypt.company(),
        fake(Some("ar-JO"))?.companyPrefix()?.to_owned(),
        fake(Some("ko-KR"))?.company(),
        taiwan.companyModifier()?.to_owned(),
        taiwan.companyEnSuffix()?.to_owned(),
        polish.company(),
        fake(Some("fr-FR"))?.companySuffix(),
        fake(Some("zh-Hant"))?.VAT()?.to_string(),
        greek.doy()?.to_owned(),
        greek.object()?.to_owned(),
        fake(Some("kk-KZ"))?.businessIdentificationNumber(Some(birthdate))?,
        fake(Some("en-ZA"))?.companyNumber()?,
        fake(Some("ar-SA"))?.companyIdNumber()?,
        egypt.companyTaxIdNumber()?,
        egypt.companyTradeRegisterNumber()?,
        fake(Some("cs-CZ"))?.ico()?,
        danish.cvr()?,
        danish.p()?,
        fake(Some("is-IS"))?.vsk()?,
        fake(Some("fr-FR"))?.siret(None)?,
        polish.regon()?,
        polish.unique(false).regonLocal()?,
        fake(Some("en-US"))?.ein()?,
        inn,
        kpp,
        fake(Some("tr-TR"))?.tcNo()?,
        fake(Some("lt-LT"))?.passportNumber()?,
        fake(Some("lv-LV"))?.passportNumber()?,
        fake(Some("mn-MN"))?.idNumber()?,
        fake(Some("en-ZA"))?.idNumberWithBirthdate(Some(birthdate), Some(false), None)?,
        fake(Some("ar-SA"))?.foreignerIdNumber()?,
        fake(Some("es-ES"))?.dni()?,
        fake(Some("es-PE"))?.dni()?,
        fake(Some("es-PE"))?.ruc(Some(true))?,
        fake(Some("en-ZA"))?.licenceCode()?.to_owned(),
        brazilian.cpf(None)?,
        brazilian.rg(Some(false))?,
        brazilian.unique(false).cnpj(None)?,
        polish.pesel(Some(birthdate), Some("F"))?,
        polish.unique(false).taxpayerIdentificationNumber()?,
        fake(Some("en-US"))?.ssn()?,
        fake(Some("de-AT"))?.ssnWithBirthdate(Some(birthdate))?,
        fake(Some("is-IS"))?.ssn()?,
        fake(Some("fi-FI"))?.personalIdentityNumberWithBirthdate(Some(birthdate), None)?,
        fake(Some("en-GB"))?.nino()?,
        fake(Some("zh-Hant"))?.unique(false).personalIdentityNumber(None)?,
    ])
}
```

### 台湾英文公司名称 {#taiwanese-english-company-names}

使用 `zh-TW` 或 `zh-Hant` 时，`companyEn()` 沿用固定上游默认装配缺少 `lastNameEn` 的错误。通过 `companyEnWithLastName` 传入可返回错误的 Rust 闭包，模板每需要一个姓氏就调用一次。两个方法的 `unique` 版本共用 `companyEn` 历史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwanese_company_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant"))?
        .unique(false)
        .companyEnWithLastName(|| Ok("Lin".into()))
}
```

### 职业名称 {#job-titles}

调用 `jobTitle()` 从当前地区的 Company provider 生成职业名称。`de-DE`、`en-US`、`fr-FR`、`id-ID`、`nb-NO`、`nl-NL`、`ru-RU`、`sv-SE`、`tr-TR` 和 `uk-UA` 有独立词表。比利时、加拿大和瑞士的法语地区继承法国词表。Company provider 继承基础职业模板时返回单个 Lorem 词，缺少 Company provider 时使用美国词表。可通过 `unique(false).jobTitle()` 去重。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn job_title_sample() -> DatabaseResult<String> {
    fake(Some("id-ID"))?.unique(false).jobTitle()
}
```

### 偏置采样与生成器修饰器 {#faker-biased-and-modifiers}

用 `biasedNumberBetween(min, max, function)?` 在包含端点的整数范围内采样。传入 `None` 分别使用默认下限 0、上限 100 和平方根曲线。自定义曲线接收 [0, 1) 内的数并返回 0 至 1 的概率。范围反向、概率无效或采样耗尽时返回错误。该方法支持 `unique`。

用 `optional(weight, default, generate)?` 按概率调用类型化生成闭包，或返回可选默认值。默认权重为 0.5，大于 1 时按 FakerPHP 的百分数规则解释。用 `valid(predicate, maxRetries, generate)?` 重试直到满足谓词，默认重试上限为 10,000。两者都会传播生成错误。用 `uniqueWithRetries(reset, maxRetries)` 为共享去重历史的句柄设置尝试上限。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn modified_samples() -> DatabaseResult<(Option<i64>, i64)> {
    let faker = fake(None)?;
    let sample = faker.optional(Some(0.75), Some(0), |faker| {
        faker.biasedNumberBetween(Some(1), Some(100), Some(&|x| x))
    })?;
    let even = faker.valid(|value| value % 2 == 0, None, |faker| {
        Ok(faker.numberBetween(0, 100))
    })?;
    Ok((sample, even))
}
```

### 基于语料的文本 {#corpus-text-samples}

用 `realText(maxNbChars, indexSize)?` 或 `realTextBetween(minNbChars, maxNbChars, indexSize)?` 从固定 FakerPHP 语料按加权 Markov 链生成文本。分别传入 `None` 可使用默认最大长度 200、最小长度 160 或上下文长度 2。上下文长度支持 1 至 5。 `realTextBetween` 的 Unicode 字符数大于最小值，且不超过最大值。简体中文等未提供上游词库的语言回退至英语。繁体中文和日语按字符采样。波斯语 `realText` 使用上游的字节长度。该语料没有大写开头的词，`realTextBetween` 会返回重试耗尽错误。两个方法均支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn corpus_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant"))?.realTextBetween(Some(80), Some(120), Some(2))
}
```

### HTML 样本 {#html-samples}

用 `randomHtml(maxDepth, maxWidth)?` 生成 HTML 文档，其中包含标题、示例登录表单和随机正文子树。两个参数分别传入 `None` 时默认取四。深度小于或等于一时只生成标题和表单。宽度限制每次随机生成的同级节点数。表单和链接使用保留的示例域名。数字 ID 在单份文档内唯一，`unique` 单独记录完整文档。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn html_sample() -> DatabaseResult<String> {
    let faker = fake(None)?;
    faker.randomHtml(Some(3), None)
}
```

### 血型样本 {#blood-group-samples}

用 `bloodType()` 生成 A、AB、B 或 O，`bloodRh()` 生成 Rh 正负号。 `bloodGroup()` 独立采样两部分并直接拼接。各语言使用相同的取值。三个方法均支持 `unique`，并分别记录去重历史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn blood_samples() -> DatabaseResult<(&'static str, &'static str, String)> {
    let faker = fake(None)?;
    Ok((faker.bloodType(), faker.bloodRh(), faker.unique(false).bloodGroup()?))
}
```

### 颜色样本 {#color-samples}

用 `hexColor()` 生成六位 RGB 十六进制颜色。 `safeHexColor()` 沿用固定 FakerPHP 的范围 `#000000` 至 `#00ffff`，每一对十六进制数字相同。 `rgbColor()` 和 `hslColor()` 返回逗号分隔值，对应的 `AsArray` 方法返回三个整数。 `rgbCssColor()` 和 `rgbaCssColor()` 生成 CSS 字符串。 `safeColorName()` 和 `colorName()` 按解析后的语言选择 FakerPHP 词表，缺少对应词表时使用默认词表。十个方法均支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn color_samples() -> DatabaseResult<(String, [u8; 3], &'static str)> {
    let faker = fake(Some("zh-Hant"))?;
    let unique = faker.unique(false);
    Ok((faker.rgbaCssColor(), unique.rgbColorAsArray()?, unique.colorName()?))
}
```

### 浏览器 User-Agent {#browser-user-agent-samples}

用 `userAgent()?` 随机生成浏览器样本，也可用 `firefox()?`、`chrome()`、`internetExplorer()`、`opera()`、`safari()` 或 `msedge()` 指定格式。样本沿用固定 FakerPHP 的浏览器版本和平台标识。Firefox 按应用时区采样 Gecko 日期。用 `macProcessor()`、`linuxProcessor()`、`windowsPlatformToken()`、`macPlatformToken()`、`linuxPlatformToken()` 和 `iosMobileToken()` 单独生成标识。以上格式化方法均支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn browser_samples() -> DatabaseResult<(String, String)> {
    let faker = fake(None)?;
    Ok((faker.userAgent()?, faker.unique(false).msedge()?))
}
```

### 银行标识 {#bank-identifier-samples}

用 `iban(countryCode, prefix, length)` 生成带模 97 校验位的 IBAN 样本。传入 `None` 随机选择国家，或指定两字母代码。已收录国家使用固定 FakerPHP 格式，未收录代码使用指定长度的数字账号，默认 24 位。前缀替换账号开头并保留前导零。Rust 接受由 ASCII 大写字母和数字组成的前缀，账号部分最长 30 个字符。`swiftBicNumber()` 生成 8 或 11 个字符的样本，两者均支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn bank_samples() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en"))?;
    let unique = faker.unique(false);
    Ok((unique.iban(Some("DE"), "0012", None)?, unique.swiftBicNumber()?))
}
```

### 地区银行账号 {#regional-bank-account-samples}

用 `bankAccountNumber(prefix, countryCode, length)` 按 Faker 地区生成账号。支持 IBAN 别名的地区默认使用本国代码，可显式覆盖参数。埃及地区固定生成埃及 IBAN。美国英语、马来西亚马来语和尼泊尔语生成本地账号。这四种地区传入 `("", None, None)`。`bankRoutingNumber()` 为 `en_US` 生成九位路由号样本。`Faker::calculateRoutingNumberChecksum` 读取前八位并返回校验位。不支持的地区返回错误。两种生成方法均支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_bank_samples() -> DatabaseResult<(String, String)> {
    let uk = fake(Some("en-GB"))?;
    let us = fake(Some("en-US"))?;
    Ok((
        uk.bankAccountNumber("BANK", None, None)?,
        us.unique(false).bankRoutingNumber()?,
    ))
}
```

### 金融机构 {#institution-samples}

`bank()` 在 `de_DE`、`el_CY`、`ka_GE`、`kk_KZ`、`ms_MY`、`pl_PL`、`pt_BR`、`ru_RU`、`th_TH`、`uk_UA` 和 `zh_CN` 使用固定地区目录。马来西亚马来语还提供 `localBank`、`foreignBank`、`governmentBank`、`insurance` 和 `currencySymbol`。尼泊尔语提供 `commercialBank`、`developmentBank`、`financeCompany`、`microFinance` 和 `digitalWallet`。两个地区的 `swiftCode()` 使用各自的目录。这些生成器支持 `unique`，不支持的地区返回错误。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn institution_samples() -> DatabaseResult<(&'static str, String)> {
    let faker = fake(Some("ms-MY"))?;
    Ok((faker.foreignBank()?, faker.unique(false).swiftCode()?))
}
```

### VAT 样本 {#vat-samples}

用 `vat(None)` 采用地区默认格式。对支持空格选项的地区，可用 `vat(Some(false))` 生成紧凑格式。支持 `bg_BG`、`de_AT`、`es_ES`、`fr_BE`、`nl_BE`、`fr_FR`、`en_GB`、`it_IT` 和 `nl_NL`。英国的 `vatWithType` 接受 `branch`、`gov` 和 `health`，其他值使用标准格式。法国 VAT 复用 `siren`，意大利 `vatId` 和荷兰 `btw` 沿用各自的生成规则。各生成器支持 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vat_samples() -> DatabaseResult<(String, String)> {
    let french = fake(Some("fr-FR"))?;
    let uk = fake(Some("en-GB"))?;
    Ok((french.unique(false).vat(None)?, uk.vatWithType(Some("branch"))?))
}
```

### 信用卡样本 {#credit-card-samples}

信用卡生成器提供品牌、带 Luhn 校验位的卡号、到期日和完整样本。卡号格式化时默认用连字符分组。`pt-BR` 使用巴西品牌格式。到期日期传入 `true` 时从现在到未来 36 个月取样，传入 `false` 时从过去 36 个月到未来 36 个月取样。`creditCardExpirationDateString` 接受 Chrono 格式指令，默认 `%m/%y`。五个方法均支持 `unique`。

`creditCardType()` · `creditCardNumber(type, formatted, separator)` · `creditCardExpirationDate(valid)` · `creditCardExpirationDateString(valid, format)` · `creditCardDetails(valid)`

```rust
use openrust::database::{DatabaseResult, eloquent::factories::{CreditCardDetails, fake}};

fn card_samples() -> DatabaseResult<(CreditCardDetails, String)> {
    let faker = fake(Some("pt-BR"))?;
    let details = faker.unique(false).creditCardDetails(true)?;
    let number = faker.creditCardNumber(Some("Elo"), true, None)?;
    Ok((details, number))
}
```

### 邮寄地址和称谓 {#address-and-title-samples}

用 `streetAddress()` 生成街道行，或用 `address()` 生成完整地址样本。美国、法国、简体中文和繁体中文使用各自的上游 Faker 排列规则。城市、街道和部分地址组件仍使用 fake-rs 数据。单独字段可用 `buildingNumber()`、`secondaryAddress()`、`postcode()`、`state()` 和 `stateAbbr()`。`title(Some("male"))` 和 `title(Some("female"))` 指定称谓组，`title(None)` 在两组中随机选择，也可直接调用 `titleMale()` 和 `titleFemale()`。这些方法均支持 `unique`。

### 按性别生成名字 {#given-names-by-gender}

用 `firstNameMale()` 或 `firstNameFemale()` 从对应的上游 Faker 词库抽取名字。`firstNameWithGender(Some("male"))` 和 `nameWithGender(Some("female"))` 显式选择分组。传入 `None` 或未识别的分组会在两组抽样。全名按地区模板组合现有姓氏数据，繁体中文名字由一个或两个抽取字符组成。缺少地区词库时使用英文名字及模板。

`en-US`、`fr-FR`、`it-IT` 和 `ja-JP` 的 `name()`、`firstName()` 与 `lastName()` 使用固定 Person 模板和词表。公司模板共享同一套姓氏。`firstNameWithGender` 和 `nameWithGender` 与对应的无参方法共享 `unique` 历史。

`ar-SA`、`de-DE`、`fa-IR`、`nl-NL`、`pt-BR`、`pt-PT`、`tr-TR`、`zh-Hans` 和 `zh-Hant` 也使用对应的 Person 模板和词表。荷兰 Person 与 Company 使用同一套姓氏抽样规则，长姓、常见荷兰姓、常见外来姓和一般荷兰姓的权重依次为 1:10:5:10。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_name_sample() -> DatabaseResult<String> {
    fake(Some("nl-NL"))?.unique(false).name()
}
```

### 北欧姓名 {#nordic-names}

丹麦和冰岛的姓名模板可以包含中间名。可用 `middleName()` 单独生成。`is-IS` 的 `lastName()` 返回父名词干，`lastNameMale()` 和 `lastNameFemale()` 添加对应后缀。芬兰姓名可包含本地称谓。瑞典语和挪威书面语姓名使用名字加姓氏。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nordic_name_sample() -> DatabaseResult<String> {
    fake(Some("is-IS"))?.unique(false).nameWithGender(Some("female"))
}
```

### 波兰和立陶宛姓名 {#polish-and-lithuanian-names}

`pl-PL` 和 `lt-LT` 可通过 `lastNameWithGender(Some("male"))` 或 `lastNameWithGender(Some("female"))` 选择姓氏词表。`lastNameMale()` 和 `lastNameFemale()` 使用相同词表。固定版立陶宛的无参 `lastName()` 从名字抽样，全名则使用对应性别的姓氏。`lastNameWithGender` 和 `lastName` 共享去重历史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn gendered_surname_sample() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).lastNameWithGender(Some("female"))
}
```

### 英国、南非、塞浦路斯与法语及意大利语地区姓名 {#additional-regional-names}

`en-GB`、`en-ZA`、`el-CY`、`fr-BE`、`fr-CA`、`fr-CH` 和 `it-CH` 使用地区 Person 的名字、姓氏和实际生效的全名模板。瑞士法语和意大利语沿用各自基类的模板和称谓，使用瑞士本地姓名词表。Person 与 Company 共用姓氏词表。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_person_sample() -> DatabaseResult<String> {
    fake(Some("fr-CH"))?.unique(false).nameWithGender(Some("female"))
}
```

### 西语地区姓名 {#spanish-language-names}

`es-AR`、`es-ES`、`es-PE` 和 `es-VE` 使用固定 Person 的名字、姓氏和称谓。全名保留各地区的可选姓名后缀和模板权重。委内瑞拉模板可以组合多个名字或姓氏。Company 共用姓氏词表。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn spanish_name_sample() -> DatabaseResult<String> {
    fake(Some("es-VE"))?.unique(false).nameWithGender(Some("female"))
}
```

### 中欧地区姓名 {#central-european-names}

`de-AT`、`de-CH`、`cs-CZ`、`sk-SK`、`hu-HU` 和 `hr-HR` 使用固定 Person 模板，并与 Company 共用姓氏词表。捷克和斯洛伐克支持 `lastNameWithGender`、`lastNameMale` 和 `lastNameFemale`。匈牙利模板保留姓氏在前和婚后姓名格式，可通过 `firstNameMaleNe()` 与 `lastNameFemaleMarried()` 单独抽样。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn central_european_name_sample() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).lastNameWithGender(Some("female"))
}
```

### 希腊、保加利亚和斯洛文尼亚姓名 {#balkan-names}

`el-GR` 和 `bg-BG` 使用对应性别的姓氏词表和全名模板。可用 `lastNameWithGender`、`lastNameMale` 或 `lastNameFemale` 选择姓氏。斯洛文尼亚两种姓氏方法使用同一词表。`title(gender)` 使用统一的本地称谓，`titleMale()` 与 `titleFemale()` 沿用基类称谓。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn balkan_name_sample() -> DatabaseResult<String> {
    fake(Some("bg-BG"))?.unique(false).nameWithGender(Some("female"))
}
```

### 埃及及西亚地区姓名 {#egyptian-and-west-asian-names}

`ar-EG`、`ar-JO`、`he-IL`、`hy-AM` 和 `ka-GE` 使用固定 Person 的本地名字、姓氏和称谓。埃及全名模板可包含缩写前缀。亚美尼亚和格鲁吉亚按名字加姓氏的模板生成全名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn west_asian_name_sample() -> DatabaseResult<String> {
    fake(Some("ka-GE"))?.unique(false).name()
}
```

### 俄语、乌克兰语和哈萨克语姓名 {#russian-ukrainian-and-kazakh-names}

`ru-RU`、`uk-UA` 和 `kk-KZ` 使用固定 Person 的地区词表和全名模板。俄语和乌克兰语的 `middleName()` 从两组父名取样，`middleNameWithGender`、`middleNameMale()` 和 `middleNameFemale()` 可选择分组。俄语姓氏支持性别选择并保留固定的女性后缀。哈萨克语 PHP provider 使用数字性别常量。Rust 沿用统一的 `"male"` 和 `"female"` 选择对应模板。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn patronymic_sample() -> DatabaseResult<&'static str> {
    fake(Some("uk-UA"))?.unique(false).middleNameWithGender(Some("female"))
}
```

### 南亚、东南亚和韩国姓名 {#south-southeast-asian-and-korean-names}

`bn-BD`、`id-ID`、`ms-MY`、`ko-KR`、`th-TH` 和 `vi-VN` 使用各地的姓名词表及全名模板。印度尼西亚姓氏支持性别选择。韩文全名姓氏在前，中间无空格。越南语 `middleName()` 从名字词表抽样，显式传入性别的 `middleNameWithGender` 使用对应的中间名词表。马来西亚 `name()` 使用马来、华人和印度裔全名模板，`firstName()` 还包含基督教名字。显式选择男性或女性名字时，沿用固定 provider 继承的 `John` 和 `Jane`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vietnamese_middle_name_sample() -> DatabaseResult<&'static str> {
    fake(Some("vi-VN"))?.unique(false).middleNameWithGender(Some("female"))
}
```

`ms-MY` 可直接生成姓名模板的各个成分。马来姓名使用 `firstNameMaleMalay`、`firstNameFemaleMalay`、`lastNameMalay`、`muhammadName`、`nurName`、`haji`、`hajjah` 和 `titleMaleMalay`。其余接口包括 `lastNameChinese`、`firstNameMaleChinese`、`firstNameFemaleChinese`、`firstNameMaleChristian`、`firstNameFemaleChristian`、`initialIndian`、`firstNameMaleIndian`、`firstNameFemaleIndian` 和 `lastNameIndian`。可选成分可以为空，非空时保留词表原有的末尾空格。

`myKadNumber(gender, formatted, separator)` 生成样本号码。关闭格式化时输出 12 位数字，开启后用连字符或指定分隔符分为六位、两位和四位三组。末位在 `"male"` 时为奇数，`"female"` 时为偶数，其余情况不限制。月、日分别抽样。以上接口均支持 `unique()`，在 `ms-MY` 以外的地区返回错误。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn malaysian_person_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("ms-MY"))?;
    let unique = faker.unique(false);
    Ok((unique.firstNameMaleMalay()?, unique.myKadNumber(Some("male"), true, None)?))
}
```

### 罗马尼亚语和塞尔维亚语姓名 {#romanian-and-serbian-names}

`ro-RO` 和 `ro-MD` 共享罗马尼亚语姓名及称谓词表。`sr-RS` 与 `sr-Cyrl-RS` 使用各自的西里尔文字词表，`sr-Latn-RS` 使用拉丁文字词表。

在 `ro-RO` 调用 `cnp(gender, dateOfBirth, county, isResident)` 可生成 13 位样本号码。出生日期可省略，也可在 1800–2099 年内使用 `YYYY`、`YYYY-MM` 或 `YYYY-MM-DD`。日期缺少的字段取自该年随机一天。抽到的日若超出指定月份的天数，调用返回错误。县代码区分大小写。该方法计算校验位并支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn romanian_cnp_sample() -> DatabaseResult<String> {
    fake(Some("ro-RO"))?.unique(false).cnp(Some("female"), Some("1981-06-16"), Some("B1"), true)
}
```

### 爱沙尼亚、拉脱维亚和黑山姓名 {#estonian-latvian-and-montenegrin-names}

`et-EE`、`lv-LV` 和 `me-ME` 使用各地的名字和姓氏词表，沿用名字在前的基础模板。拉脱维亚 `driverLicence()` 输出两个小写 ASCII 字母和六位数字。护照号码使用七位数字。可用 `personalIdentityNumberWithBirthdate` 传入公历出生日期。这些身份格式化器均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn latvian_driver_licence() -> DatabaseResult<String> {
    fake(Some("lv-LV"))?.unique(false).driverLicence()
}
```

### 印度英语和尼泊尔语姓名 {#indian-english-and-nepali-names}

`en-IN` 和 `ne-NP` 使用各地的名字、姓氏和全名模板。印度英语提供 `middleNameMale()`，尼泊尔语同时提供 `middleNameMale()` 和 `middleNameFemale()`。可直接调用，或向 `middleNameWithGender` 传入对应性别。两个地区调用无参 `middleName()` 都会返回该地区不支持此接口的错误。按性别生成的接口均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nepali_middle_name() -> DatabaseResult<&'static str> {
    fake(Some("ne-NP"))?.unique(false).middleNameFemale()
}
```

### 尼日利亚和乌干达英语姓名 {#nigerian-and-ugandan-english-names}

`en-NG` 和 `en-UG` 使用各地的姓名词表和模板。尼日利亚全名可含一个或两个名字，姓氏在后。乌干达模板既可将名字放在前面，也可将姓氏放在前面。乌干达无参 `lastName()` 从通用姓氏词表抽样。可用 `lastNameMale()`、`lastNameFemale()` 或向 `lastNameWithGender` 传入明确的性别，从对应词表抽样。这些接口均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ugandan_surname() -> DatabaseResult<String> {
    fake(Some("en-UG"))?.unique(false).lastNameFemale()
}
```

### 蒙古语姓名 {#mongolian-names}

`mn-MN` 全名使用一个西里尔首字母、句点和名字，句点后没有空格。`namePrefix()` 从 30 个姓名首字母中抽样。`alphabet()` 使用 `idNumber()` 的独立 35 字母词表。两个接口均返回 Rust `char`，并支持 `unique()`。单独调用姓氏生成接口会返回基础 provider 的 `Doe`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn mongolian_initial() -> DatabaseResult<char> {
    fake(Some("mn-MN"))?.unique(false).namePrefix()
}
```

### 新加坡英语姓名和证件样本 {#singapore-english-names-and-identity-samples}

`en-SG` 沿用基础姓名 `John Doe` 和 `Jane Doe`。用 `nric(birthDate)` 或 `fin(issueDate)` 生成证件样本，也可用 `singaporeId(issueDate, foreigner)` 指定类型。日期接受可选的公历日期，省略时使用 `dateTimeThisCentury()` 的日期范围。前缀以 2000 年为分界。NRIC 从 1968 年起取年份后两位，更早的日期从 `00` 和 `01` 抽样。三个接口均计算末尾校验字母并支持 `unique()`。

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn singapore_identity_sample() -> DatabaseResult<String> {
    fake(Some("en-SG"))?.unique(false).nric(NaiveDate::from_ymd_opt(2000, 1, 1))
}
```

### 比利时荷兰语姓名和 RRN 样本 {#belgian-dutch-names-and-rrn-samples}

`nl-BE` 使用当地名字和姓氏词表，并沿用名字在前的基础模板。`rrn(gender)` 使用 `dateTimeThisCentury()` 抽取出生日期并生成 11 位样本。三位序号在 `"male"` 时为奇数，`"female"` 时为偶数，其他情况不限。末尾两位按模 97 计算，2000 年起的出生日期使用对应的世纪偏移。该接口支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn belgian_rrn_sample() -> DatabaseResult<String> {
    fake(Some("nl-BE"))?.unique(false).rrn(Some("female"))
}
```

### 姓名前缀和后缀 {#person-name-prefixes-and-suffixes}

在提供对应格式化器的地区使用 `prefix()` 和 `suffix()`。默认 `en-US` 提供 `suffix()`，与全名模板共用后缀词表。意大利语 `it-IT` 和 `it-CH` 的上游空值以 `None` 返回，亚美尼亚语和台湾华语返回 `Some("")`。不支持该接口的地区返回错误。两个方法均支持 `unique()`，每次去重历史中空值或空字符串各只能返回一次。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_affixes() -> DatabaseResult<(&'static str, Option<&'static str>)> {
    Ok((fake(Some("fr-FR"))?.prefix()?, fake(None)?.suffix()?))
}
```

### 日语假名姓名 {#japanese-kana-names}

在 `ja-JP` 上调用 `kanaName(gender)` 可生成片假名全名，姓氏在前，以一个空格分隔。`firstKanaName(gender)` 生成名字，`lastKanaName()` 生成姓氏。可传入 `Some("male")`、`Some("female")` 或 `None`，分别选择男性、女性或任一名字词表。`firstKanaNameMale()` 和 `firstKanaNameFemale()` 直接选择对应词表。五个接口均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn japanese_kana_name() -> DatabaseResult<String> {
    fake(Some("ja-JP"))?.unique(false).kanaName(Some("female"))
}
```

### 瑞士 AHV 和 AVS 样本 {#swiss-ahv-and-avs-samples}

用 `de-CH`、`fr-CH` 或 `it-CH` 的 `avs13()` 生成 `756.####.####.##` 格式的瑞士社会保险号码样本。瑞士德语还提供 `ahv13()`，格式和 EAN 校验位相同。两个接口均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn swiss_avs_sample() -> DatabaseResult<String> {
    fake(Some("fr-CH"))?.unique(false).avs13()
}
```

### 捷克出生号码和丹麦 CPR {#czech-birth-numbers-and-danish-cpr}

在 `cs-CZ` 上调用 `birthNumber(gender, minAge, maxAge, slashProbability)` 生成捷克出生号码样本，数值参数指定年龄上下限和出现斜杠的百分比。`birthNumberMale()` 和 `birthNumberFemale()` 默认使用 0 到 100 岁及 50% 的斜杠概率。生成器保留女性月份偏移、2004 年起的额外月份偏移和 1954 年起的校验位。年龄上下限倒置时返回错误。`da-DK` 的 `cpr()` 抽取公历出生日期及首位非零的四位序号。两地接口均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn czech_birth_number() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).birthNumber(Some("female"), 18, 65, 100)
}
```

### 法国 NIR 和意大利税号样本 {#french-nir-and-italian-tax-id-samples}

在 `fr-FR` 或 `fr-CH` 中使用 `nir(gender, formatted)`。用 `Some("male")` 或 `Some("female")` 选择首位，或用 `None` 随机选择。NIR 使用法国省级行政区代码和模 97 校验位。紧凑格式有 15 个字符，带空格格式按 1–2–2–2–3–3–2 分组。在 `it-IT` 或 `it-CH` 中使用 `taxId()` 生成 FakerPHP 的 16 位大写税号样本。两个接口均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn french_nir_sample() -> DatabaseResult<String> {
    fake(Some("fr-FR"))?.unique(false).nir(Some("female"), true)
}
```

### 伊朗国民号码和印度尼西亚 NIK 样本 {#iranian-national-codes-and-indonesian-nik-samples}

在 `fa-IR` 用 `nationalCode()` 生成十位样本，包含非零地区代码和模 11 校验位。在 `id-ID` 用 `nik(gender, birthDate)` 生成 16 位样本，开头取自固定出生地代码词表，号码内包含出生日期。女性样本的出生日期日数加 40。日期传入 `None` 时从过去 30 年抽样，也可传入 `NaiveDate` 指定公历日期。两个接口均支持 `unique()`。

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn indonesian_nik_sample() -> DatabaseResult<String> {
    fake(Some("id-ID"))?.unique(false).nik(Some("female"), NaiveDate::from_ymd_opt(2000, 2, 29))
}
```

### 哈萨克斯坦 IIN 和委内瑞拉身份证号样本 {#kazakh-iin-and-venezuelan-national-id-samples}

在 `kk-KZ` 中使用 `individualIdentificationNumber(birthDate, gender)`。生日接受 1800–2099 年的可选公历日期，默认从过去 30 年抽样，性别默认男性。`Faker::checkSum(prefix)` 接收 11 位 ASCII 数字并返回校验值，两轮均得到 10 时，生成器会重新抽取序号。在 `es-VE` 中用 `nationalId(separator)` 设置 `V` 或 `E` 后的分隔符。两个生成器均支持 `unique()`。

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn kazakh_iin_sample() -> DatabaseResult<String> {
    fake(Some("kk-KZ"))?.unique(false)
        .individualIdentificationNumber(NaiveDate::from_ymd_opt(2000, 2, 29), Some("female"))
}
```

使用 `es-VE` 的 `taxpayerIdentificationNumber()` 生成委内瑞拉 RIF 样本。格式由六种前缀之一（`J`、`G`、`V`、`E`、`P`、`C`）、八位主体数字和一位随机尾数组成。用 `taxpayerIdentificationNumberWithSeparator(Some("-"))` 在主体两侧加入分隔符。两个入口共用 `unique()` 的去重记录。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn venezuelan_rif_sample() -> DatabaseResult<String> {
    fake(Some("es-VE"))?
        .unique(false)
        .taxpayerIdentificationNumberWithSeparator(Some("-"))
}
```

### 葡萄牙 NIF 和台湾华语名字词表 {#portuguese-nif-and-taiwanese-name-pools}

在 `pt-PT` 中用 `taxpayerIdentificationNumber()` 生成九位 NIF 样本。`Faker::dvCalcMod11(number)` 接收 ASCII 数字字符串计算校验位，可保留前导零。在 `zh-TW` 中，`randomName(pool, count)` 从字符串切片独立抽样后拼接，可以重复抽到同一项。词表为空或抽样数为零时返回空字符串。两个生成器均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwanese_name_sample() -> DatabaseResult<String> {
    fake(Some("zh-TW"))?.randomName(&["明", "安"], 2)
}
```

### 波罗的海驾驶执照和护照样本 {#baltic-driving-licence-and-passport-samples}

`lt-LT` 和 `lv-LV` 均支持 `driverLicence()` 与 `passportNumber()`。立陶宛两种样本都是八位 ASCII 数字，可保留前导零。拉脱维亚样本以两个小写 ASCII 字母开头，驾驶执照后接六位数字，护照后接七位数字。两个接口均支持 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn baltic_driver_licence() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).driverLicence()
}
```

### Person 地区回退 {#person-locale-fallback}

当指定地区没有 Person provider 时，姓名接口、`suffix()` 和 `ssn()` 使用 `en-US` Person provider。已有 Person provider 的地区使用自身和继承的接口。例如，`en-CA` 会回退到美国英语 Person，`en-SG` 有自己的 Person provider，调用 `ssn()` 返回不支持错误。此回退仅适用于 Person 接口。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_fallback_sample() -> DatabaseResult<String> {
    fake(Some("en-CA"))?.unique(false).ssn()
}
```

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

`state` 按声明顺序叠加。`sequence` 从零开始，同一构建器上的各次调用接续其索引。新构建器重新从零开始。并发调用获得不同索引，按实际执行顺序分配。`make` 创建内存模型并运行 `afterMaking`。`create` 保存模型、创建已声明的子关系，然后运行 `afterCreating`。调用 `count` 后返回 Eloquent 集合。工厂属性绕过批量赋值过滤，并继续执行模型修改器与属性转换。

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

用 `makeOne(attributes)` 构造一个未保存的模型，或用 `makeMany(records)` 按顺序构造多个未保存的模型。两者都接受显式属性并执行 `afterMaking`。`makeMany` 使用同样的 `FactoryRecords` 选项，数量为零时返回空集合。`makeMany` 和 `createMany` 都会消耗已配置的默认数量。显式指定记录、零条及模型生成失败时同样生效。后续调用默认处理一条记录，Rust 批量接口仍返回集合形状。两种方法均不保存返回的模型，解析已声明的上级关系时仍可能写入上级模型。对应的 `On` 方法可指定连接。`lazy(attributes)` 返回异步回调，调用并等待后才解析连接并创建单个模型或指定数量的批次。重复调用会重新创建。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::{Factory, FactoryRecords}},
};

async fn consume_factory_count<M: Model>() -> DatabaseResult<()> {
    let batch = Factory::<M>::new(Attributes::new).count(3);
    assert_eq!(batch.makeMany(FactoryRecords::Default).await?.len(), 3);
    assert_eq!(batch.makeMany(FactoryRecords::Default).await?.len(), 1);
    Ok(())
}
```

`makeOne` 保留已配置的批量数量，供下一次 `makeMany` 使用。示例中的两次默认批量调用依次返回两个模型和一个模型。预构造消耗数量后，用独立的批量工厂配置 `lazy` 的写入数量。创建或调用回调后，目标表在等待返回的 Future 前没有写入。下面每次调用并等待都会写入两个新模型。未设置批量数量的工厂返回的 `lazy` 回调，每次成功执行会创建并返回一个模型。示例还验证逐条属性覆盖，并等待单模型回调。每次调用并等待都会重新创建。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{
        Attributes, Model,
        factories::{Factory, FactoryRecords},
    },
};

async fn preview_and_seed<M: Model>() -> DatabaseResult<()> {
    let factory = || Factory::<M>::new(|| {
        Attributes::from([("name".into(), "Flight".into())])
    });
    let previews = factory().count(2);

    let one = previews.makeOne(Attributes::new()).await?;
    assert!(!one.state().exists());
    let many = previews.makeMany(FactoryRecords::Default).await?;
    assert_eq!(many.len(), 2);
    assert!(many.iter().all(|model| !model.state().exists()));
    assert_eq!(previews.makeMany(FactoryRecords::Default).await?.len(), 1);
    assert!(previews.makeMany(FactoryRecords::Count(0)).await?.is_empty());

    let names = FactoryRecords::Attributes(vec![
        Attributes::from([("name".into(), "Morning flight".into())]),
        Attributes::from([("name".into(), "Evening flight".into())]),
    ]);
    let custom = previews.makeMany(names).await?;
    assert_eq!(custom[0].getAttribute("name"), Some(&"Morning flight".into()));
    assert_eq!(custom[1].getAttribute("name"), Some(&"Evening flight".into()));

    let batch = factory().count(2);
    let seed = batch.lazy(Attributes::new());
    let pending = seed();
    assert_eq!(pending.await?.len(), 2);
    assert_eq!(seed().await?.len(), 2);

    let single = factory();
    let seed_one = single.lazy(Attributes::new());
    assert!(seed_one().await?.state().exists());
    Ok(())
}
```

用 `forEachSequence(states)` 为每个模型应用一组属性，并按属性组数确定批次数量。`crossJoinSequence(sequences)` 按声明顺序组合各序列，最后一个序列变化最快，同名属性以后面的序列为准。单独设置批次数量，超过组合数时从头循环。传入空序列会返回错误。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, factories::Factory}};

async fn sequence_lifecycle<M: Model>() -> DatabaseResult<()> {
    let factory = Factory::<M>::new(Attributes::new)
        .sequence(|index| Attributes::from([("label".into(), index.to_string().into())]));
    assert_eq!(factory.raw().await?["label"], "0".into());
    assert_eq!(factory.raw().await?["label"], "1".into());
    Ok(())
}
```

在单模型或批量工厂上使用 `insert(attributes)` 批量写入生成的行，或用 `insertOn(connection, attributes)` 指定连接。工厂会执行 `afterMaking` 并解析所需的上级键。批量写入跳过目标模型的持久化事件、子关系创建及 `afterCreating`。写入前，工厂通过读取访问器和序列化转换投影属性，仍按可见白名单筛选，包含符合条件的隐藏列并跳过追加字段。插入构建器随后重新应用修改器与写入转换。Rust 投影保留数据库值类型，包括二进制数据。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, factories::Factory}};

async fn insert_projected<M: Model>(factory: Factory<M>) -> DatabaseResult<()> {
    factory.count(2).insert(Attributes::new()).await
}
```

单条和批量工厂都可以用 `makeWithParent(parent, attributes)` 和 `insertWithParent(parent, attributes)` 把父模型交给匹配的 `stateWithParent` 闭包。对应的 `On` 方法可指定连接。父模型作为状态上下文传入，需要建立关联时，由状态或调用属性赋予外键。

单条或批量工厂可以调用 `createWithParent(parent, attributes)`、`createQuietlyWithParent(parent, attributes)` 或 `lazyWithParent(parent, attributes)`。类型匹配的 `stateWithParent` 和 `afterCreatingWithParent` 可以读取借用的父模型。即时创建的 `On` 变体可指定连接，延迟回调在等待时解析连接。父模型本身不会添加外键。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, factories::Factory}};

async fn create_for_parent<M: Model, P: Model>(
    factory: &Factory<M>,
    parent: &P,
    attributes: Attributes,
) -> DatabaseResult<M> {
    let create = factory.lazyWithParent(parent, attributes);
    create().await
}
```

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

### 延迟计算的工厂属性 {#deferred-factory-attributes}

用 `attribute` 在应用工厂状态和显式覆盖后计算单个属性。按依赖顺序登记属性。用 `parentAttributeWith` 根据当前属性构建关联工厂。同名显式值优先，包括设为 null 的外键。上级关系排除规则也适用于 `parentAttributeWith`。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn deferred_factory<M: Model>() -> Factory<M> {
    Factory::new(Attributes::new)
        .set("name", "Flight 1")
        .attribute("display_name", |attributes| {
            Ok(attributes["name"].clone())
        })
}
```

用 `modelAttribute("owner_id", &owner)` 将现有模型的主键用作工厂属性。没有主键的模型产生 null，显式属性优先。创建时，工厂在保存模型之后、创建子关系之前清除已缓存的空关系集合。非空集合和单值关系继续保留，包括已缓存的 null。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn from_owner<M: Model, R: Model>(owner: &R) -> Factory<M> {
    Factory::new(Attributes::new).modelAttribute("owner_id", owner)
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

`hasAttachedNamed` 成功写入后，如果该命名关系已加载，或者中间表变更会触碰关联模型，工厂会重新加载该关系。`afterCreating` 可以读取更新后的集合。配置自定义 Pivot 时，集合中的关联模型带有对应的 Pivot 模型。

```rust
use openrust::database::eloquent::{
    Attributes, Model,
    factories::Factory,
    relations::BelongsToMany,
};

fn attach_named<M: Model, R: Model>(related: R) -> Factory<M> {
    Factory::new(Attributes::new)
        .hasAttachedNamed::<R, BelongsToMany<M, R>>(
            "members", vec![related], Attributes::new(),
        )
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

向 `hasAttached` 或 `hasAttachedNamed` 传入属性映射列表，即可按关联模型顺序设置中间表属性。关联工厂数量超过列表长度时会循环。用 `FactoryPivot::resolve` 根据已保存的父模型计算每次附加的属性。单个属性映射会应用于全部关联。 中间表序列也会在同一构建器的不同父模型和重复调用之间接续。

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
