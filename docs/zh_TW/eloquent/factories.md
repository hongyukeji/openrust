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

下文列出已支援的產生器。FakerPHP 的檔案、圖片產生和 User-Agent 等產生器仍待接入。

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


用 `faker.unique(false)` 讓同一個產生器按方法去重，用 `unique(true)` 清空歷史。`Factory::tryWithFaker` 的同一批次共用產生器，重試耗盡會在該批模型寫入前回傳錯誤。各次獨立呼叫擁有自己的歷史。例如，可在工廠定義中使用 `faker.unique(false).safeEmail()?`。

內建產生方法還包括 `firstName`、`lastName`、`freeEmail`、`userName`、`streetName`、`postcode`、`phoneNumber`、`company` 和 `word`。這些方法也可透過 `unique` 呼叫。樣本取自 fake-rs 中對應語言的資料集。

用 `numberBetween(first, second)` 產生整數，包含兩個端點，端點可以任意排序。`randomDigit()` 回傳 0 至 9，`randomDigitNotNull()` 回傳 1 至 9。`dateTimeBetween(start, end)` 接收明確的 UTC 日期時間，以整秒取樣並包含兩端，起始秒晚於結束秒時回傳錯誤。四種方法均支援 `unique`。

`randomDigitNot(except)` 排除指定數字。`randomNumber(digits, strict)` 的位數傳入 `None` 時隨機選擇一至九位。嚴格模式產生確切位數，非嚴格模式也接受零位並回傳零。明確位數超過九位或嚴格模式指定零位時回傳錯誤。兩種方法皆支援 `unique`。

`randomFloat(decimals, min, max)` 先在區間取樣，再依指定小數位捨入，中點遠離零。傳入 `None` 可隨機選擇零至九位精度或產生上界。上下界可逆序，捨入結果可能超出原區間。非有限邊界回傳錯誤。與 PHP 8.5 相同，正精度超過 308 或捨入後的數值溢位時保留取樣值。精度小於 −308 時回傳零。`unique` 將正零與負零視為同一個值。

用 `unixTime(max)` 在 Unix 紀元與指定端點之間取樣整秒，或用 `dateTime(max)` 回傳 UTC 日期時間。傳入 `None` 時使用目前時間。`date(format, max)` 和 `time(format, max)` 接收 Chrono 格式，預設分別為 `%Y-%m-%d` 和 `%H:%M:%S`，格式無效時回傳錯誤。`iso8601(max)` 包含實際使用的時區偏移量。五種方法皆支援 `unique`。

用 `dateTimeInInterval(date, interval)` 在 UTC 起點和該時間加上帶正負號的 Chrono `TimeDelta` 之間按整秒取樣。負時長向過去取樣。起點和時長傳入 `None` 時，分別預設為 30 年前和往後五天。`dateTimeThisCentury(max)`、`dateTimeThisDecade(max)` 和 `dateTimeThisMonth(max)` 分別從目前時間往前 100 年、10 年和一個月開始。`dateTimeThisYear(max)` 從應用程式時區當年 1 月 1 日零點開始。其他日曆回溯和 `dateTimeInInterval` 的預設起點也採用應用程式時區。兩個區間參數都省略時，預設五天依本地日曆計算。明確傳入的 `TimeDelta` 表示實際經過的時長。這四個方法在 `max` 為 `None` 時以目前時間為終點，拒絕早於起點的終點，日曆溢出依上游 Faker 規則正規化。五個方法皆支援 `unique`。Rust 使用具型別的日期和時長。需要自訂日曆區間時，可計算終點後呼叫 `dateTimeBetween`。

用 `amPm(max)`、`dayOfMonth(max)`、`dayOfWeek(max)`、`month(max)`、`monthName(max)` 和 `year(max)` 依實際使用的時區格式化隨機日期的各個部分。日和月的數字以補零字串回傳，星期和月份名稱沿用上游 Faker 的英文日期格式。`dateTimeAD(max)` 從上游 Faker 的 64 位元公元日期下界開始取樣。這些方法在 `max` 為 `None` 時使用目前時間。`century()` 隨機回傳 I 至 XXI 的羅馬數字。所有方法皆支援 `unique`。

用 `timezone(countryCode)` 隨機取得 IANA 時區識別碼。傳入 `None` 從完整清單取樣，也可傳入 `Some("TW")` 等國家或地區代碼來限定範圍。代碼區分大小寫，未知代碼回傳 `None`。框架內建與 `chrono-tz` 同版的 IANA 資料。此方法支援 `unique`。回傳的識別碼可用於將 UTC 日期轉換到目標時區。

日期字串優先使用 `setDefaultTimezone(Some(zone))` 為目前產生器指定的時區，其次讀取應用程式或工廠連線所屬應用程式的 `app.timezone`，預設為 UTC。傳入 `None` 可清除覆寫，`getDefaultTimezone()` 可查看覆寫值。`dateTimeInTimezone(max, timezone)` 回傳帶時區的日期時間，傳入 `Some(zone)` 可覆寫本次呼叫。日期時間方法仍回傳 UTC 時刻。產生器的時區覆寫用於輸出格式化，日曆邊界繼續依應用程式時區計算。

用 `dateTimeBetweenExpressions(start, end)` 傳入 PHP 風格的日期表達式，例如 `"-30 years"` 和 `"now"`。兩個端點共用同一參考時刻，並依應用程式時區解析。表達式中的明確時區優先。數字終點字串依 Unix 時間戳解析，`"@-1"` 則是明確的時間戳表達式。端點解析失敗時依上游 Faker 的數值轉換規則使用 Unix 零秒。起點晚於終點仍回傳錯誤。此方法與 `dateTimeBetween` 共用 `unique` 歷史。

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

### 電話號碼和 IMEI 樣本 {#phone-and-imei-samples}

用 `phoneNumber()` 產生地區顯示號碼。簡體中文和繁體中文使用固定上游 Faker 範本，其他地區沿用既有 fake-rs 格式。`e164PhoneNumber()` 使用固定的國際號碼範本及地區覆寫，回傳以加號開頭的數字字串。`imei()` 回傳 15 位數字，含 Luhn 檢查碼。三個方法均支援 `unique`。

### 條碼與 ISBN {#barcode-samples}

使用 `ean8()`、`ean13()`、`isbn10()` 和 `isbn13()` 產生帶有檢查碼的樣本。回傳字串保留前導零。ISBN-10 的末位可以是 `X`，ISBN-13 以 `978` 或 `979` 開頭。各產生器獨立維護 `unique` 歷史。

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn catalog_codes() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en"))?;
    Ok((faker.ean13(), faker.unique(false).isbn13()?))
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

用 `bankAccountNumber(prefix, countryCode, length)` 依 Faker 地區產生帳號。支援 IBAN 別名的地區預設使用本國代碼，可明確覆寫參數。埃及地區固定產生埃及 IBAN。美國英語、馬來西亞馬來語和尼泊爾語產生本地帳號。這四種地區傳入 `("", None, None)`。`bankRoutingNumber()` 為 `en_US` 產生九位路由號範例。`Faker::calculateRoutingNumberChecksum` 讀取前八位並回傳檢查碼。未支援的地區會回傳錯誤。兩種產生方法都支援 `unique`。

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

用 `streetAddress()` 產生街道行，或用 `address()` 產生完整地址樣本。美國、法國、簡體中文和繁體中文使用各自的上游 Faker 排列規則。城市、街道和部分地址元件仍使用 fake-rs 資料。個別欄位可用 `buildingNumber()`、`secondaryAddress()`、`postcode()`、`state()` 和 `stateAbbr()`。`title(Some("male"))` 和 `title(Some("female"))` 指定稱謂組，`title(None)` 在兩組中隨機選取，也可直接呼叫 `titleMale()` 和 `titleFemale()`。這些方法均支援 `unique`。

### 依性別產生名字 {#given-names-by-gender}

用 `firstNameMale()` 或 `firstNameFemale()` 從對應的上游 Faker 名字清單取樣。`firstNameWithGender(Some("male"))` 和 `nameWithGender(Some("female"))` 明確選擇分組。傳入 `None` 或未識別的分組會從兩組取樣。全名依地區範本組合現有姓氏資料，繁體中文名字由一個或兩個抽取字元組成。缺少地區清單時使用英文名字和範本。既有無參數 `name()` 和 `firstName()` 沿用原本的資料來源，並與具分組的方式共用各自的 `unique` 歷史。

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

預先建構消耗數量後，用獨立的批次工廠設定 `lazy` 的寫入數量。建立或呼叫回呼後，目標資料表在等待回傳的 Future 前沒有寫入。以下每次呼叫並等待都會寫入兩個新模型。

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

    let _one = previews.makeOne(Attributes::new()).await?;
    assert_eq!(previews.makeMany(FactoryRecords::Default).await?.len(), 2);
    assert_eq!(previews.makeMany(FactoryRecords::Default).await?.len(), 1);

    let batch = factory().count(2);
    let seed = batch.lazy(Attributes::new());
    assert_eq!(seed().await?.len(), 2);
    Ok(())
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

用 `hasNamed`、`forNamed` 或 `hasAttachedNamed` 按名稱解析關聯。工廠使用 `Model::namedRelations` 或應用程式 `resolveRelationUsing` 登錄的描述元。明確傳入 Rust 描述元型別，例如 `forNamed::<_, MorphTo<Comment>>("subject", posts)`。名稱不存在或型別不符時回傳錯誤。

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
