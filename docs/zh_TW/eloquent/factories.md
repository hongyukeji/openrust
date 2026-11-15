# Eloquent 工廠

## 定義模型工廠

用 `Factory::new` 定義模型的預設屬性。若希望透過 `Flight::factory()` 呼叫，在模型上實作 `HasFactory`。執行下方指令在 `database/factories` 產生檔案，再宣告 Rust 模組並填寫屬性定義。

應用程式入口與資料庫連線選擇請見 [在 Seeder 中使用模型工廠](../database/seeding.md#using-model-factories)。

```sh
./artisan make:factory FlightFactory --model=Flight
```

### 模型與工廠繫結 {#factory-model-binding}

產生的工廠實作 `FactoryDefinition`，透過關聯型別 `Model` 在編譯期繫結模型。在 `database/factories/mod.rs` 宣告產生的模組，並在 `database/mod.rs` 宣告 `factories`。在模型上實作 `HasFactory`，將 `Flight::factory()` 委派給選定的工廠。也可直接使用 `FlightFactory::factory()`。每次建立工廠時先執行 `configure`，呼叫端追加的狀態可以覆寫其預設值。

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

用 `FlightFactory::new(attributes)` 或 `FlightFactory::newWithState(callback)` 在 `configure` 前登錄初始狀態。`FlightFactory::times(2)` 建立設定完成的兩筆批次。單筆與批次建構器的 `modelName()` 回傳繫結的 Rust 型別名稱。`newModel(attributes)` 依模型的批量賦值規則建構尚未儲存的模型，無須執行工廠定義與狀態。
用 `Factory::<Flight>::factoryForModel()` 取得模型透過 `HasFactory` 繫結的工廠。繫結以編譯期模型型別為準。

產生 `Fleet/FlightFactory` 等巢狀工廠時，Artisan 會推斷對應的 `app::models::fleet` 模型。可用 `--model` 或 `-m` 選擇其他模型。編譯前宣告產生的模組，模型和工廠仍透過 `FactoryDefinition::Model` 與 `HasFactory` 明確繫結。

### 工廠擴充 {#factory-extensions}

用 Rust 擴充 trait 為工廠提供可重複使用的方法。呼叫端匯入 trait 後，可以用方法為建構器套用具名狀態。

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

### 本地化測試資料 {#localized-fake-data}

下文列出已支援的產生器。

下文涉及的上游格式以[上游 Faker 資料集與產生器](https://github.com/FakerPHP/Faker/tree/v1.24.1)為準。

使用 `Factory::withFaker` 定義產生的模型屬性。它讀取所選連線所屬應用程式的 `app.faker_locale`。使用 `fake(Some("fr_FR"))` 明確指定語言，或用 `fake(None)` 讀取目前應用程式的設定。Rust 產生器使用 fake-rs 的資料，提供 `name`、`safeEmail`、`city` 和 `sentence`。尚未支援的語言使用英文資料。

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


在應用程式內，`fake()` 回傳 `Arc<Faker>`，相同規範化語言的呼叫共用產生器。`unique(false)` 的歷史和時區覆寫會跨呼叫保留，`unique(true)` 會清空共用歷史。工廠使用由選定連線所屬應用程式持有的獨立產生器，同一應用程式內的批次與後續工廠呼叫共用其歷史。產生器在建構批次時耗盡，會在寫入該批模型前回傳錯誤。應用程式外的 `fake()` 每次建立新產生器。可用 `Faker::new` 單獨建立產生器。

內建產生方法還包括 `firstName`、`lastName`、`freeEmail`、`userName`、`streetName`、`postcode`、`phoneNumber`、`company` 和 `word`。這些方法也可透過 `unique` 呼叫。樣本取自 fake-rs 中對應語言的資料集。

用 `numberBetween(first, second)` 產生整數，包含兩個端點，端點可以任意排序。`randomDigit()` 回傳 0 至 9，`randomDigitNotNull()` 回傳 1 至 9。`dateTimeBetween(start, end)` 接收明確的 UTC 日期時間，以整秒取樣並包含兩端，起始秒晚於結束秒時回傳錯誤。四種方法均支援 `unique`。

`randomDigitNot(except)` 排除指定數字。`randomNumber(digits, strict)` 的位數傳入 `None` 時隨機選擇一至九位。嚴格模式產生確切位數，非嚴格模式也接受零位並回傳零。明確位數超過九位或嚴格模式指定零位時回傳錯誤。兩種方法皆支援 `unique`。

`randomFloat(decimals, min, max)` 先在區間取樣，再依指定小數位捨入，中點遠離零。傳入 `None` 可隨機選擇零至九位精度或產生上界。上下界可逆序，捨入結果可能超出原區間。非有限邊界回傳錯誤。與 PHP 8.5 相同，正精度超過 308 或捨入後的數值溢位時保留取樣值。精度小於 −308 時回傳零。`unique` 將正零與負零視為同一個值。

用 `unixTime(max)` 在 Unix 紀元與指定端點之間取樣整秒，或用 `dateTime(max)` 回傳 UTC 日期時間。傳入 `None` 時使用目前時間。`date(format, max)` 和 `time(format, max)` 接收 Chrono 格式，預設分別為 `%Y-%m-%d` 和 `%H:%M:%S`，格式無效時回傳錯誤。`iso8601(max)` 包含實際使用的時區偏移量。五種方法皆支援 `unique`。

從 Laravel 重用 PHP DateTime 格式時，呼叫 `dateWithPhpFormat(format, max)` 或 `timeWithPhpFormat(format, max)`。兩者皆以實際使用的時區轉為文字。`Y-m-d` 使用 PHP 格式符號，`%Y-%m-%d` 使用 Chrono 格式符號。兩個 PHP 入口分別與對應的 Chrono 方法共用 `unique` 歷史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn php_date_sample() -> DatabaseResult<String> {
    fake(None)?.dateWithPhpFormat(r"Y-m-d\TH:i:sP", Some(chrono::DateTime::UNIX_EPOCH))
}
```

用 `dateTimeInInterval(date, interval)` 在 UTC 起點和該時間加上帶正負號的 Chrono `TimeDelta` 之間按整秒取樣。負時長向過去取樣。起點和時長傳入 `None` 時，分別預設為 30 年前和往後五天。`dateTimeThisCentury(max)`、`dateTimeThisDecade(max)` 和 `dateTimeThisMonth(max)` 分別從目前時間往前 100 年、10 年和一個月開始。`dateTimeThisYear(max)` 從應用程式時區當年 1 月 1 日零點開始。其他日曆回溯和 `dateTimeInInterval` 的預設起點也採用應用程式時區。兩個區間參數都省略時，預設五天依本地日曆計算。明確傳入的 `TimeDelta` 表示實際經過的時長。這四個方法在 `max` 為 `None` 時以目前時間為終點，拒絕早於起點的終點，日曆溢出依上游 Faker 規則正規化。五個方法皆支援 `unique`。Rust 使用具型別的日期和時長。需要自訂日曆區間時，可計算終點後呼叫 `dateTimeBetween`。

用 `amPm(max)`、`dayOfMonth(max)`、`dayOfWeek(max)`、`month(max)`、`monthName(max)` 和 `year(max)` 依實際使用的時區格式化隨機日期的各個部分。數字月份補零，數字日期除 `cs-CZ` 外也補零。捷克語、土耳其語、簡體中文、繁體中文和喬治亞語使用對應的星期和月份名稱。土耳其語和兩種中文還會在地化 `amPm`。`dateTimeAD(max)` 從上游 Faker 的 64 位元公元日期下界開始取樣。這些方法在 `max` 為 `None` 時使用目前時間。`century()` 隨機回傳 I 至 XXI 的羅馬數字。所有方法皆支援 `unique`。

在 `cs-CZ` 中，`monthNameGenitive(max)` 回傳月份屬格，`formattedDate()` 產生 `16. listopadu 2003` 這類顯示文字。日、月、年獨立取樣，結果可能超出有效日期範圍。兩種方法皆支援 `unique`，在其他地區呼叫時回傳錯誤。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_date_sample() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).formattedDate()
}
```

用 `timezone(countryCode)` 隨機取得 IANA 時區識別碼。傳入 `None` 從完整清單取樣，也可傳入 `Some("TW")` 等國家或地區代碼來限定範圍。代碼區分大小寫，未知代碼回傳 `None`。框架內建與 `chrono-tz` 同版的 IANA 資料。此方法支援 `unique`。回傳的識別碼可用於將 UTC 日期轉換到目標時區。

日期字串優先使用 `setDefaultTimezone(Some(zone))` 為目前產生器指定的時區，其次讀取應用程式或工廠連線所屬應用程式的 `app.timezone`，預設為 UTC。傳入 `None` 可清除覆寫，`getDefaultTimezone()` 可查看覆寫值。`dateTimeInTimezone(max, timezone)` 回傳帶時區的日期時間，傳入 `Some(zone)` 可覆寫本次呼叫。日期時間方法仍回傳 UTC 時刻。產生器的時區覆寫用於輸出格式化，日曆邊界繼續依應用程式時區計算。

需要讓結果攜帶輸出時區時，使用 `dateTimeAD`、`dateTimeBetween`、`dateTimeInInterval` 和四個 `dateTimeThis…` 方法的 `InTimezone` 變體。兩個日期運算式區間方法也有帶時區的版本。明確指定的時區只作用於回傳值。相對運算式沿用應用程式時區，也可在運算式內指定時區。原方法回傳 UTC。`unique` 依對應方法共用歷史，並區分名稱不同但 UTC 偏移相同的時區。夏令時間回撥時，同一時區重複出現的當地時間視為重複值。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn zoned_date_sample() -> DatabaseResult<String> {
    let epoch = chrono::DateTime::UNIX_EPOCH;
    let tokyo = "Asia/Tokyo".parse().expect("known IANA time zone");
    Ok(fake(None)?
        .dateTimeBetweenInTimezone(epoch, epoch, Some(tokyo))?
        .to_rfc3339())
}
```

用 `dateTimeBetweenExpressions(start, end)` 傳入 PHP 風格的日期表達式，例如 `"-30 years"` 和 `"now"`。兩個端點共用同一參考時刻，並依應用程式時區解析。表達式中的明確時區優先。數字終點字串依 Unix 時間戳解析，`"@-1"` 則是明確的時間戳表達式。端點解析失敗時依上游 Faker 的數值轉換規則使用 Unix 零秒。起點晚於終點仍回傳錯誤。此方法與 `dateTimeBetween` 共用 `unique` 歷史。

日期運算式中的命名時區遵循 PHP 拼寫規則。例如，`America/New_York` 和 `Us/Eastern` 可以解析，首段全大寫的 `US/Eastern` 解析失敗。要使用固定偏移，直接寫 `-05:00` 這類數值偏移。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn date_expression_sample() -> DatabaseResult<i64> {
    let date = fake(None)?.resolveDateMax("2024-01-01 12:34:56 America/New_York")?;
    Ok(date.timestamp())
}
```

用 `dateTimeInIntervalExpressions(date, interval)` 傳入 PHP 風格的相對區間，例如 `Some("+1 month")` 或 `Some("5 days ago")`。終點依起始日期所在時區的本地時間計算，跨越日光節約時間時也遵循此規則。取樣結果回傳 UTC。省略日期時從 30 年前開始，省略區間時使用五個日曆日。區間字串接受相對表達式，拒絕絕對日期、時間和時區識別碼。此方法與 `dateTimeInInterval` 共用 `unique` 歷史。

用 `resolveDateMax(expression)` 解析 PHP 風格的日期上界，再以 `Some(max)` 傳給任何接受 `max` 的日期產生方法。解析一次即可在多個呼叫中共用同一上界。小數和科學記號的數字字串向零截斷為 Unix 秒。空字串使用目前時間。日期文字預設依應用程式時區解讀，文字中的明確時區優先。無效或超出範圍的值回傳錯誤。

用 `unixTimeExpression(max)` 從字串上界直接取樣 Unix 秒，支援 Chrono 日期範圍之外的時間戳和延伸年份。它與 `unixTime` 共用 `unique` 歷史。語法無效的上界使用 Unix 零秒。需要在取樣前驗證輸入時，用 `resolveDateMax`。超過解析限制或數值範圍時仍回傳錯誤。有限數字字串超出有號 64 位元範圍時飽和至端點，溢位為無限大時依 PHP 整數轉換規則取零。回傳日期的介面仍要求上界可由 Chrono 表示。

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

### 文字產生 {#text-generation}

用 `words(count)`、`sentences(count)` 和 `paragraphs(count)` 產生指定數量的清單。對應的 `AsText` 方法用空格連接單字或句子，用空行分隔段落。`sentenceWithWordCount(count, variable)` 和 `paragraph(count, variable)` 支援固定數量或依上游 Faker 規則浮動。原有依範圍取樣的 `sentence(words)` 也可以繼續使用。`text(max)` 以 UTF-8 位元組為上限。上限不足五個位元組或無法容納產生項目時回傳錯誤。這些方法皆支援 `unique`。詞庫由 fake-rs 依語言提供。

### 預留字元格式化 {#pattern-formatters}

用 `numerify(Some(pattern))` 將 `#` 替換為數字、將 `%` 替換為非零數字。`lexify` 將 `?` 替換為 ASCII 小寫字母。`bothify` 處理這三種預留字元，並以相同機率將每個 `*` 替換為數字或小寫字母。`asciify` 將 `*` 替換為 `!` 至 `~` 的可列印 ASCII 字元。其餘字元照原樣保留。傳入 `None` 使用上游 Faker 預設範本。`randomLetter()` 和 `randomAscii()` 回傳單一 Rust 字元。六個方法皆支援 `unique`。

用 `regexify(pattern)?` 從基本正規表示式產生樣本。支援可選的斜線分隔符號和外層錨點、字面值、ASCII 字元類別和範圍、`\d`、`\w`、點號、一般群組、分支和有限重複。與上游 Faker 相同，`*` 和 `+` 分別為目前範本選取一至九的重複上限。語法錯誤、環視和反向參照等未支援功能，以及超出產生限制的模式均回傳錯誤。`unique(false).regexify(pattern)?` 可對結果去重。

### 集合取樣 {#collection-sampling}

用 `randomElement(values)` 借用一個元素，空切片回傳 `None`。`randomKey(values)` 回傳從零開始的位置。若要從映射鍵或列舉變體中選取，將鍵或變體的切片傳給 `randomElement`。`randomElements(values, count, allowDuplicates)?` 接受 `Some(n)` 固定抽取數量，或用 `None` 在一到切片長度之間隨機選取數量。不放回時每個位置最多抽一次，不同位置上的相同值仍可同時出現。零筆回傳空向量。非零取樣要求切片非空，不放回取樣的數量不能超過切片長度。

用 `shuffleArray(values)` 回傳打亂後的副本，或用 `shuffleString(value)` 依 Unicode 純量值打亂字串。`boolean(None)` 有 50% 的機率回傳 true，傳入 `Some(75)` 可設為 75%。這些方法均支援 `unique`，泛型取樣與陣列打亂依 JSON 序列化結果判斷重複。

### 語言、國家和貨幣樣本 {#locale-and-code-samples}

用 `languageCode()`、`countryCode()`、`countryISOAlpha3()`、`currencyCode()` 和 `emoji()` 從固定上游 Faker 目錄產生測試值。`randomLocale()` 抽取 `en_GB`、`zh_TW` 等語言地區識別碼，對應上游 Faker 的 `locale()` 產生器。OpenRust 既有的 `locale()` 讀取目前產生器的設定。樣本目錄包含歷史識別碼。六個產生器均支援 `unique`。

### 摘要樣本 {#digest-samples}

用 `md5()`、`sha1()` 和 `sha256()` 產生 32、40 和 64 個字元的小寫十六進位樣本。每個產生器遵循上游 Faker 的做法，從 0 到 2,147,483,647 抽取整數，再對其十進位文字計算摘要。三個方法均支援 `unique`，供測試和資料填充使用。

### 網路位址樣本 {#network-address-samples}

用 `ipv4()`、`localIpv4()`、`ipv6()` 和 `macAddress()` 產生網路測試資料。`ipv4()` 在 `1.0.0.0`–`127.255.255.255` 和 `128.0.0.0`–`255.255.255.254` 兩段中以相同機率選一段，再抽取位址，可能回傳私有或特殊用途位址。`localIpv4()` 在 `10/8`、`172.16/12` 和 `192.168/16` 中以相同機率選一段。IPv6 回傳八組未壓縮的小寫十六進位數，各組省略前導零。MAC 位址由六組以冒號分隔的兩位大寫十六進位數組成。四個方法均支援 `unique`。

### 網域名稱、URL 和 slug {#domain-url-slug-samples}

用 `domainWord()?` 將產生的姓氏轉寫為小寫 ASCII，用 `tld()` 依支援語言地區的上游 Faker 權重抽取後綴，`domainName()?` 將兩者連接。`url()?` 使用上游 HTTP 和 HTTPS 範本，可帶 `www`、slug 和 `.html`。姓氏使用 OpenRust 的語言資料和轉寫規則，與上游 Faker 的 ICU 或備援映射可能不同。姓氏無法轉寫出可用字元時，網域名稱和 URL 產生器回傳錯誤。`slug(count, variable)` 用連字號拼接產生的詞。傳入 `None` 使用六個基準詞、`Some(0)` 回傳空字串，`false` 固定詞數。五個產生器均支援 `unique`。

### 電子郵件和密碼樣本 {#email-and-password-samples}

用 `email()?` 產生一般電子郵件地址，或用 `companyEmail()?` 產生使用隨機網域的地址。簡體中文的一般電子郵件使用該地區的免費信箱清單。`safeEmail()` 使用保留的範例網域，`freeEmail()` 使用地區信箱服務網域。只需網域時用 `safeEmailDomain()` 或 `freeEmailDomain()`。使用者名稱使用地區姓名或詞庫並轉寫為 ASCII。若無可用字元，OpenRust 產生一個 ASCII 字母，保留既有使用者名稱及電子郵件方法的直接回傳型別。`password(min, max)?` 使用從 `!` 到 `~` 的可列印 ASCII 字元，長度含兩端，接受反向邊界，預設值為 6 和 20。以上產生器均支援 `unique`。

### 地區 URL 範本 {#regional-url-templates}

`ne-NP` 的 `url()` 使用固定 HTTP 範本，依範本加上 `www`、`.np` 或路徑。`uk-UA` 的 `companyUrl()` 回傳原始西里爾字母公司標籤，可附加連字號與公司後綴。標籤沒有協定和網域後綴。兩個產生器均支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_url_sample() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("ne-NP"))?.url()?, fake(Some("uk-UA"))?.companyUrl()?))
}
```

### 地區網域與信箱 {#regional-domains-and-email}

`tld()` 和 `freeEmailDomain()` 從固定 Internet 詞表取樣，重複項目保留原權重。沙烏地阿拉伯、約旦、埃及、伊朗、韓國和日本的 `domainName()` 使用羅馬字姓氏詞表。其他地區組合 `domainWord()` 和網域後綴。尼泊爾的一般信箱範本包含兩個公司網域、一個免費網域、三個附加 `.np` 的公司網域。直接呼叫 `companyEmail()` 使用一般網域。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_email_sample() -> DatabaseResult<String> {
    fake(Some("ne-NP"))?.unique(false).email()
}
```

### 地區使用者名稱 {#regional-usernames}

`userName()` 在沙烏地阿拉伯、約旦、埃及、伊朗、韓國和日本使用固定 FakerPHP 羅馬字姓名詞表。立陶宛範本從相同性別群組取名字與姓氏。簡體中文使用固定 Lorem 詞表和含底線的範本。`safeEmail()` 和 `freeEmail()` 共用此產生器作為信箱本地部分。通用路徑使用 Rust 音譯，音譯後只剩分隔符時產生一個字母，保留使用者名稱與信箱的無錯誤簽名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn username_sample() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).userName()
}
```

上述六個地區的 Internet Provider 提供 `firstNameAscii()` 和 `lastNameAscii()`，可直接從使用者名稱和網域名稱使用的詞表抽取，保留詞表原有拼法。這兩個方法支援 `unique()`，其他地區呼叫時回傳錯誤。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ascii_name_sample() -> DatabaseResult<(&'static str, &'static str)> {
    let faker = fake(Some("ar-EG"))?;
    Ok((faker.unique(false).firstNameAscii()?, faker.lastNameAscii()?))
}
```

### 電話號碼和 IMEI 樣本 {#phone-and-imei-samples}

用 `phoneNumber()` 產生固定 FakerPHP 地區 Provider 的顯示號碼。地區沒有 PhoneNumber Provider 時使用美國顯示範本。孟加拉使用孟加拉數字，越南依所選區碼決定用戶號碼長度。巴西也提供僅含數字的 `phoneNumberCleared()`，支援 `unique()`。 `e164PhoneNumber()` 使用固定的國際號碼範本及地區覆寫，回傳以加號開頭的數字字串。`imei()` 回傳 15 位數字，含 Luhn 檢查碼。三個方法均支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn display_phone_sample() -> DatabaseResult<(String, String)> {
    let bangladesh = fake(Some("bn-BD"))?;
    let brazil = fake(Some("pt-BR"))?;
    Ok((bangladesh.phoneNumber(), brazil.unique(false).phoneNumberCleared()?))
}
```

### 地區手機、市話與分機號碼樣本 {#regional-phone-samples}

支援對應介面的地區可用 `mobileNumber()` 和 `fixedLineNumber()`。馬來西亞也支援 `mobileNumberWithOptions(countryCodePrefix, formatting)` 和 `fixedLineNumberWithOptions(countryCodePrefix, formatting)`，預設含國家前綴和分隔符號。巴西的 `cellphoneNumber(formatted)` 和 `landlineNumber(formatted)` 預設含格式。香港提供 `landlineNumber(None)`，韓國使用 `cellPhoneNumber()`，希臘保留 `mobilePhoneNumber()`。

用 `phoneNumberWithExtension()` 產生美國或加拿大的分機號碼。地區沒有 PhoneNumber Provider 時回退至 `en-US`，沿用美國的分機與 E.164 規則。地區無對應介面時回傳錯誤。各介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_phone_sample() -> DatabaseResult<(String, String)> {
    let malaysia = fake(Some("ms-MY"))?;
    let canada = fake(Some("en-CA"))?;
    Ok((
        malaysia.mobileNumberWithOptions(false, false)?,
        canada.unique(false).phoneNumberWithExtension()?,
    ))
}
```

### 免付費電話、服務號碼與 VoIP 樣本 {#phone-service-samples}

美國、加拿大、德國和羅馬尼亞使用 `tollFreePhoneNumber()`。沒有 PhoneNumber Provider 的地區繼承美國格式。希臘、紐西蘭、南非和西班牙使用 `tollFreeNumber()`。新加坡提供 `tollFreeLineNumber()`、`tollFreeInternationalNumber()` 和 `premiumPhoneNumber()`。法國提供 `serviceNumber()`，羅馬尼亞提供 `premiumRatePhoneNumber()`，希臘提供 `personalNumber()`、`sharedCostNumber()` 和 `premiumRateNumber()`。

新加坡和馬來西亞提供 `voipNumber()`。馬來西亞也支援 `voipNumberWithOptions(countryCodePrefix, formatting)`，預設包含國家前綴和分隔符號。各方法皆支援 `unique()`，目前地區的 Provider 缺少對應介面時回傳錯誤。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn phone_service_sample() -> DatabaseResult<(String, String)> {
    Ok((
        fake(Some("en-SG"))?.unique(false).tollFreeLineNumber()?,
        fake(Some("ms-MY"))?.voipNumberWithOptions(false, false)?,
    ))
}
```

### 地區號碼組成 {#regional-phone-components}

使用 `areaCode()`、`exchangeCode()` 和 `tollFreeAreaCode()` 產生支援的號碼組成。回傳字串以保留前導零。芬蘭提供本地區碼和國際區碼變體，`numberFormat()` 回傳含預留位置的範本。巴西的 `cellphone(formatted)`、`landline(formatted)` 和 `phone(formatted)` 產生不含區碼的用戶號碼。這些介面也支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn phone_component_sample() -> DatabaseResult<(String, String)> {
    Ok((
        fake(Some("en-CA"))?.unique(false).tollFreeAreaCode()?,
        fake(Some("pt-BR"))?.cellphone(Some(false))?,
    ))
}
```

### 條碼與 ISBN {#barcode-samples}

使用 `ean8()`、`ean13()`、`isbn10()` 和 `isbn13()` 產生帶有檢查碼的樣本。回傳字串保留前導零。ISBN-10 的末位可以是 `X`，ISBN-13 以 `978` 或 `979` 開頭。各產生器獨立維護 `unique` 歷史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn catalog_codes() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en"))?;
    Ok((faker.ean13(), faker.unique(false).isbn13()?))
}
```

### 檔案範例 {#file-samples}

`mimeType()` 從固定 FakerPHP 目錄選擇 MIME 類型。`fileExtension()` 先選 MIME 群組，再回傳不含點的副檔名。兩個方法與版本 3 的 `uuid()` 都支援 `unique`。`file(sourceDirectory, targetDirectory, fullPath)` 從既有目錄的頂層選取可讀檔案，再複製到另一個既有目錄，產生的檔名保留來源副檔名。Rust 使用明確路徑，失敗時回傳 I/O 錯誤。

```rust
use std::{io, path::{Path, PathBuf}};
use openrust::database::eloquent::factories::Faker;

fn copy_sample(faker: &Faker, source: &Path, target: &Path) -> io::Result<PathBuf> {
    faker.file(source, target, false)
}
```

### 預留圖片 {#placeholder-images}

用 `imageUrl(&ImageOptions)` 產生 FakerPHP 預留圖片 URL。預設是 640 × 480 的 PNG，隨機產生文字和背景，也支援 JPG 和 JPEG。`image(directory, fullPath, options).await` 下載圖片，省略目錄時使用系統暫存目錄。HTTP 請求失敗或回應中斷時回傳空路徑，無效選項與本機寫入失敗會回傳 I/O 錯誤。

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

### 地區身分編號樣本 {#regional-identity-samples}

使用 `en-US` 的 `ssn()`、`en-GB` 的 `nino()`，以及 `zh-TW` 或 `zh-Hant` 的 `personalIdentityNumber(gender)` 產生測試資料。美國與英國編號沿用固定 FakerPHP 的範圍和前綴規則。臺灣編號會計算檢查碼，性別可傳入 `Some("male")`、`Some("female")` 或 `None` 隨機產生。三個方法都支援 `unique`，遇到未支援的語言會回傳錯誤。

`ssn()` 也支援 `th-TH`、`de-AT` 與 `is-IS`。泰語使用與美式英語相同的 FakerPHP 格式。奧地利和冰島格式從最近 100 年內取樣日期並計算檢查碼。使用 `de-AT` 時，可呼叫 `ssnWithBirthdate(Some(date))` 傳入 `chrono::NaiveDate`，或用 `None` 取樣日期。兩個 SSN 方法共用 `unique` 歷史。

`personalIdentityNumber(gender)` 也支援 `lt-LT`、`lv-LV`、`fi-FI`、`nb-NO`、`sv-SE` 和 `pl-PL`。前五種可透過 `personalIdentityNumberWithBirthdate(birthdate, gender)` 指定出生日期。立陶宛可用 `personalIdentityNumberWithSerial(birthdate, gender, serial)` 指定序號前綴。波蘭採用身分證格式。固定版挪威 provider 的末兩位隨機產生。

使用 `pl-PL` 時，`pesel(birthdate, sex)` 產生 11 位 PESEL。可傳入 1800 至 2299 年的 `chrono::NaiveDate`，或傳入 `None` 取樣出生日期。`Some("F")` 使用偶數性別位，`Some("M")` 和 `None` 沿用 FakerPHP 的奇數性別位。`taxpayerIdentificationNumber()` 產生 10 位 NIP。兩個方法都計算檢查碼並支援 `unique`。

使用 `pt-BR` 時，可使用 `cpf(formatted)`、`rg(formatted)` 和 `cnpj(formatted)`。傳入 `None` 使用預設標點，傳入 `Some(false)` 只傳回數字。RG 沿用固定的聖保羅 provider，檢查碼為數字。CNPJ 使用分支號 `0001`。三個方法都支援 `unique`。

`dni()` 在 `es-ES` 產生八位數字和模 23 檢查字母，在 `es-PE` 產生八位數字。秘魯的 `ruc(isNaturalPerson)` 預設使用 `20` 前綴，傳入 `Some(true)` 時使用 `10`，固定版產生器接著附加九位隨機數字。`licenceCode()` 依 `es-ES` 或 `en-ZA` 取樣駕照類別。以上方法都支援 `unique`。

`idNumber()` 支援 `nl-NL`、`en-ZA`、`ar-SA` 和 `mn-MN`。南非可用 `idNumberWithBirthdate(birthdate, citizen, gender)` 指定出生日期、公民身分及性別。沙烏地阿拉伯的 `nationalIdNumber()` 固定使用前綴 `1`，`foreignerIdNumber()` 固定使用前綴 `2`。埃及可用 `nationalIdNumberWithGender(gender)` 控制登記序號奇偶，或用 `nationalIdNumber()` 採用預設規則。埃及產生器的末位隨機產生。蒙古語的 `alphabet()` 傳回一個西里爾字母，埃及的 `governorateId()` 傳回兩位省份代碼。以上方法都支援 `unique`。

`passportNumber()` 在 `lt-LT` 傳回八位數字，在 `lv-LV` 傳回兩個小寫字母和七位數字。`tcNo()` 沿用固定土耳其產生器，允許前導零，並保留可能在後綴產生負號的上游帶符號餘數。`Faker::tcNoChecksum(prefix)` 接收九位 ASCII 數字，`Faker::tcNoIsValid(number)` 比對兩字元後綴，前綴無效時回傳錯誤。兩個產生方法都支援 `unique`。

`en-US` 的 `ein()` 傳回兩位前綴、連字號和七位後綴。`ru-RU` 的 `inn10(area)` 和舊名 `inn(area)` 產生十位 INN。傳入 `None`、空字串或零時，從 01 至 91 抽樣地區編號。明確指定的地區編號須用 ASCII 數字表示 1 至 99。`kpp(inn)` 取 INN 前四位並附加 `01001`，省略或不足四個位元組時產生新 INN。`Faker::inn10Checksum` 根據九位前綴計算檢查碼，`Faker::inn10IsValid` 驗證完整號碼。四個產生方法都支援 `unique`。

`fr-FR` 的 `siret(None)` 依 3–3–3–5 分組產生法國機構編號，傳入 `Some(false)` 可取得無空格形式。九位 SIREN 前綴和十四位完整編號分別通過 Luhn 檢查。`pl-PL` 的 `regon()` 產生九位企業登記號，`regonLocal()` 以有效的 REGON 為前綴產生十四位本地登記號。兩者採用加權模 11 檢查，餘數為 10 時檢查碼為零。三個產生方法都支援 `unique`。

`cs-CZ` 的 `ico()` 產生帶有檢查碼的八位捷克企業編號。`da-DK` 的 `cvr()` 傳回八位數字，`p()` 傳回十位數字。`is-IS` 的 `vsk()` 傳回五位數字。丹麥和冰島的格式沿用上游樣本規則，首位為非零數字。四個方法都支援 `unique`。

`ar-SA` 的 `companyIdNumber()` 產生以 `700` 開頭的十位沙烏地阿拉伯企業編號。`ar-EG` 的 `companyTaxIdNumber()` 傳回十位企業稅號，`companyTradeRegisterNumber()` 傳回七位商業登記號。埃及兩種編號允許前導零。三種格式都附加 Luhn 檢查碼並支援 `unique`。

`kk-KZ` 的 `businessIdentificationNumber(date)` 傳回十二位 BIN。可以傳入登記日期指定年月，或傳入 `None` 從本年年初至目前時間取樣。`en-ZA` 的 `companyNumber()` 從過去五十年至目前時間取樣登記年份，傳回 `YYYY/NNNNNN/TT`。兩者都支援 `unique`。

`zh-Hant` 的 `VAT()` 傳回八位臺灣稅號樣本，型別為整數。現有 `vat(...)` 在此地區傳回相同格式的數字字串，不加國家前綴。`el-GR` 的 `doy()` 從 21 個稅務機關抽樣，`object()` 從 19 種營業項目抽樣。每個方法都支援 `unique`。

`en-US`、`fr-FR`、`it-IT` 和 `pl-PL` 的 `company()` 採用 FakerPHP 公司名稱範本，`companySuffix()` 從對應的後綴清單抽樣。波蘭公司名稱也使用固定版上游的波蘭姓氏與機構前綴，另外三個地區使用固定 Person 姓氏詞表。沒有固定公司範本的地區沿用 fake-rs 的公司資料與回退規則。兩個方法都支援 `unique`。

`zh-Hans`、`zh-Hant`、`ja-JP` 和 `ko-KR` 的 `company()` 使用對應的 FakerPHP 範本。這四個地區及 `pl-PL` 可用 `companyPrefix()` 抽樣機構前綴。用 `zh-Hant` 的 `companyModifier()` 或 `companyEnSuffix()` 抽樣名稱修飾詞或英文後綴。這三個方法對不支援的地區傳回錯誤，並支援 `unique`。日本公司名稱使用固定日本姓氏詞表，`companySuffix()` 傳回繼承的 `Ltd`，公司名稱範本使用前綴。

`ar-EG`、`ar-JO` 和 `ar-SA` 的公司名稱使用固定上游的阿拉伯語前綴、地區姓氏與後綴。埃及範本也會使用城市名稱。產生器保留後綴詞表中的空格。三個地區的 `companyPrefix()` 和 `companySuffix()` 都支援 `unique`。

`de-DE`、`de-AT`、`de-CH` 和 `cs-CZ` 的 `company()` 使用固定上游的地區姓氏、範本與後綴。捷克名稱一律帶後綴，可以用空格、連字號或 `a` 連接兩個姓氏。三個德語地區也會產生僅含姓氏的公司名稱。`companySuffix()` 支援 `unique`，保留後綴拼寫與原有抽樣權重。

`da-DK`、`is-IS`、`fi-FI`、`sv-SE` 和 `nb-NO` 的 `company()` 使用固定上游的地區姓名與後綴詞表，保留範本中的連接詞、標點與抽樣權重。可用 `companySuffix()` 單獨抽樣後綴。兩個方法都支援 `unique`。

`es-AR`、`es-ES`、`es-PE`、`es-VE`、`pt-BR` 和 `pt-PT` 的 `company()` 使用固定上游的地區姓氏和公司範本。`companySuffix()` 使用對應的後綴詞表，例如巴西的 `Ltda.` 和葡萄牙的 `Lda.`。`companyPrefix()` 新增西班牙和委內瑞拉。三個方法均支援 `unique`。

`nl-NL` 的 `company()` 組合商品與店鋪類型，或在行業名稱後接姓氏，並可附加荷蘭公司後綴。`nl-BE` 使用比利時姓氏，可附加當地後綴。荷蘭姓氏保留原有大小寫，包括 `van` 等詞。兩個地區都支援 `companySuffix()` 和 `unique`。

`hr-HR`、`sk-SK` 和 `hu-HU` 的 `company()` 使用固定上游的地區姓名與範本。克羅埃西亞名稱可使用行業前綴或後綴，`companyPrefix()` 可單獨抽樣行業前綴。斯洛伐克範本保留連字號和原有的 `and`。匈牙利名稱可以僅由姓氏組成。三地都支援 `companySuffix()` 和 `unique`。

`el-GR` 的 `company()` 將希臘姓氏與法定後綴組合。`me-ME` 從固定蒙特內哥羅詞表組合企業名稱和法定類別，可用 `companyName()` 與 `companyType()` 分別抽樣。當地 `companySuffix()` 傳回繼承的 `Ltd`。四個方法都支援 `unique`。

`en-GB` 和 `en-ZA` 的 `company()` 組合地區姓氏與繼承的 `Ltd` 後綴。`el-CY` 使用姓氏加賽普勒斯法定後綴，或用連字號連接兩個姓氏。三地都支援 `companySuffix()` 和 `unique`。

`fr-BE`、`fr-CA`、`fr-CH` 和 `it-CH` 的 `company()` 使用固定上游的地區姓氏與範本。加拿大法語繼承法國公司後綴，瑞士法語及義大利語分別使用自己的後綴，例如 `Sàrl` 和 `Sarl`。`company()` 和 `companySuffix()` 都支援 `unique`。

`lt-LT`、`sl-SI` 和 `tr-TR` 的 `company()` 使用固定上游的地區範本。立陶宛將法定類別放在名稱前，斯洛維尼亞獨資經營名稱以 `s.p.` 結尾，土耳其範本可包含營業領域。`tr-TR` 的 `companyField()` 可單獨抽樣營業領域，並支援 `unique`。

`ru-RU` 和 `kk-KZ` 的公司名稱組合了法定前綴、名稱詞素與可選的名稱尾部。可用 `companyNameElement()` 與 `companyNameSuffix()` 單獨抽樣，`companySuffix()` 仍傳回繼承的 `Ltd`。`uk-UA` 的 `companyName()` 抽樣基礎名稱，`company()` 可加入前綴、引號或連字號後綴。這些地區方法支援 `unique`。

`fa-IR`、`he-IL`、`hy-AM` 和 `ka-GE` 的 `company()` 使用固定上游的地區範本與詞表。伊朗公司名稱可包含前綴與營業領域，該地區的 `contract()` 抽樣勞動契約類型。喬治亞公司名稱由前綴、詞素與名稱尾部組成，也可分別抽樣。這些方法支援 `unique`。

`bn-BD` 的 `company()` 組合名稱與類別，也可透過 `companyName()` 和 `companyType()` 分別抽樣。印尼（`id-ID`）公司名稱由法定前綴和一到兩個姓氏組成，部分範本附帶後綴。馬來西亞（`ms-MY`）使用公司名稱詞表，或組合產業與地區姓名，並保留上游後綴權重。可單獨呼叫 `companyName()` 和 `industry()`。泰國（`th-TH`）使用當地姓氏和繼承的 `Ltd` 後綴，`slogan()` 組合三段泰語詞組。這些方法支援 `unique`。

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

### 台灣英文公司名稱 {#taiwanese-english-company-names}

使用 `zh-TW` 或 `zh-Hant` 時，`companyEn()` 沿用固定上游預設裝配缺少 `lastNameEn` 的錯誤。透過 `companyEnWithLastName` 傳入可回傳錯誤的 Rust 閉包，範本每需要一個姓氏就呼叫一次。兩個方法的 `unique` 版本共用 `companyEn` 歷史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwanese_company_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant"))?
        .unique(false)
        .companyEnWithLastName(|| Ok("Lin".into()))
}
```

### 職業名稱 {#job-titles}

呼叫 `jobTitle()` 從目前地區的 Company provider 產生職業名稱。`de-DE`、`en-US`、`fr-FR`、`id-ID`、`nb-NO`、`nl-NL`、`ru-RU`、`sv-SE`、`tr-TR` 和 `uk-UA` 有獨立詞表。比利時、加拿大與瑞士的法語地區繼承法國詞表。Company provider 繼承基礎職業範本時回傳單個 Lorem 詞，缺少 Company provider 時使用美國詞表。可透過 `unique(false).jobTitle()` 去重。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn job_title_sample() -> DatabaseResult<String> {
    fake(Some("id-ID"))?.unique(false).jobTitle()
}
```

### 偏置取樣與產生器修飾器 {#faker-biased-and-modifiers}

用 `biasedNumberBetween(min, max, function)?` 在包含端點的整數範圍內取樣。傳入 `None` 分別採用預設下限 0、上限 100 與平方根曲線。自訂曲線接收 [0, 1) 內的數並傳回 0 至 1 的機率。範圍反向、機率無效或取樣耗盡時回傳錯誤。此方法支援 `unique`。

用 `optional(weight, default, generate)?` 依機率呼叫具型別的產生閉包，或傳回選用的預設值。預設權重為 0.5，大於 1 時依 FakerPHP 的百分比規則解讀。用 `valid(predicate, maxRetries, generate)?` 重試直到符合述詞，預設重試上限為 10,000。兩者都會傳遞產生錯誤。用 `uniqueWithRetries(reset, maxRetries)` 設定共用去重歷史的控制代碼可嘗試的次數。

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

### 語料文字 {#corpus-text-samples}

用 `realText(maxNbChars, indexSize)?` 或 `realTextBetween(minNbChars, maxNbChars, indexSize)?` 從固定 FakerPHP 語料以加權 Markov 鏈產生文字。各參數可傳入 `None`，採用預設最大長度 200、最小長度 160 或上下文長度 2。上下文長度支援 1 至 5。 `realTextBetween` 的 Unicode 字元數大於最小值，且不超過最大值。簡體中文等缺少上游語料的語言會回退至英語。繁體中文與日語以字元取樣。波斯語 `realText` 沿用上游的位元組長度。該語料沒有以大寫開頭的詞，`realTextBetween` 會回傳重試耗盡錯誤。兩個方法都支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn corpus_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant"))?.realTextBetween(Some(80), Some(120), Some(2))
}
```

### HTML 樣本 {#html-samples}

用 `randomHtml(maxDepth, maxWidth)?` 產生 HTML 文件，其中包含標題、範例登入表單和隨機內文子樹。兩個參數各自傳入 `None` 時預設為四。深度小於或等於一時只產生標題和表單。寬度限制每次隨機產生的同層節點數。表單與連結使用保留的範例網域。數字 ID 在單份文件內唯一，`unique` 單獨記錄完整文件。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn html_sample() -> DatabaseResult<String> {
    let faker = fake(None)?;
    faker.randomHtml(Some(3), None)
}
```

### 血型樣本 {#blood-group-samples}

用 `bloodType()` 產生 A、AB、B 或 O，`bloodRh()` 產生 Rh 正負號。 `bloodGroup()` 獨立取樣兩部分並直接串接。各語言使用相同的取值。三個方法都支援 `unique`，並分別記錄去重歷史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn blood_samples() -> DatabaseResult<(&'static str, &'static str, String)> {
    let faker = fake(None)?;
    Ok((faker.bloodType(), faker.bloodRh(), faker.unique(false).bloodGroup()?))
}
```

### 色彩樣本 {#color-samples}

用 `hexColor()` 產生六位 RGB 十六進位色碼。 `safeHexColor()` 沿用固定 FakerPHP 的範圍 `#000000` 至 `#00ffff`，每一對十六進位數字相同。 `rgbColor()` 與 `hslColor()` 傳回逗號分隔值，對應的 `AsArray` 方法傳回三個整數。 `rgbCssColor()` 與 `rgbaCssColor()` 產生 CSS 字串。 `safeColorName()` 與 `colorName()` 依解析後的語言選用 FakerPHP 詞表，缺少對應詞表時使用預設詞表。十個方法都支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn color_samples() -> DatabaseResult<(String, [u8; 3], &'static str)> {
    let faker = fake(Some("zh-Hant"))?;
    let unique = faker.unique(false);
    Ok((faker.rgbaCssColor(), unique.rgbColorAsArray()?, unique.colorName()?))
}
```

### 瀏覽器 User-Agent {#browser-user-agent-samples}

用 `userAgent()?` 隨機產生瀏覽器樣本，也可用 `firefox()?`、`chrome()`、`internetExplorer()`、`opera()`、`safari()` 或 `msedge()` 指定格式。樣本沿用固定 FakerPHP 的瀏覽器版本與平台識別字串。Firefox 依應用程式時區取樣 Gecko 日期。用 `macProcessor()`、`linuxProcessor()`、`windowsPlatformToken()`、`macPlatformToken()`、`linuxPlatformToken()` 和 `iosMobileToken()` 單獨產生識別字串。以上格式化方法都支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn browser_samples() -> DatabaseResult<(String, String)> {
    let faker = fake(None)?;
    Ok((faker.userAgent()?, faker.unique(false).msedge()?))
}
```

### 銀行識別碼 {#bank-identifier-samples}

用 `iban(countryCode, prefix, length)` 產生帶模 97 檢查碼的 IBAN 範例。傳入 `None` 隨機選擇國家，或指定兩字母代碼。已收錄國家使用固定 FakerPHP 格式，未收錄代碼使用指定長度的數字帳號，預設 24 位。前綴取代帳號開頭並保留前導零。Rust 接受由 ASCII 大寫字母和數字組成的前綴，帳號部分最長 30 個字元。`swiftBicNumber()` 產生 8 或 11 個字元的範例，兩者都支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn bank_samples() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en"))?;
    let unique = faker.unique(false);
    Ok((unique.iban(Some("DE"), "0012", None)?, unique.swiftBicNumber()?))
}
```

### 地區銀行帳號 {#regional-bank-account-samples}

用 `bankAccountNumber(prefix, countryCode, length)` 依 Faker 地區產生帳號。支援 IBAN 別名的地區預設使用本國代碼，可明確覆寫參數。埃及地區固定產生埃及 IBAN。美國英語、馬來西亞馬來語和尼泊爾語產生本地帳號。這四種地區傳入 `("", None, None)`。`bankRoutingNumber()` 為 `en_US` 產生九位路由號範例。`Faker::calculateRoutingNumberChecksum` 讀取前八位並回傳檢查碼。兩種產生方法都支援 `unique`。

地區缺少本地 Payment 提供器時，美國提供器負責產生本地帳號和路由號。本地提供器缺少某項方法時，該方法回傳錯誤。回退帳號使用 `("", None, None)`。舊版 `at-AT` 別名使用奧地利帳號和 VAT 格式。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_bank_samples() -> DatabaseResult<(String, String)> {
    let uk = fake(Some("en-GB"))?;
    let us = fake(Some("en-CA"))?;
    Ok((
        uk.bankAccountNumber("BANK", None, None)?,
        us.unique(false).bankRoutingNumber()?,
    ))
}
```

### 金融機構 {#institution-samples}

`bank()` 在 `de_DE`、`el_CY`、`ka_GE`、`kk_KZ`、`ms_MY`、`pl_PL`、`pt_BR`、`ru_RU`、`th_TH`、`uk_UA` 與 `zh_CN` 使用固定地區目錄。馬來西亞馬來語另提供 `localBank`、`foreignBank`、`governmentBank`、`insurance` 和 `currencySymbol`。尼泊爾語提供 `commercialBank`、`developmentBank`、`financeCompany`、`microFinance` 和 `digitalWallet`。兩個地區的 `swiftCode()` 使用各自的目錄。這些產生器支援 `unique`，未支援的地區會回傳錯誤。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn institution_samples() -> DatabaseResult<(&'static str, String)> {
    let faker = fake(Some("ms-MY"))?;
    Ok((faker.foreignBank()?, faker.unique(false).swiftCode()?))
}
```

### VAT 範例 {#vat-samples}

用 `vat(None)` 採用地區預設格式。對支援空格選項的地區，可用 `vat(Some(false))` 產生緊湊格式。支援 `bg_BG`、`de_AT`、`es_ES`、`fr_BE`、`nl_BE`、`fr_FR`、`en_GB`、`it_IT` 和 `nl_NL`。英國的 `vatWithType` 接受 `branch`、`gov` 和 `health`，其他值採用標準格式。法國 VAT 共用 `siren`，義大利 `vatId` 與荷蘭 `btw` 沿用各自的產生規則。各產生器均支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vat_samples() -> DatabaseResult<(String, String)> {
    let french = fake(Some("fr-FR"))?;
    let uk = fake(Some("en-GB"))?;
    Ok((french.unique(false).vat(None)?, uk.vatWithType(Some("branch"))?))
}
```

### 信用卡範例 {#credit-card-samples}

信用卡產生器提供品牌、帶 Luhn 檢查碼的卡號、到期日及完整範例。卡號格式化時預設以連字號分組。`pt-BR` 使用巴西品牌格式。到期日傳入 `true` 時從目前到未來 36 個月取樣，傳入 `false` 時從過去 36 個月到未來 36 個月取樣。`creditCardExpirationDateString` 接受 Chrono 格式指令，預設 `%m/%y`。五個方法都支援 `unique`。

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

### 郵寄地址和稱謂 {#address-and-title-samples}

用 `streetAddress()` 產生街道行，或用 `address()` 產生完整地址樣本。美國、法國、簡體中文和繁體中文使用各自的上游 Faker 排列規則。美國英語、簡體中文和繁體中文使用固定 FakerPHP 的城市、街道範本和地址詞表。其他地區仍可能使用 fake-rs 元件。個別欄位可用 `buildingNumber()`、`secondaryAddress()`、`postcode()`、`state()` 和 `stateAbbr()`。`title(Some("male"))` 和 `title(Some("female"))` 指定稱謂組，`title(None)` 在兩組中隨機選取，也可直接呼叫 `titleMale()` 和 `titleFemale()`。這些方法均支援 `unique`。

簡體中文可用 `area()` 抽取區名，繁體中文可用 `street()` 抽取街道詞幹。臺灣城市樣本先抽取縣市，再從該縣市的區鄉鎮詞表取樣。兩個入口皆支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwan_address_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant-TW"))?.unique(false).address()
}
```

`fr-FR` 使用固定的城市與街道範本及國家詞表。`streetPrefix()` 和 `region()` 從法國詞表取樣。`department()` 回傳行政區編號與名稱的字串組，保留前導零和 `2A` 等編號。`departmentNumber()` 和 `departmentName()` 分別獨立取樣。對 `zh-Hant-TW`，`randomChineseNumber()` 回傳一至九的漢字，`randomNumber2()` 和 `randomNumber3()` 分別回傳 1 到 100、1 到 1000 的整數，含端點。以上入口皆支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn french_department_sample() -> DatabaseResult<(&'static str, &'static str)> {
    fake(Some("fr-FR"))?.unique(false).department()
}
```

比利時、瑞士和加拿大法語採用各自的城市、街道與郵遞區號範本。`cityName()` 用於比利時和瑞士，`province()` 用於比利時，`canton()` 回傳瑞士邦代碼與名稱的字串組。`cantonShort()` 和 `cantonName()` 分別獨立取樣。三地沿用法國的國家、大區和行政區詞表，瑞士使用自己的街道前綴。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn swiss_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("fr-CH"))?;
    Ok((faker.address(), faker.unique(false).cantonShort()?))
}
```

加拿大、澳洲和紐西蘭英語地址依上游規則沿用美國詞表，並套用各地格式。加拿大提供 `province()`、`provinceAbbr()` 和郵遞區號專用字母 `randomPostcodeLetter()`。澳洲使用自己的州與郵遞區號詞表，並提供 `buildingLetter()`。紐西蘭使用 `region()` 和四位郵遞區號。地區方法皆支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn canadian_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("en-CA"))?;
    Ok((faker.address(), faker.unique(false).provinceAbbr()?))
}
```

英國英語使用固定 `en-GB` 郵遞區號詞表以及城市與街道範本。街道以門牌號碼開始，也可以在街道名前獨立輸出 `Flat` 或 `Studio` 一行。用 `county()` 取樣英國郡名。`postcode()` 和 `county()` 皆支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn british_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("en-GB"))?;
    Ok((faker.address(), faker.unique(false).county()?))
}
```

南非英語使用當地省份、簡稱、城市後綴和四位郵遞區號。烏干達英語提供 `cityName()`、`district()` 和 `region()`，郵遞區號為一至五位。奈及利亞英語提供 `county()` 和 `region()`，五位郵遞區號可含空格。上述地區方法皆支援 `unique`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ugandan_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("en-UG"))?;
    Ok((faker.address(), faker.unique(false).district()?))
}
```

新加坡英語沿用以姓氏組成的街名，並使用當地街道後綴與六位郵遞區號。完整地址可使用門牌號碼，也可使用棟號和單元列。固定版 FakerPHP 的 `floorNumber()` 與 `apartmentNumber()` 傳回原始範本字串，完整地址也保留這些字串。地區方法支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn singapore_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en-SG"))?;
    Ok((faker.address(), faker.blockNumber()?))
}
```

印度英語使用當地城市、邦和街區詞表。完整地址末尾是以 1–5 開頭的六位郵遞區號。街道地址由門牌和街區組成，也可包含住宅社區。`areaSuffix()` 保留詞表中的前導空格。街區和社區的五個方法皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn indian_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en-IN"))?;
    Ok((faker.address(), faker.unique(false).societyName()?))
}
```

菲律賓英語地址使用當地省份、城市和市鎮，郵遞區號為四位，也可包含公寓或套房編號。`municipality()` 和 `barangay()` 提供當地欄位，獨立的 `state()` 和 `stateAbbr()` 沿用美國詞表。香港英語地址首列為街道、村落或屋苑，其後依序為地區和城市。繼承的五位 `postcode()` 可單獨呼叫。地區方法支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn philippine_and_hong_kong_address_samples() -> DatabaseResult<(String, String)> {
    let philippines = fake(Some("en-PH"))?;
    let hong_kong = fake(Some("en-HK"))?;
    Ok((philippines.address(), hong_kong.address()))
}
```

日語地址以七位郵遞區號開頭，空兩格後依序接上都道府縣、市、區與街道，也可包含大樓名稱和編號。`postcode1()` 與 `postcode2()` 傳回三位和四位的郵遞區號組件。`prefecture()`、`ward()` 與 `areaNumber()` 也可單獨呼叫。這些地區方法支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn japanese_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("ja-JP"))?;
    Ok((faker.address(), faker.unique(false).prefecture()?))
}
```

韓語完整地址先寫特別市或廣域市及其區，或寫道及市，接著是街道名與門牌號碼。基礎 `streetAddress()` 會將門牌號碼放在前面。`postcode()` 可單獨產生五位郵遞區號。`metropolitanCity()`、`province()` 和 `borough()` 支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn korean_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("ko-KR"))?;
    Ok((faker.address(), faker.unique(false).metropolitanCity()?))
}
```

越南語地址可列出街道、坊和縣，也可在門牌號碼後加入村組。第二列為城市或省。固定版坊名範本也可使用村組前綴。村組、坊和縣的名稱及前綴支援 `unique()`。`postcode()` 可單獨產生五位、六位或五位加連字號與四位的郵遞區號。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vietnamese_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("vi-VN"))?;
    Ok((faker.address(), faker.unique(false).districtName()?))
}
```

泰語地址分兩列。第一列是門牌號碼與道路名稱，道路名稱由前綴與詞表項目組成。第二列是由人名生成的城市名稱、逗號及五位郵遞區號。門牌號碼可含斜線和前導零。`province()` 可單獨產生省名。道路前綴與省名支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn thai_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("th-TH"))?;
    Ok((faker.address(), faker.unique(false).province()?))
}
```

印尼語地址在門牌號碼前加 `No.`，五位郵遞區號首位非零，末尾使用州名簡稱。馬來語地址的城鎮、郵遞區號和州名取自同一地區。可用 `postcodeWithState("perlis")` 按 FakerPHP 的州鍵產生郵遞區號。`postcode()` 隨機選擇地區，郵遞區號保留前導零。馬來語大樓和社區方法支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn indonesian_and_malaysian_address_samples() -> DatabaseResult<(String, String, String)> {
    let indonesia = fake(Some("id-ID"))?;
    let malaysia = fake(Some("ms-MY"))?;
    Ok((indonesia.address(), malaysia.address(), malaysia.postcodeWithState("perlis")?))
}
```

孟加拉語地址的街號使用孟加拉數字。獨立的 `postcode()` 使用 ASCII 數字，可附四位延伸碼。尼泊爾語地址以連字號連接城市與一位或兩位選區編號，再接上街名、縣和五位郵遞區號。地區方法支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn bengali_and_nepali_address_samples() -> DatabaseResult<(String, String)> {
    let bengali = fake(Some("bn-BD"))?;
    let nepali = fake(Some("ne-NP"))?;
    Ok((bengali.address(), nepali.address()))
}
```

埃及、約旦和沙烏地阿拉伯的阿拉伯語地址將街道和城市分兩列，可附公寓或大樓資訊。約旦城市名稱可帶方位前綴，獨立的州名和簡稱沿用固定版的美國詞表。埃及提供 `governorate()` 和 `governorateId()`，沙烏地阿拉伯提供 `governorate()` 和 `subdivision()`。地區方法皆支援 `unique()`。固定版沙烏地提供器缺少城市前綴詞表，單獨呼叫 `cityPrefix()` 時沿用 Rust 現有的遞補行為。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn arabic_address_samples() -> DatabaseResult<(String, String, String)> {
    Ok((fake(Some("ar-EG"))?.address(), fake(Some("ar-JO"))?.address(), fake(Some("ar-SA"))?.address()))
}
```

波斯語地址由城市、街道和大樓組成，末尾可加帶標籤的十位郵遞區號。大樓和郵遞區號的前綴可單獨呼叫，並支援 `unique()`。希伯來語地址分兩列，郵遞區號為五位或七位。沒有公寓或樓層時，街道列末尾保留一個空格。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn persian_and_hebrew_address_samples() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("fa-IR"))?.address(), fake(Some("he-IL"))?.address()))
}
```

土耳其語地址將門牌號碼放在街名後面。第二列以五位郵遞區號開頭，接著是 81 個城市詞表中的一個。街名沿用固定版名字與姓氏的抽樣權重。可用 `cityName()` 直接抽取城市，也支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn turkish_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("tr-TR"))?;
    Ok((faker.address(), faker.unique(false).cityName()?))
}
```

俄語地址使用六位郵遞區號，城市和地區各自抽樣。烏克蘭語地址使用五位郵遞區號，城市與州成對抽樣，可透過 `cityAndRegion()` 單獨取得配對，也支援 `unique()`。哈薩克語地址以首位為零的六位郵遞區號開頭，街道後綴放在街名後。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ukrainian_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("uk-UA"))?;
    Ok((faker.address(), faker.unique(false).cityAndRegion()?))
}
```

波蘭語郵遞區號由兩位數字、連字號和三位數字組成。捷克語地址可另含一列地區或國家，也可能省略門牌號碼。斯洛伐克語郵遞區號在前三位後加空格，固定版完整地址以字面字元 `\n` 分隔。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn central_european_address_samples() -> DatabaseResult<(String, String, String)> {
    Ok((fake(Some("pl-PL"))?.address(), fake(Some("cs-CZ"))?.address(), fake(Some("sk-SK"))?.address()))
}
```

德國、奧地利和瑞士的德語地址將門牌號碼放在街名後，第二列先寫郵遞區號再寫城市。奧地利從詞表抽取完整郵遞區號，瑞士德語街名可用城市名稱作為詞幹。`streetSuffixShort()` 和 `streetSuffixLong()` 提供當地街道後綴。瑞士德語的 `canton()` 傳回同一筆記錄中的邦簡稱和德語邦名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn swiss_german_address_sample() -> DatabaseResult<(String, (&'static str, &'static str))> {
    let faker = fake(Some("de-CH"))?;
    Ok((faker.address(), faker.unique(false).canton()?))
}
```

荷蘭和比利時的荷蘭語地址將姓氏與街道後綴直接拼接，接著加上門牌號碼。兩地分別從自己的詞表抽取完整郵遞區號。比利時荷蘭語保留第二列郵遞區號前的一個空格。`cityName()` 和 `state()` 從當地城市與省名詞表抽樣。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn dutch_address_samples() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("nl-NL"))?.address(), fake(Some("nl-BE"))?.address()))
}
```

挪威語與瑞典語地址將門牌號碼放在街名之後，門牌末尾的字母使用大寫。挪威郵遞區號為四位，可用零開頭。瑞典郵遞區號為五位，可在第三位後加空格。`streetSuffixWord()` 從當地獨立街道後綴詞表抽樣，並支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn norwegian_and_swedish_address_samples() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("nb-NO"))?.address(), fake(Some("sv-SE"))?.address()))
}
```

丹麥語地址可附樓層和住戶方位，也可用 `buildingLevel()`、`buildingSide()` 和 `kommune()` 單獨產生欄位，並透過 `unique()` 抽樣。芬蘭語地址可附三位公寓編號和地區名。冰島語地址使用三位郵遞區號，門牌號碼可附大寫字母。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nordic_address_samples() -> DatabaseResult<(String, String, String)> {
    Ok((fake(Some("da-DK"))?.address(), fake(Some("fi-FI"))?.address(), fake(Some("is-IS"))?.address()))
}
```

西語地址提供器涵蓋西班牙、阿根廷、秘魯與委內瑞拉。阿根廷完整地址使用省級縮寫。秘魯門牌號碼前保留字面值 ` # `，無補充地址時保留結尾空格。委內瑞拉完整地址可省略郵遞區號。四個地區共用自西班牙繼承的國家和自治區詞表。使用 `community()` 抽取自治區，也支援透過 `unique()` 呼叫。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn spanish_address_samples() -> DatabaseResult<[String; 4]> {
    Ok([
        fake(Some("es-ES"))?.address(),
        fake(Some("es-AR"))?.address(),
        fake(Some("es-PE"))?.address(),
        fake(Some("es-VE"))?.address(),
    ])
}
```

巴西地址以郵遞區號開頭，第二行包含州縮寫。可用 `region()` 和 `regionAbbr()` 抽取巴西五大區及其縮寫，也支援 `unique()`。葡萄牙地址使用固定城市詞表，郵遞區號有四位和七位兩種格式，門牌和樓層可選。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn portuguese_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("pt-BR"))?.address(), fake(Some("pt-PT"))?.address()])
}
```

義大利地址包含五位郵遞區號及省級欄位。省名和縮寫依照 FakerPHP 的行為獨立抽樣。瑞士義大利語地址使用四位郵遞區號、固定城市詞表，門牌號碼可以帶小寫字母。`canton()` 回傳州縮寫和義大利語州名組成的元組。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn italian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("it-IT"))?.address(), fake(Some("it-CH"))?.address()])
}
```

希臘地址保留兩種郵遞區號格式、可帶連字號的門牌號碼，以及可透過 `prefecture()` 取得的地區詞表。街道名稱抽取男性或女性姓氏形式。賽普勒斯希臘語地址使用四位郵遞區號及四個城市之一。上游沒有街道後綴，Rust 以空字串表示。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn greek_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("el-GR"))?.address(), fake(Some("el-CY"))?.address()])
}
```

羅馬尼亞與摩爾多瓦羅馬尼亞語地址保留四種街道配置，可包含樓棟、樓梯單元、樓層及公寓號碼。羅馬尼亞地址包含縣名與六位郵遞區號。摩爾多瓦羅馬尼亞語地址使用四位郵遞區號。樓層和公寓號碼可帶前導零。可用 `block()`、`blockSegment()`、`floor()` 和 `apartmentNumber()` 分別抽取這些欄位。`streetPlainName()` 回傳省略前綴的街名，`county()` 抽取羅馬尼亞縣名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn romanian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("ro-RO"))?.address(), fake(Some("ro-MD"))?.address()])
}
```

克羅埃西亞地址自當地詞表抽取街名和城市，使用五位郵遞區號，也可透過 `street()` 單獨抽取街名。斯洛維尼亞郵遞區號為四位，末位固定為零。斯洛維尼亞完整地址保留 FakerPHP 中兩處字面的 `\n` 分隔符。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn croatian_slovenian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("hr-HR"))?.address(), fake(Some("sl-SI"))?.address()])
}
```

立陶宛郵遞區號使用 `LT-` 前綴，地址有三種配置，其中一種以連字號連接門牌與單元號碼。可用 `municipality()` 抽取立陶宛市鎮。拉脫維亞地址以 `LV-` 郵遞區號開頭，接著是地區和城市。兩地均可用 `regionSuffix()` 取得當地的地區後綴。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn lithuanian_latvian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("lt-LT"))?.address(), fake(Some("lv-LV"))?.address()])
}
```

亞美尼亞地址以地區開頭，以 `00` 開頭的四位郵遞區號結尾。喬治亞地址保留八種配置，國家和地區欄位可選。可用 `regionGenitiveForm()` 抽取與地區後綴連用的喬治亞語屬格地區名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn armenian_georgian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("hy-AM"))?.address(), fake(Some("ka-GE"))?.address()])
}
```

塞爾維亞地址在 `sr-RS` 和 `sr-Cyrl-RS` 使用西里爾字母，在 `sr-Latn-RS` 使用拉丁字母。蒙特內哥羅地址使用當地城市和街道詞表。蒙特內哥羅的街道前綴為空，選中帶前綴的模板時保留前導空格。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn serbian_montenegrin_address_samples() -> DatabaseResult<[String; 4]> {
    Ok([
        fake(Some("sr-RS"))?.address(),
        fake(Some("sr-Cyrl-RS"))?.address(),
        fake(Some("sr-Latn-RS"))?.address(),
        fake(Some("me-ME"))?.address(),
    ])
}
```

匈牙利地址以四位郵遞區號開頭，按權重抽取首都、大城市及小城市模板。可用 `capital()`、`bigCity()` 或 `smallerCity()` 指定城市類別。地區缺少 FakerPHP Address 提供器時，`address()` 使用美國地址配置和地址詞表。地址中的人名仍採用指定地區現有的 Person 提供器。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn hungarian_and_fallback_address_samples() -> DatabaseResult<[String; 4]> {
    Ok([
        fake(Some("hu-HU"))?.address(),
        fake(Some("bg-BG"))?.address(),
        fake(Some("cy-GB"))?.address(),
        fake(Some("zz-ZZ"))?.address(),
    ])
}
```

OpenRust 已驗證固定 FakerPHP 版本的全部 71 個本地 Address 提供器。地址樣本遵循當地格式和詞表。Rust 和 PHP 使用獨立的亂數序列，因此相同種子可能產生不同的樣本值。

### 依性別產生名字 {#given-names-by-gender}

用 `firstNameMale()` 或 `firstNameFemale()` 從對應的上游 Faker 名字清單取樣。`firstNameWithGender(Some("male"))` 和 `nameWithGender(Some("female"))` 明確選擇分組。傳入 `None` 或未識別的分組會從兩組取樣。全名依地區範本組合現有姓氏資料，繁體中文名字由一個或兩個抽取字元組成。缺少地區清單時使用英文名字和範本。

`en-US`、`fr-FR`、`it-IT` 和 `ja-JP` 的 `name()`、`firstName()` 與 `lastName()` 使用固定 Person 範本和詞表。公司範本共用同一套姓氏。`firstNameWithGender` 和 `nameWithGender` 與對應的無參數方法共用 `unique` 歷史。

`ar-SA`、`de-DE`、`fa-IR`、`nl-NL`、`pt-BR`、`pt-PT`、`tr-TR`、`zh-Hans` 和 `zh-Hant` 也使用對應的 Person 範本和詞表。荷蘭 Person 與 Company 使用同一套姓氏取樣規則，長姓、常見荷蘭姓、常見外來姓與一般荷蘭姓的權重依序為 1:10:5:10。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_name_sample() -> DatabaseResult<String> {
    fake(Some("nl-NL"))?.unique(false).name()
}
```

### 北歐姓名 {#nordic-names}

丹麥和冰島的姓名範本可以包含中間名。可用 `middleName()` 單獨產生。`is-IS` 的 `lastName()` 回傳父名詞幹，`lastNameMale()` 和 `lastNameFemale()` 加上對應後綴。芬蘭姓名可包含當地稱謂。瑞典語和挪威書面語姓名使用名字加姓氏。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nordic_name_sample() -> DatabaseResult<String> {
    fake(Some("is-IS"))?.unique(false).nameWithGender(Some("female"))
}
```

### 波蘭和立陶宛姓名 {#polish-and-lithuanian-names}

`pl-PL` 和 `lt-LT` 可透過 `lastNameWithGender(Some("male"))` 或 `lastNameWithGender(Some("female"))` 選擇姓氏詞表。`lastNameMale()` 和 `lastNameFemale()` 使用相同詞表。固定版立陶宛的無參數 `lastName()` 從名字取樣，全名則使用對應性別的姓氏。`lastNameWithGender` 和 `lastName` 共用去重歷史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn gendered_surname_sample() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).lastNameWithGender(Some("female"))
}
```

### 英國、南非、賽普勒斯與法語及義大利語地區姓名 {#additional-regional-names}

`en-GB`、`en-ZA`、`el-CY`、`fr-BE`、`fr-CA`、`fr-CH` 和 `it-CH` 使用地區 Person 的名字、姓氏和實際生效的全名範本。瑞士法語和義大利語沿用各自基礎類別的範本和稱謂，使用瑞士當地姓名詞表。Person 與 Company 共用姓氏詞表。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_person_sample() -> DatabaseResult<String> {
    fake(Some("fr-CH"))?.unique(false).nameWithGender(Some("female"))
}
```

### 西語地區姓名 {#spanish-language-names}

`es-AR`、`es-ES`、`es-PE` 和 `es-VE` 使用固定 Person 的名字、姓氏和稱謂。全名保留各地區可選的姓名後綴與範本權重。委內瑞拉範本可組合多個名字或姓氏。Company 共用姓氏詞表。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn spanish_name_sample() -> DatabaseResult<String> {
    fake(Some("es-VE"))?.unique(false).nameWithGender(Some("female"))
}
```

### 中歐地區姓名 {#central-european-names}

`de-AT`、`de-CH`、`cs-CZ`、`sk-SK`、`hu-HU` 和 `hr-HR` 使用固定 Person 範本，並與 Company 共用姓氏詞表。捷克和斯洛伐克支援 `lastNameWithGender`、`lastNameMale` 和 `lastNameFemale`。匈牙利範本保留姓氏在前及婚後姓名格式，可透過 `firstNameMaleNe()` 與 `lastNameFemaleMarried()` 單獨取樣。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn central_european_name_sample() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).lastNameWithGender(Some("female"))
}
```

### 希臘、保加利亞和斯洛維尼亞姓名 {#balkan-names}

`el-GR` 和 `bg-BG` 使用對應性別的姓氏詞表及全名範本。可用 `lastNameWithGender`、`lastNameMale` 或 `lastNameFemale` 選擇姓氏。斯洛維尼亞兩種姓氏方法使用同一詞表。`title(gender)` 使用統一的當地稱謂，`titleMale()` 與 `titleFemale()` 沿用基礎類別稱謂。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn balkan_name_sample() -> DatabaseResult<String> {
    fake(Some("bg-BG"))?.unique(false).nameWithGender(Some("female"))
}
```

### 埃及及西亞地區姓名 {#egyptian-and-west-asian-names}

`ar-EG`、`ar-JO`、`he-IL`、`hy-AM` 和 `ka-GE` 使用固定 Person 的當地名字、姓氏與稱謂。埃及全名範本可包含縮寫前綴。亞美尼亞和喬治亞依名字加姓氏的範本產生全名。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn west_asian_name_sample() -> DatabaseResult<String> {
    fake(Some("ka-GE"))?.unique(false).name()
}
```

### 俄語、烏克蘭語和哈薩克語姓名 {#russian-ukrainian-and-kazakh-names}

`ru-RU`、`uk-UA` 和 `kk-KZ` 使用固定 Person 的地區詞表和全名範本。俄語和烏克蘭語的 `middleName()` 從兩組父名取樣，`middleNameWithGender`、`middleNameMale()` 與 `middleNameFemale()` 可選擇分組。俄語姓氏支援性別選擇並保留固定的女性後綴。哈薩克語 PHP provider 使用數字性別常數。Rust 沿用統一的 `"male"` 和 `"female"` 選擇對應範本。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn patronymic_sample() -> DatabaseResult<&'static str> {
    fake(Some("uk-UA"))?.unique(false).middleNameWithGender(Some("female"))
}
```

### 南亞、東南亞和韓國姓名 {#south-southeast-asian-and-korean-names}

`bn-BD`、`id-ID`、`ms-MY`、`ko-KR`、`th-TH` 和 `vi-VN` 使用各地的姓名詞表與全名範本。印尼姓氏支援性別選擇。韓文全名姓氏在前，中間無空格。越南語 `middleName()` 從名字詞表抽樣，明確傳入性別的 `middleNameWithGender` 使用對應的中間名詞表。馬來西亞 `name()` 使用馬來、華人和印度裔全名範本，`firstName()` 也包含基督教名字。明確選擇男性或女性名字時，沿用固定 provider 繼承的 `John` 和 `Jane`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vietnamese_middle_name_sample() -> DatabaseResult<&'static str> {
    fake(Some("vi-VN"))?.unique(false).middleNameWithGender(Some("female"))
}
```

`ms-MY` 可直接產生姓名範本的各個成分。馬來姓名使用 `firstNameMaleMalay`、`firstNameFemaleMalay`、`lastNameMalay`、`muhammadName`、`nurName`、`haji`、`hajjah` 和 `titleMaleMalay`。其餘介面包括 `lastNameChinese`、`firstNameMaleChinese`、`firstNameFemaleChinese`、`firstNameMaleChristian`、`firstNameFemaleChristian`、`initialIndian`、`firstNameMaleIndian`、`firstNameFemaleIndian` 和 `lastNameIndian`。可選成分可以為空，非空時保留詞表原有的結尾空格。

`myKadNumber(gender, formatted, separator)` 產生樣本號碼。關閉格式化時輸出 12 位數字，開啟後以連字號或指定分隔符號分為六位、兩位和四位三組。末位在 `"male"` 時為奇數，`"female"` 時為偶數，其餘情況無限制。月、日分別抽樣。以上介面均支援 `unique()`，在 `ms-MY` 以外的地區回傳錯誤。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn malaysian_person_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("ms-MY"))?;
    let unique = faker.unique(false);
    Ok((unique.firstNameMaleMalay()?, unique.myKadNumber(Some("male"), true, None)?))
}
```

### 羅馬尼亞語和塞爾維亞語姓名 {#romanian-and-serbian-names}

`ro-RO` 和 `ro-MD` 共用羅馬尼亞語姓名及稱謂詞表。`sr-RS` 和 `sr-Cyrl-RS` 使用各自的西里爾文字詞表，`sr-Latn-RS` 使用拉丁文字詞表。

在 `ro-RO` 呼叫 `cnp(gender, dateOfBirth, county, isResident)` 可產生 13 位數樣本號碼。出生日期可省略，也可在 1800–2099 年內使用 `YYYY`、`YYYY-MM` 或 `YYYY-MM-DD`。日期缺少的欄位取自該年隨機一天。抽到的日若超出指定月份的天數，呼叫回傳錯誤。縣代碼區分大小寫。此方法會計算校驗位並支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn romanian_cnp_sample() -> DatabaseResult<String> {
    fake(Some("ro-RO"))?.unique(false).cnp(Some("female"), Some("1981-06-16"), Some("B1"), true)
}
```

### 愛沙尼亞、拉脫維亞和蒙特內哥羅姓名 {#estonian-latvian-and-montenegrin-names}

`et-EE`、`lv-LV` 和 `me-ME` 使用各地的名字和姓氏詞表，沿用名字在前的基礎範本。拉脫維亞 `driverLicence()` 輸出兩個小寫 ASCII 字母和六位數字。護照號碼使用七位數字。可用 `personalIdentityNumberWithBirthdate` 傳入公曆出生日期。這些身分格式化器均支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn latvian_driver_licence() -> DatabaseResult<String> {
    fake(Some("lv-LV"))?.unique(false).driverLicence()
}
```

### 印度英語和尼泊爾語姓名 {#indian-english-and-nepali-names}

`en-IN` 和 `ne-NP` 使用各地的名字、姓氏和全名範本。印度英語提供 `middleNameMale()`，尼泊爾語同時提供 `middleNameMale()` 和 `middleNameFemale()`。可直接呼叫，或向 `middleNameWithGender` 傳入對應性別。兩個地區呼叫無參數的 `middleName()` 都會回傳該地區不支援此介面的錯誤。依性別產生的介面均支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nepali_middle_name() -> DatabaseResult<&'static str> {
    fake(Some("ne-NP"))?.unique(false).middleNameFemale()
}
```

### 奈及利亞和烏干達英語姓名 {#nigerian-and-ugandan-english-names}

`en-NG` 和 `en-UG` 使用各地的姓名詞表與範本。奈及利亞全名可含一個或兩個名字，姓氏在後。烏干達範本可將名字或姓氏放在前面。烏干達無參數的 `lastName()` 從通用姓氏詞表抽樣。可用 `lastNameMale()`、`lastNameFemale()` 或向 `lastNameWithGender` 傳入明確的性別，從對應詞表抽樣。這些介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ugandan_surname() -> DatabaseResult<String> {
    fake(Some("en-UG"))?.unique(false).lastNameFemale()
}
```

### 蒙古語姓名 {#mongolian-names}

`mn-MN` 全名使用一個西里爾首字母、句點和名字，句點後沒有空格。`namePrefix()` 從 30 個姓名首字母中抽樣。`alphabet()` 使用 `idNumber()` 的獨立 35 字母詞表。兩個介面均回傳 Rust `char`，並支援 `unique()`。單獨呼叫姓氏產生介面會回傳基礎 provider 的 `Doe`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn mongolian_initial() -> DatabaseResult<char> {
    fake(Some("mn-MN"))?.unique(false).namePrefix()
}
```

### 新加坡英語姓名和證件樣本 {#singapore-english-names-and-identity-samples}

`en-SG` 沿用基礎姓名 `John Doe` 和 `Jane Doe`。用 `nric(birthDate)` 或 `fin(issueDate)` 產生證件樣本，也可用 `singaporeId(issueDate, foreigner)` 指定類型。日期接受選填的公曆日期，省略時使用 `dateTimeThisCentury()` 的日期範圍。字首以 2000 年為分界。NRIC 從 1968 年起取年份後兩位，更早的日期從 `00` 和 `01` 抽樣。三個介面均計算結尾校驗字母並支援 `unique()`。

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn singapore_identity_sample() -> DatabaseResult<String> {
    fake(Some("en-SG"))?.unique(false).nric(NaiveDate::from_ymd_opt(2000, 1, 1))
}
```

### 比利時荷蘭語姓名和 RRN 樣本 {#belgian-dutch-names-and-rrn-samples}

`nl-BE` 使用當地名字和姓氏詞表，並沿用名字在前的基礎範本。`rrn(gender)` 使用 `dateTimeThisCentury()` 抽取出生日期並產生 11 位數樣本。三位序號在 `"male"` 時為奇數，`"female"` 時為偶數，其他情況不限。結尾兩位按模 97 計算，2000 年起的出生日期使用對應的世紀偏移。此介面支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn belgian_rrn_sample() -> DatabaseResult<String> {
    fake(Some("nl-BE"))?.unique(false).rrn(Some("female"))
}
```

### 姓名前綴和後綴 {#person-name-prefixes-and-suffixes}

在提供對應格式化器的地區使用 `prefix()` 和 `suffix()`。預設 `en-US` 提供 `suffix()`，與全名範本共用後綴詞表。義大利語 `it-IT` 和 `it-CH` 的上游空值以 `None` 回傳，亞美尼亞語和臺灣華語回傳 `Some("")`。不支援此介面的地區回傳錯誤。兩個方法皆支援 `unique()`，每次去重歷史中空值或空字串各只能回傳一次。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_affixes() -> DatabaseResult<(&'static str, Option<&'static str>)> {
    Ok((fake(Some("fr-FR"))?.prefix()?, fake(None)?.suffix()?))
}
```

### 日語假名姓名 {#japanese-kana-names}

在 `ja-JP` 呼叫 `kanaName(gender)` 可產生片假名全名，姓氏在前，以一個空格分隔。`firstKanaName(gender)` 產生名字，`lastKanaName()` 產生姓氏。可傳入 `Some("male")`、`Some("female")` 或 `None`，分別選擇男性、女性或任一名字詞表。`firstKanaNameMale()` 和 `firstKanaNameFemale()` 直接選擇對應詞表。五個介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn japanese_kana_name() -> DatabaseResult<String> {
    fake(Some("ja-JP"))?.unique(false).kanaName(Some("female"))
}
```

### 瑞士 AHV 和 AVS 樣本 {#swiss-ahv-and-avs-samples}

用 `de-CH`、`fr-CH` 或 `it-CH` 的 `avs13()` 產生 `756.####.####.##` 格式的瑞士社會保險號碼樣本。瑞士德語也提供 `ahv13()`，格式和 EAN 校驗位相同。兩個介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn swiss_avs_sample() -> DatabaseResult<String> {
    fake(Some("fr-CH"))?.unique(false).avs13()
}
```

### 捷克出生號碼和丹麥 CPR {#czech-birth-numbers-and-danish-cpr}

在 `cs-CZ` 呼叫 `birthNumber(gender, minAge, maxAge, slashProbability)` 產生捷克出生號碼樣本，數值參數指定年齡上下限和出現斜線的百分比。`birthNumberMale()` 和 `birthNumberFemale()` 預設使用 0 到 100 歲及 50% 的斜線機率。產生器保留女性月份偏移、2004 年起的額外月份偏移和 1954 年起的校驗位。年齡上下限顛倒時回傳錯誤。`da-DK` 的 `cpr()` 抽取公曆出生日期及首位非零的四位序號。兩地介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn czech_birth_number() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).birthNumber(Some("female"), 18, 65, 100)
}
```

### 法國 NIR 和義大利稅號樣本 {#french-nir-and-italian-tax-id-samples}

在 `fr-FR` 或 `fr-CH` 中使用 `nir(gender, formatted)`。用 `Some("male")` 或 `Some("female")` 選擇首位，或用 `None` 隨機選擇。NIR 使用法國省級行政區代碼和模 97 校驗位。緊湊格式有 15 個字元，含空格格式按 1–2–2–2–3–3–2 分組。在 `it-IT` 或 `it-CH` 中使用 `taxId()` 產生 FakerPHP 的 16 位大寫稅號樣本。兩個介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn french_nir_sample() -> DatabaseResult<String> {
    fake(Some("fr-FR"))?.unique(false).nir(Some("female"), true)
}
```

### 伊朗國民號碼和印尼 NIK 樣本 {#iranian-national-codes-and-indonesian-nik-samples}

在 `fa-IR` 用 `nationalCode()` 產生十位數樣本，包含非零地區代碼和模 11 校驗位。在 `id-ID` 用 `nik(gender, birthDate)` 產生 16 位數樣本，開頭取自固定出生地代碼詞表，號碼內包含出生日期。女性樣本的出生日期日數加 40。日期傳入 `None` 時從過去 30 年抽樣，也可傳入 `NaiveDate` 指定公曆日期。兩個介面皆支援 `unique()`。

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn indonesian_nik_sample() -> DatabaseResult<String> {
    fake(Some("id-ID"))?.unique(false).nik(Some("female"), NaiveDate::from_ymd_opt(2000, 2, 29))
}
```

### 哈薩克 IIN 和委內瑞拉身分證號碼樣本 {#kazakh-iin-and-venezuelan-national-id-samples}

在 `kk-KZ` 中使用 `individualIdentificationNumber(birthDate, gender)`。生日接受 1800–2099 年的選填公曆日期，預設從過去 30 年抽樣，性別預設為男性。`Faker::checkSum(prefix)` 接收 11 位 ASCII 數字並回傳校驗值，兩輪均得到 10 時，產生器會重新抽取序號。在 `es-VE` 中用 `nationalId(separator)` 設定 `V` 或 `E` 後的分隔符號。兩個產生器皆支援 `unique()`。

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn kazakh_iin_sample() -> DatabaseResult<String> {
    fake(Some("kk-KZ"))?.unique(false)
        .individualIdentificationNumber(NaiveDate::from_ymd_opt(2000, 2, 29), Some("female"))
}
```

使用 `es-VE` 的 `taxpayerIdentificationNumber()` 產生委內瑞拉 RIF 樣本。格式由六種前綴之一（`J`、`G`、`V`、`E`、`P`、`C`）、八位主體數字和一位隨機尾數組成。用 `taxpayerIdentificationNumberWithSeparator(Some("-"))` 在主體兩側加入分隔符。兩個入口共用 `unique()` 的去重記錄。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn venezuelan_rif_sample() -> DatabaseResult<String> {
    fake(Some("es-VE"))?
        .unique(false)
        .taxpayerIdentificationNumberWithSeparator(Some("-"))
}
```

### 葡萄牙 NIF 和臺灣華語名字詞表 {#portuguese-nif-and-taiwanese-name-pools}

在 `pt-PT` 中用 `taxpayerIdentificationNumber()` 產生九位數 NIF 樣本。`Faker::dvCalcMod11(number)` 接收 ASCII 數字字串計算校驗位，可保留前導零。在 `zh-TW` 中，`randomName(pool, count)` 從字串切片獨立抽樣後拼接，可以重複抽到同一項。詞表為空或抽樣數為零時回傳空字串。兩個產生器皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwanese_name_sample() -> DatabaseResult<String> {
    fake(Some("zh-TW"))?.randomName(&["明", "安"], 2)
}
```

### 波羅的海駕駛執照和護照樣本 {#baltic-driving-licence-and-passport-samples}

`lt-LT` 和 `lv-LV` 均支援 `driverLicence()` 與 `passportNumber()`。立陶宛兩種樣本都是八位 ASCII 數字，可保留前導零。拉脫維亞樣本以兩個小寫 ASCII 字母開頭，駕駛執照後接六位數字，護照後接七位數字。兩個介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn baltic_driver_licence() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).driverLicence()
}
```

### 馬來西亞車牌 {#malaysian-registration-plates}

使用 `ms-MY` 的 `jpjNumberPlate()` 產生馬來西亞車牌。帶權重的前綴、特殊前綴的大小寫及可為空的字母均沿用固定版 FakerPHP。也可個別呼叫 `peninsularPrefix()`、`sarawakPrefix()`、`sabahPrefix()`、`specialPrefix()`、`validAlphabet()` 和 `numberSequence()`。這些方法均支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn malaysian_plate_sample() -> DatabaseResult<String> {
    fake(Some("ms-MY"))?.unique(false).jpjNumberPlate()
}
```

### 波蘭車牌 {#polish-registration-plates}

使用 `pl-PL` 的 `licensePlate()` 產生一般波蘭車牌。`licensePlateWithOptions(special, voivodeships, counties)` 可依省和縣名篩選。啟用特殊車牌後，可在省級篩選中指定 `army` 或 `services` 產生軍用或機構車牌。篩選沒有符合項目時，從該級可用項目中抽取。OpenRust 依地區選擇提供器，FakerPHP 需顯式註冊波蘭 LicensePlate 提供器。兩個 Rust 方法共用 `unique()` 歷史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn polish_plate_sample() -> DatabaseResult<String> {
    fake(Some("pl-PL"))?.unique(false).licensePlateWithOptions(
        false, Some(&["dolnośląskie"]), Some(&["Wrocław"]),
    )
}
```

### Person 地區回退 {#person-locale-fallback}

當指定地區沒有 Person provider 時，姓名介面、`suffix()` 和 `ssn()` 使用 `en-US` Person provider。已有 Person provider 的地區使用自身和繼承的介面。例如，`en-CA` 會回退到美國英語 Person，`en-SG` 有自己的 Person provider，呼叫 `ssn()` 回傳不支援錯誤。此回退僅適用於 Person 介面。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_fallback_sample() -> DatabaseResult<String> {
    fake(Some("en-CA"))?.unique(false).ssn()
}
```

### 公司標語與商業短語 {#company-slogans-and-business-phrases}

用 `catchPhrase()` 產生公司標語，用 `bs()` 產生商業短語。兩者皆使用目前地區的 Company Provider。地區沒有 Company Provider 時回退至 `en-US`。地區有自己的 Provider 但缺少對應介面時回傳錯誤。`ar-SA`、`ar-JO` 和 `hy-AM` 的 `bs()` 回傳空字串。`hy-AM` 的 `catchPhrase()` 也回傳空字串。`zh-TW` 的商業短語直接串接詞組，不加空格。

法語和捷克語 Company Provider 也提供 `catchPhraseNoun()`、`catchPhraseVerb()` 和 `catchPhraseAttribute()`，比利時、加拿大和瑞士的法語 Provider 同樣支援。法語標語會過濾重複出現的 `sécurité` 和 `simpl`。五個介面皆支援 `unique()`。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn company_phrase_sample() -> DatabaseResult<(String, String)> {
    let french = fake(Some("fr-CA"))?;
    let taiwanese = fake(Some("zh-TW"))?;
    Ok((french.unique(false).catchPhrase()?, taiwanese.bs()?))
}
```

### 座標 {#coordinate-samples}

用 `latitude(None, None)?` 和 `longitude(None, None)?` 產生帶正負號的十進位度數，四捨五入至小數點後六位。任一邊界可傳入 `Some(value)`，預設範圍分別為 −90 到 90 和 −180 到 180。接受反向邊界，非有限邊界回傳錯誤。`localCoordinates()?` 回傳含 `latitude` 和 `longitude` 欄位的可序列化 `Coordinates`。匈牙利和蒙特內哥羅採用固定上游地區範圍，其他地區採用全球範圍。臺灣範圍在 `zh_TW` 下由獨立的 `localLatitude()?` 和 `localLongitude()?` 提供。五個方法均支援 `unique`。

### 軟刪除狀態 {#trashed-state}

對已設定 `softDeletes` 的模型，在工廠或批次工廠使用 `trashed(None)?`。它追加模型所宣告的刪除欄位狀態，依選定連線的應用程式時區計算前一日同一時間。傳入 `Some(value)` 可指定刪除日期。後續狀態和明確指定的建立屬性可覆寫此值。未啟用軟刪除的模型會回傳錯誤。工廠按照一般建立生命週期產生處於此狀態的新模型。

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

### 條件狀態 {#conditional-states}

用 `when(condition, transform)` 在布林條件為真時轉換工廠，或用 `unless(condition, transform)` 在條件為假時轉換。`whenElse` 和 `unlessElse` 的第三個參數是備用轉換。選中的閉包在設定工廠時執行一次，並回傳工廠以繼續鏈式呼叫。這些方法可在 `count` 前後使用。

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

## 使用工廠建立模型

`state` 按宣告順序疊加。`sequence` 從零開始，同一建構器上的各次呼叫接續其索引。新建構器重新從零開始。並行呼叫取得不同索引，依實際執行順序分配。`make` 建立記憶體模型並執行 `afterMaking`。`create` 儲存模型、建立已宣告的子關聯，再執行 `afterCreating`。呼叫 `count` 後回傳 Eloquent 集合。工廠屬性略過大量賦值篩選，並繼續執行模型修改器與屬性轉換。

用 `prependState` 將轉換放到目前所有已登記狀態之前。每個轉換都能讀取先前產生的屬性。用 `set(name, value)` 在末尾附加單一屬性覆寫。兩種方法皆可在 `count` 前後使用。直接傳入建立方法的屬性會在已登記狀態之後套用。

批次工廠先建構所有模型，再執行 `afterMaking`。`create` 逐一儲存模型及其宣告的關聯，寫入完成後才執行該批次的 `afterCreating`。`afterMaking` 失敗時，該批模型尚未寫入，建構期間解析的上層工廠可能已寫入記錄。

使用 `raw()` 或 `rawOn(connection)` 取得套用模型修改器和轉換前的展開屬性。指定數量時回傳屬性映射清單。原始屬性展開會略過目標工廠的回呼和子關聯，仍可能儲存宣告的上層模型以解析關聯鍵。使用 `createQuietly()` 或 `createQuietlyOn(connection)` 可儲存模型及關聯，同時抑制 Eloquent 模型事件。工廠回呼照常執行。

用 `rawWith(attributes)` 或 `rawWithState(callback)` 追加僅本次生效的狀態。閉包接收每筆記錄在解析關聯鍵前的屬性。覆寫外鍵時會略過該鍵的解析，包括明確設為 null。對應的 `On` 方法可指定連線。

單筆和批次建構器皆可用 `rawWithParent(parent, callback)` 或 `rawWithParentOn(connection, parent, callback)` 借用父模型。型別相符的 `stateWithParent` 與本次閉包都可讀取父模型。傳入父模型本身不會新增關聯屬性。

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

用 `makeWithState`、`createWithState` 或 `createQuietlyWithState` 為這次操作附加末尾狀態。計數工廠也可以用 `makeWith`、`createWith` 或 `createQuietlyWith` 接收屬性映射。單筆模型的映射入口為 `makeOne`、`createOne` 和 `createOneQuietly`。對應的 `On` 方法可以指定連線。

回呼會逐筆讀取已累積的屬性，此時尚未解析延遲屬性和父模型鍵。末尾狀態只作用於這次呼叫。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, factories::Factory}};

async fn seed_with_final_state<M: Model>() -> DatabaseResult<()> {
    let name = |value: &str| Attributes::from([("name".into(), value.into())]);
    let batch = Factory::<M>::new(move || name("Flight")).count(2);

    let drafts = batch.makeWith(name("Preview")).await?;
    assert!(drafts.iter().all(|model| !model.state().exists()));

    let saved = batch.createWithState(|attributes| {
        assert_eq!(attributes["name"], "Flight".into());
        name("Scheduled")
    }).await?;
    assert_eq!(saved.len(), 2);

    let seed = batch.lazy(name("Evening flight"));
    assert_eq!(seed().await?.len(), 2);
    assert_eq!(seed().await?.len(), 2);
    Ok(())
}
```

用 `createOne(attributes)` 建立單一模型，即使工廠已指定批次數量。用 `createMany(records)` 逐筆依序建立。傳入 `FactoryRecords::Default` 使用工廠數量，未指定時建立一筆。傳入 `FactoryRecords::Count(n)` 指定數量，或用 `FactoryRecords::Attributes(vec![…])` 為每筆記錄指定覆寫屬性。覆寫值在工廠狀態之後套用。每筆記錄完成兩個工廠回呼後才開始下一筆。後續記錄失敗時，先前寫入可能保留。用 `createOneQuietly` 和 `createManyQuietly` 抑制模型事件，或用對應的 `On` 方法指定連線。

用 `makeOne(attributes)` 建構一個未儲存的模型，或用 `makeMany(records)` 依序建構多個未儲存的模型。兩者皆接受明確屬性並執行 `afterMaking`。`makeMany` 使用相同的 `FactoryRecords` 選項，數量為零時回傳空集合。`makeMany` 和 `createMany` 都會消耗已設定的預設數量。明確指定記錄、零筆及模型產生失敗時同樣生效。後續呼叫預設處理一筆記錄，Rust 批次介面仍回傳集合形式。兩種方法皆不儲存回傳的模型，解析已宣告的上層關聯時仍可能寫入上層模型。對應的 `On` 方法可指定連線。`lazy(attributes)` 回傳非同步回呼，呼叫並等待後才解析連線並建立單一模型或指定數量的批次。重複呼叫會重新建立。

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

`makeOne` 保留已設定的批次數量，供下一次 `makeMany` 使用。範例中的兩次預設批次呼叫依序回傳兩個模型和一個模型。預先建構消耗數量後，用獨立的批次工廠設定 `lazy` 的寫入數量。建立或呼叫回呼後，目標資料表在等待回傳的 Future 前沒有寫入。以下每次呼叫並等待都會寫入兩個新模型。未設定批次數量的工廠回傳的 `lazy` 回呼，每次成功執行會建立並回傳一個模型。範例也驗證逐筆屬性覆寫，並等待單模型回呼。每次呼叫並等待都會重新建立。

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
    assert_eq!(custom.len(), 2);
    assert!(custom.iter().all(|model| !model.state().exists()));
    assert_eq!(custom[0].getAttribute("name"), Some(&"Morning flight".into()));
    assert_eq!(custom[1].getAttribute("name"), Some(&"Evening flight".into()));

    let batch = factory().count(2);
    let seed = batch.lazy(Attributes::new());
    let pending = seed();
    assert_eq!(pending.await?.len(), 2);
    assert_eq!(seed().await?.len(), 2);

    let single = factory();
    let seed_one = single.lazy(Attributes::new());
    let pending_one = seed_one();
    assert!(pending_one.await?.state().exists());
    assert!(seed_one().await?.state().exists());
    Ok(())
}
```

工廠在建構模型時暫停預設的關聯自動載入，批次結果維持關閉，包括 `makeMany` 和 `createMany`。帶計數的 `make` 和 `create` 仍使用模型定義的集合鍵。若要明確啟用該批次的自動載入，可呼叫 `withRelationshipAutoloading()`。後續查詢仍遵循模型設定的預設行為。

```rust
use openrust::database::eloquent::{Collection, Model};

fn factory_relationship_loading_sample<M: Model>(models: Collection<M>) -> Collection<M> {
    models.withRelationshipAutoloading()
}
```

用 `forEachSequence(states)` 為每個模型套用一組屬性，並依屬性組數決定批次數量。`crossJoinSequence(sequences)` 依宣告順序組合各序列，最後一個序列變化最快，同名屬性以後面的序列為準。另行設定批次數量，超過組合數時從頭循環。傳入空序列會回傳錯誤。

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

在單一模型或批次工廠上使用 `insert(attributes)` 批次寫入產生的列，或用 `insertOn(connection, attributes)` 指定連線。工廠會執行 `afterMaking` 並解析所需的上層鍵。批次寫入略過目標模型的持久化事件、子關聯建立及 `afterCreating`。寫入前，工廠透過讀取存取器和序列化轉換投影屬性，仍依可見清單篩選，包含符合條件的隱藏欄位並略過追加欄位。插入建構器接著重新套用修改器與寫入轉換。Rust 投影保留資料庫值型別，包含二進位資料。

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, factories::Factory}};

async fn insert_projected<M: Model>(factory: Factory<M>) -> DatabaseResult<()> {
    factory.count(2).insert(Attributes::new()).await
}
```

單筆和批次工廠皆可用 `makeWithParent(parent, attributes)` 和 `insertWithParent(parent, attributes)` 將父模型交給型別相符的 `stateWithParent` 閉包。對應的 `On` 方法可指定連線。父模型作為狀態上下文傳入，需要建立關聯時，由狀態或呼叫屬性指定外鍵。

單筆或批次工廠可以呼叫 `createWithParent(parent, attributes)`、`createQuietlyWithParent(parent, attributes)` 或 `lazyWithParent(parent, attributes)`。型別相符的 `stateWithParent` 和 `afterCreatingWithParent` 可以讀取借用的父模型。立即建立的 `On` 變體可指定連線，延遲回呼在等待時解析連線。父模型本身不會新增外鍵。

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

在單一模型或批次工廠上使用 `withoutAfterMaking()` 或 `withoutAfterCreating()`，清除該工廠已登記的對應回呼。之後可以重新登記回呼。另一階段的回呼及 Eloquent 模型事件照常執行。

在單一模型或批次工廠上使用 `connection(Some("archive"))` 指定命名連線。`getConnectionName()` 回傳工廠設定的覆寫值，`connection(None)` 清除該值。執行時依序查找工廠覆寫、模型連線和目前應用或 Seeder 的預設連線。`On` 方法對目標工廠使用明確傳入的連線。巢狀工廠優先使用自身覆寫或模型連線，再繼承外層連線。關聯連線驗證照常執行。

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

### 延後計算的工廠屬性 {#deferred-factory-attributes}

用 `attribute` 在套用工廠狀態與明確覆寫後計算單一屬性。依相依順序登記屬性。用 `parentAttributeWith` 根據目前屬性建構關聯工廠。同名的明確值優先，包括設為 null 的外鍵。上層關聯排除規則也適用於 `parentAttributeWith`。

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

用 `modelAttribute("owner_id", &owner)` 將既有模型的主鍵用作工廠屬性。沒有主鍵的模型會產生 null，明確指定的屬性優先。建立時，工廠在儲存模型之後、建立子關聯之前清除快取中的空關聯集合。非空集合與單值關聯會保留，包括已快取的 null。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn from_owner<M: Model, R: Model>(owner: &R) -> Factory<M> {
    Factory::new(Attributes::new).modelAttribute("owner_id", owner)
}
```

## 工廠關聯

設定數量後仍可繼續設定狀態、序列、回呼和關聯。`count(n)` 取代待處理數量，`forEachSequence(states)` 將數量設為序列長度。其他設定方法保留待處理數量。 直接向 `has` 或 `hasNamed` 傳入工廠時，每個父模型建立一筆子記錄。若子工廠已設定數量，會採用該數量，零筆也有效。 單筆子記錄會執行模型建構回呼，跳過批次集合鉤子。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory, relations::HasMany},
    seeder::SeederContext,
};

async fn seed_counted_relationships<P: Model, C: Model>(
    context: &SeederContext<'_>,
    parents: Factory<P>,
    children: Factory<C>,
    relation: HasMany<P, C>,
) -> DatabaseResult<()> {
    parents
        .count(2)
        .has(relation, children)
        .sequence(|index| Attributes::from([("name".into(), format!("Batch {index}").into())]))
        .afterCreating(|parent| {
            assert!(parent.state().exists());
            Ok(())
        })
        .createOn(context.connection())
        .await?;
    Ok(())
}
```


### 狀態與回呼的父模型 {#factory-parent-context}

當子工廠需要讀取透過 `has` 或 `hasAttached` 建立它的模型時，使用 `stateWithParent::<Parent>` 和 `afterCreatingWithParent::<Parent>`。型別相符時回呼接收 `Some(&Parent)`，獨立建立或父模型型別不同時接收 `None`。`afterMaking` 建構回呼仍只接收子模型。

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

`has` 接受 `HasOne`、`HasMany`、`MorphOne` 或 `MorphMany`。用 `r#for` 建立 `BelongsTo` 或 `MorphTo` 的上層模型，用 `forExisting` 關聯既有模型。每個 `for` 宣告依子模型與上層模型的資料庫連線組合解析一次所需的上層模型。同一建構器重複執行時重用該模型，新建構器獨立解析。`MorphTo` 描述元需要登錄目標模型與對應別名。`hasAttached` 接受 `BelongsToMany` 或 `MorphToMany` 並寫入中介表屬性。寫入依序執行。後續關聯失敗時，先前的寫入可能保留。

對 `BelongsTo` 和 `MorphTo`，同一外鍵以最後一次 `r#for` 或 `forExisting` 宣告覆寫工廠定義。之後可以用狀態或操作參數覆寫外鍵，包括傳入 `Value::Null`。此時不會建立或關聯上層模型。`MorphTo` 仍單獨提供類型預設值，狀態也可以覆寫類型。

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

子工廠先讀取工廠定義，再套用關聯查詢的預設屬性和明確狀態。直接及多型子關聯隨後補入外鍵和類型欄位，再執行 `afterMaking`。附加模型工廠也會在套用明確狀態前讀取關聯查詢的預設屬性。

用 `hasNamed`、`forNamed` 或 `hasAttachedNamed` 按名稱解析關聯。工廠使用 `Model::namedRelations` 或應用程式 `resolveRelationUsing` 登錄的描述元。明確傳入 Rust 描述元型別，例如 `forNamed::<_, MorphTo<Comment>>("subject", posts)`。名稱不存在或型別不符時回傳錯誤。 使用 `forExistingNamed` 透過已註冊的關聯名稱關聯既有父模型。

當關聯遵循相關 Rust 模型的命名時，可用 `hasInferred`。它先查找小駝峰複數關聯，模型未註冊該複數名稱時回退至單數。`hasAttachedInferred` 使用複數，`forInferred` 和 `forExistingInferred` 使用父模型的小駝峰名稱。關聯描述符仍明確指定。自訂關聯名稱使用對應的命名方法。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, factories::Factory, relations::HasMany},
    seeder::SeederContext,
};

async fn seed_inferred_children<P: Model, C: Model>(
    context: &SeederContext<'_>,
    parents: Factory<P>,
    children: Factory<C>,
) -> DatabaseResult<()> {
    parents
        .count(2)
        .hasInferred::<_, HasMany<P, C>>(children)
        .createOn(context.connection())
        .await?;
    Ok(())
}
```

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, factories::Factory, relations::BelongsTo},
    seeder::SeederContext,
};

async fn seed_existing_owner<M: Model, R: Model>(
    context: &SeederContext<'_>,
    factory: Factory<M>,
    relation: &str,
    owner: R,
) -> DatabaseResult<M> {
    factory
        .forExistingNamed::<_, BelongsTo<M, R>>(relation, owner)
        .createOn(context.connection())
        .await
}
```

用 `parentAttribute("owner_id", owners)` 宣告由另一工廠產生的外鍵屬性。它會展開為已儲存模型的鍵，也可重用相符的模型。定義、狀態或呼叫傳入的明確值優先。`withoutParents(vec![])` 停用所有工廠外鍵屬性。傳入 `FactoryParentSelector::attribute("owner_id")` 或 `FactoryParentSelector::model::<Owner>()` 可依屬性名稱或模型型別停用。停用的屬性展開為 null，明確的 `for` 和 `forExisting` 關聯仍會解析。

程序預設開啟展開。`Factory::<Owner>::dontExpandRelationshipsByDefault()` 會停用之後建構的工廠的展開，適用於所有模型型別。`expandRelationshipsByDefault()` 或 `flushState()` 可恢復預設值。既有工廠保留建構時的設定。完全停用後，再呼叫帶選擇器的 `withoutParents` 仍維持完全停用。

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn factory_without_parent_expansion<Owner: Model>() -> Factory<Owner> {
    Factory::<Owner>::dontExpandRelationshipsByDefault();
    let factory = Factory::<Owner>::new(Attributes::new);
    Factory::<Owner>::expandRelationshipsByDefault();
    factory
}
```

`hasAttachedNamed` 成功寫入後，如果該命名關聯已載入，或中間表變更會觸碰關聯模型，工廠會重新載入該關聯。`afterCreating` 可以讀取更新後的集合。設定自訂 Pivot 時，集合中的關聯模型會帶有對應的 Pivot 模型。

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

### 重用既有模型 {#recycling-existing-models}

用 `recycle` 傳入一個已儲存的模型，或用 `recycleMany` 傳入同類型模型的迭代器。這兩個方法可在 `count` 前後呼叫。模型池會傳給巢狀關聯工廠。解析上層模型時，工廠在同一連線和類型的候選模型中隨機選取一個，缺少符合項目時建立新上層模型。

單筆或批次建構器皆可透過 `getRandomRecycledModel::<Owner>()` 依模型型別查詢重用池，無需存取資料庫。回傳模型借用或 `None`。關聯解析仍依上層模型的連線篩選候選。

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

向 `hasAttached` 或 `hasAttachedNamed` 傳入屬性映射清單，即可依關聯模型順序設定中介資料表屬性。關聯工廠數量超過清單長度時會循環。用 `FactoryPivot::resolve` 根據已儲存的父模型計算每次附加的屬性。單一屬性映射會套用到所有關聯。 中介資料表序列也會在同一建構器的不同父模型和重複呼叫之間接續。

傳入非空中介資料表屬性序列時，未指定數量的關聯工廠依序列長度產生模型。明確的 `count(n)` 優先，包括零。共用屬性和空序列預設產生一個關聯模型。

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

可傳入既有模型、模型清單或 Eloquent 集合，直接附加這些模型。模型必須已持久化、具有關聯所需的鍵，並屬於該關聯使用的連線。目標無效時，會在寫入這次附加的中介資料表記錄前回傳錯誤。
