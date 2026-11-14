# Eloquent 工廠

## 定義模型工廠

用 `Factory::new` 定義模型的預設屬性。若希望透過 `Flight::factory()` 呼叫，在模型上實作 `HasFactory`。執行下方指令在 `database/factories` 產生檔案，再宣告 Rust 模組並填寫屬性定義。

```sh
./artisan make:factory FlightFactory --model=Flight
```

### 本地化測試資料 {#localized-fake-data}

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

用 `dateTimeInInterval(date, interval)` 在 UTC 起點和該時間加上帶正負號的 Chrono `TimeDelta` 之間按整秒取樣。負時長向過去取樣。起點和時長傳入 `None` 時，分別預設為 30 年前和往後五天。`dateTimeThisCentury(max)`、`dateTimeThisDecade(max)` 和 `dateTimeThisMonth(max)` 分別從目前時間往前 100 年、10 年和一個月開始。`dateTimeThisYear(max)` 從應用程式時區當年 1 月 1 日零點開始。其他日曆回溯和 `dateTimeInInterval` 的預設起點也採用應用程式時區。兩個區間參數都省略時，預設五天依本地日曆計算。明確傳入的 `TimeDelta` 表示實際經過的時長。這四個方法在 `max` 為 `None` 時以目前時間為終點，拒絕早於起點的終點，日曆溢出依 FakerPHP 規則正規化。五個方法皆支援 `unique`。Rust 使用具型別的日期和時長。需要自訂日曆區間時，可計算終點後呼叫 `dateTimeBetween`。

用 `amPm(max)`、`dayOfMonth(max)`、`dayOfWeek(max)`、`month(max)`、`monthName(max)` 和 `year(max)` 依實際使用的時區格式化隨機日期的各個部分。日和月的數字以補零字串回傳，星期和月份名稱沿用 FakerPHP 的英文日期格式。`dateTimeAD(max)` 從 FakerPHP 的 64 位元公元日期下界開始取樣。這些方法在 `max` 為 `None` 時使用目前時間。`century()` 隨機回傳 I 至 XXI 的羅馬數字。所有方法皆支援 `unique`。

用 `timezone(countryCode)` 隨機取得 IANA 時區識別碼。傳入 `None` 從完整清單取樣，也可傳入 `Some("TW")` 等國家或地區代碼來限定範圍。代碼區分大小寫，未知代碼回傳 `None`。框架內建與 `chrono-tz` 同版的 IANA 資料。此方法支援 `unique`。回傳的識別碼可用於將 UTC 日期轉換到目標時區。

日期字串優先使用 `setDefaultTimezone(Some(zone))` 為目前產生器指定的時區，其次讀取應用程式或工廠連線所屬應用程式的 `app.timezone`，預設為 UTC。傳入 `None` 可清除覆寫，`getDefaultTimezone()` 可查看覆寫值。`dateTimeInTimezone(max, timezone)` 回傳帶時區的日期時間，傳入 `Some(zone)` 可覆寫本次呼叫。日期時間方法仍回傳 UTC 時刻。產生器的時區覆寫用於輸出格式化，日曆邊界繼續依應用程式時區計算。

用 `dateTimeBetweenExpressions(start, end)` 傳入 PHP 風格的日期表達式，例如 `"-30 years"` 和 `"now"`。兩個端點共用同一參考時刻，並依應用程式時區解析。表達式中的明確時區優先。數字終點字串依 Unix 時間戳解析，`"@-1"` 則是明確的時間戳表達式。端點解析失敗時依 FakerPHP 的數值轉換規則使用 Unix 零秒。起點晚於終點仍回傳錯誤。此方法與 `dateTimeBetween` 共用 `unique` 歷史。

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

用 `words(count)`、`sentences(count)` 和 `paragraphs(count)` 產生指定數量的清單。對應的 `AsText` 方法用空格連接單字或句子，用空行分隔段落。`sentenceWithWordCount(count, variable)` 和 `paragraph(count, variable)` 支援固定數量或依 FakerPHP 規則浮動。原有依範圍取樣的 `sentence(words)` 也可以繼續使用。`text(max)` 以 UTF-8 位元組為上限。上限不足五個位元組或無法容納產生項目時回傳錯誤。這些方法皆支援 `unique`。詞庫由 fake-rs 依語言提供。

### 預留字元格式化 {#pattern-formatters}

用 `numerify(Some(pattern))` 將 `#` 替換為數字、將 `%` 替換為非零數字。`lexify` 將 `?` 替換為 ASCII 小寫字母。`bothify` 處理這三種預留字元，並以相同機率將每個 `*` 替換為數字或小寫字母。`asciify` 將 `*` 替換為 `!` 至 `~` 的可列印 ASCII 字元。其餘字元照原樣保留。傳入 `None` 使用 FakerPHP 預設範本。`randomLetter()` 和 `randomAscii()` 回傳單一 Rust 字元。六個方法皆支援 `unique`。

用 `regexify(pattern)?` 從基本正規表示式產生樣本。支援可選的斜線分隔符號和外層錨點、字面值、ASCII 字元類別和範圍、`\d`、`\w`、點號、一般群組、分支和有限重複。與 FakerPHP 相同，`*` 和 `+` 分別為目前範本選取一至九的重複上限。語法錯誤、環視和反向參照等未支援功能，以及超出產生限制的模式均回傳錯誤。`unique(false).regexify(pattern)?` 可對結果去重。

### 集合取樣 {#collection-sampling}

用 `randomElement(values)` 借用一個元素，空切片回傳 `None`。`randomKey(values)` 回傳從零開始的位置。若要從映射鍵或列舉變體中選取，將鍵或變體的切片傳給 `randomElement`。`randomElements(values, count, allowDuplicates)?` 接受 `Some(n)` 固定抽取數量，或用 `None` 在一到切片長度之間隨機選取數量。不放回時每個位置最多抽一次，不同位置上的相同值仍可同時出現。零筆回傳空向量。非零取樣要求切片非空，不放回取樣的數量不能超過切片長度。

用 `shuffleArray(values)` 回傳打亂後的副本，或用 `shuffleString(value)` 依 Unicode 純量值打亂字串。`boolean(None)` 有 50% 的機率回傳 true，傳入 `Some(75)` 可設為 75%。這些方法均支援 `unique`，泛型取樣與陣列打亂依 JSON 序列化結果判斷重複。

### 語言、國家和貨幣樣本 {#locale-and-code-samples}

用 `languageCode()`、`countryCode()`、`countryISOAlpha3()`、`currencyCode()` 和 `emoji()` 從固定 FakerPHP 目錄產生測試值。`randomLocale()` 抽取 `en_GB`、`zh_TW` 等語言地區識別碼，對應 FakerPHP 的 `locale()` 產生器。OpenRust 既有的 `locale()` 讀取目前產生器的設定。樣本目錄包含歷史識別碼。六個產生器均支援 `unique`。

### 摘要樣本 {#digest-samples}

用 `md5()`、`sha1()` 和 `sha256()` 產生 32、40 和 64 個字元的小寫十六進位樣本。每個產生器遵循 FakerPHP 的做法，從 0 到 2,147,483,647 抽取整數，再對其十進位文字計算摘要。三個方法均支援 `unique`，供測試和資料填充使用。

### 網路位址樣本 {#network-address-samples}

用 `ipv4()`、`localIpv4()`、`ipv6()` 和 `macAddress()` 產生網路測試資料。`ipv4()` 在 `1.0.0.0`–`127.255.255.255` 和 `128.0.0.0`–`255.255.255.254` 兩段中以相同機率選一段，再抽取位址，可能回傳私有或特殊用途位址。`localIpv4()` 在 `10/8`、`172.16/12` 和 `192.168/16` 中以相同機率選一段。IPv6 回傳八組未壓縮的小寫十六進位數，各組省略前導零。MAC 位址由六組以冒號分隔的兩位大寫十六進位數組成。四個方法均支援 `unique`。

### 網域名稱、URL 和 slug {#domain-url-slug-samples}

用 `domainWord()?` 將產生的姓氏轉寫為小寫 ASCII，用 `tld()` 依支援語言地區的 FakerPHP 權重抽取後綴，`domainName()?` 將兩者連接。`url()?` 使用上游 HTTP 和 HTTPS 範本，可帶 `www`、slug 和 `.html`。姓氏使用 OpenRust 的語言資料和轉寫規則，與 FakerPHP 的 ICU 或備援映射可能不同。姓氏無法轉寫出可用字元時，網域名稱和 URL 產生器回傳錯誤。`slug(count, variable)` 用連字號拼接產生的詞。傳入 `None` 使用六個基準詞、`Some(0)` 回傳空字串，`false` 固定詞數。五個產生器均支援 `unique`。

### 電子郵件和密碼樣本 {#email-and-password-samples}

用 `email()?` 產生一般電子郵件地址，或用 `companyEmail()?` 產生使用隨機網域的地址。簡體中文的一般電子郵件使用該地區的免費信箱清單。`safeEmail()` 使用保留的範例網域，`freeEmail()` 使用地區信箱服務網域。只需網域時用 `safeEmailDomain()` 或 `freeEmailDomain()`。使用者名稱使用地區姓名或詞庫並轉寫為 ASCII。若無可用字元，OpenRust 產生一個 ASCII 字母，保留既有使用者名稱及電子郵件方法的直接回傳型別。`password(min, max)?` 使用從 `!` 到 `~` 的可列印 ASCII 字元，長度含兩端，接受反向邊界，預設值為 6 和 20。以上產生器均支援 `unique`。

### 電話號碼和 IMEI 樣本 {#phone-and-imei-samples}

用 `phoneNumber()` 產生地區顯示號碼。簡體中文和繁體中文使用固定 FakerPHP 範本，其他地區沿用既有 fake-rs 格式。`e164PhoneNumber()` 使用固定的國際號碼範本及地區覆寫，回傳以加號開頭的數字字串。`imei()` 回傳 15 位數字，含 Luhn 檢查碼。三個方法均支援 `unique`。

### 郵寄地址和稱謂 {#address-and-title-samples}

用 `streetAddress()` 產生街道行，或用 `address()` 產生完整地址樣本。美國、法國、簡體中文和繁體中文使用各自的 FakerPHP 排列規則。城市、街道和部分地址元件仍使用 fake-rs 資料。個別欄位可用 `buildingNumber()`、`secondaryAddress()`、`postcode()`、`state()` 和 `stateAbbr()`。`title(Some("male"))` 和 `title(Some("female"))` 指定稱謂組，`title(None)` 在兩組中隨機選取，也可直接呼叫 `titleMale()` 和 `titleFemale()`。這些方法均支援 `unique`。

### 依性別產生名字 {#given-names-by-gender}

用 `firstNameMale()` 或 `firstNameFemale()` 從對應的 FakerPHP 名字清單取樣。`firstNameWithGender(Some("male"))` 和 `nameWithGender(Some("female"))` 明確選擇分組。傳入 `None` 或未識別的分組會從兩組取樣。全名依地區範本組合現有姓氏資料，繁體中文名字由一個或兩個抽取字元組成。缺少地區清單時使用英文名字和範本。既有無參數 `name()` 和 `firstName()` 沿用原本的資料來源，並與具分組的方式共用各自的 `unique` 歷史。

### 座標 {#coordinate-samples}

用 `latitude(None, None)?` 和 `longitude(None, None)?` 產生帶正負號的十進位度數，四捨五入至小數點後六位。任一邊界可傳入 `Some(value)`，預設範圍分別為 −90 到 90 和 −180 到 180。接受反向邊界，非有限邊界回傳錯誤。`localCoordinates()?` 回傳含 `latitude` 和 `longitude` 欄位的可序列化 `Coordinates`。匈牙利和蒙特內哥羅採用固定上游地區範圍，其他地區採用全球範圍。臺灣範圍在 `zh_TW` 下由獨立的 `localLatitude()?` 和 `localLongitude()?` 提供。五個方法均支援 `unique`。

## 使用工廠建立模型

`state` 按宣告順序疊加。`sequence` 接收批次內從零開始的索引。`make` 建立記憶體模型並執行 `afterMaking`。`create` 儲存模型、建立已宣告的子關聯，再執行 `afterCreating`。呼叫 `count` 後回傳 Eloquent 集合。工廠屬性略過大量賦值篩選，並繼續執行模型修改器與屬性轉換。

用 `prependState` 將轉換放到目前所有已登記狀態之前。每個轉換都能讀取先前產生的屬性。用 `set(name, value)` 在末尾附加單一屬性覆寫。兩種方法皆可在 `count` 前後使用。直接傳入建立方法的屬性會在已登記狀態之後套用。

批次工廠先建構所有模型，再執行 `afterMaking`。`create` 逐一儲存模型及其宣告的關聯，寫入完成後才執行該批次的 `afterCreating`。`afterMaking` 失敗時，該批模型尚未寫入，建構期間解析的上層工廠可能已寫入記錄。

使用 `raw()` 或 `rawOn(connection)` 取得套用模型修改器和轉換前的展開屬性。指定數量時回傳屬性映射清單。原始屬性展開會略過目標工廠的回呼和子關聯，仍可能儲存宣告的上層模型以解析關聯鍵。使用 `createQuietly()` 或 `createQuietlyOn(connection)` 可儲存模型及關聯，同時抑制 Eloquent 模型事件。工廠回呼照常執行。

用 `createOne(attributes)` 建立單一模型，即使工廠已指定批次數量。用 `createMany(records)` 逐筆依序建立。傳入 `FactoryRecords::Default` 使用工廠數量，未指定時建立一筆。傳入 `FactoryRecords::Count(n)` 指定數量，或用 `FactoryRecords::Attributes(vec![…])` 為每筆記錄指定覆寫屬性。覆寫值在工廠狀態之後套用。每筆記錄完成兩個工廠回呼後才開始下一筆。後續記錄失敗時，先前寫入可能保留。用 `createOneQuietly` 和 `createManyQuietly` 抑制模型事件，或用對應的 `On` 方法指定連線。

用 `makeOne(attributes)` 建構一個未儲存的模型，或用 `makeMany(records)` 依序建構多個未儲存的模型。兩者皆接受明確屬性並執行 `afterMaking`。`makeMany` 使用相同的 `FactoryRecords` 選項，呼叫後保留已設定數量，數量為零時回傳空集合。兩種方法皆不儲存回傳的模型，解析已宣告的上層關聯時仍可能寫入上層模型。對應的 `On` 方法可指定連線。`lazy(attributes)` 回傳非同步回呼，呼叫並等待後才解析連線並建立單一模型或指定數量的批次。重複呼叫會重新建立。

以下示範如何呼叫 `lazy` 回傳的回呼。建立回呼時目標資料表沒有寫入，每次呼叫並等待都會寫入一批新模型。

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

用 `forEachSequence(states)` 為每個模型套用一組屬性，並依屬性組數決定批次數量。`crossJoinSequence(sequences)` 依宣告順序組合各序列，最後一個序列變化最快，同名屬性以後面的序列為準。另行設定批次數量，超過組合數時從頭循環。傳入空序列會回傳錯誤。

在單一模型或批次工廠上使用 `insert(attributes)` 批次寫入產生的列，或用 `insertOn(connection, attributes)` 指定連線。工廠會執行 `afterMaking` 並解析所需的上層鍵。批次寫入略過目標模型的持久化事件、子關聯建立及 `afterCreating`。寫入使用儲存屬性，包含隱藏欄位，略過追加欄位和輸出存取器。

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

## 工廠關聯

`has` 接受 `HasOne`、`HasMany`、`MorphOne` 或 `MorphMany`。用 `r#for` 建立 `BelongsTo` 或 `MorphTo` 的上層模型，用 `forExisting` 關聯既有模型。每次批次呼叫只解析一次上層模型，`make` 也遵循這項規則。再次呼叫會產生新的上層模型。`MorphTo` 描述元需要登錄目標模型與對應別名。`hasAttached` 接受 `BelongsToMany` 或 `MorphToMany` 並寫入中介表屬性。寫入依序執行。後續關聯失敗時，先前的寫入可能保留。

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

### 重用既有模型 {#recycling-existing-models}

用 `recycle` 傳入一個已儲存的模型，或用 `recycleMany` 傳入同類型模型的迭代器。這兩個方法可在 `count` 前後呼叫。模型池會傳給巢狀關聯工廠。解析上層模型時，工廠在同一連線和類型的候選模型中隨機選取一個，缺少符合項目時建立新上層模型。同一批次重用已選中的上層模型。

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

向 `hasAttached` 或 `hasAttachedNamed` 傳入屬性映射清單，即可依關聯模型順序設定中介資料表屬性。關聯工廠數量超過清單長度時會循環。用 `FactoryPivot::resolve` 根據已儲存的父模型計算每次附加的屬性。單一屬性映射會套用到所有關聯。

可傳入既有模型、模型清單或 Eloquent 集合，直接附加這些模型。模型必須已持久化、具有關聯所需的鍵，並屬於該關聯使用的連線。目標無效時，會在寫入這次附加的中介資料表記錄前回傳錯誤。
