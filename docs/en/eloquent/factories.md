# Eloquent: Factories

## Defining Model Factories

Use `Factory::new` to define the model’s default attributes. Implement `HasFactory` on the model if you want to call `Flight::factory()`. Generate a factory under `database/factories` with the command below, then declare its Rust module and supply the definition.

See [Using Model Factories in Seeders](../database/seeding.md#using-model-factories) for the application entry point and selected database connection.

```sh
./artisan make:factory FlightFactory --model=Flight
```

### Binding a Model to Its Factory {#factory-model-binding}

The generated factory implements `FactoryDefinition`. Its associated `Model` binds the factory to a model at compile time. Declare the factory in `database/factories/mod.rs` and declare `factories` in `database/mod.rs`. Implement `HasFactory` on the model to delegate `Flight::factory()` to the chosen factory. You can also call `FlightFactory::factory()` directly. The `configure` hook runs on each fresh builder before call-site states, which can override its defaults.

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

Use `FlightFactory::new(attributes)` or `FlightFactory::newWithState(callback)` to register the initial state before `configure`. `FlightFactory::times(2)` starts a configured batch of two. On either builder, `modelName()` returns the bound Rust type name. `newModel(attributes)` constructs an unsaved model using its normal mass-assignment rules, without evaluating the factory definition or states.
Use `Factory::<Flight>::factoryForModel()` to obtain the factory bound by the model’s `HasFactory` implementation. The binding is explicit and uses the compiled model type.

For a nested factory such as `Fleet/FlightFactory`, Artisan infers the model in the corresponding `app::models::fleet` module. Pass `--model` or `-m` to choose another model. Declare the generated modules before compiling. The compiled association remains explicit through `FactoryDefinition::Model` and `HasFactory`.

Declare the generated modules before compiling. Make the model modules visible within the crate when a generated factory imports them, for example with `pub(crate) mod fleet;` and `pub(crate) mod flight;` in the respective module files. Write the `HasFactory` implementation alongside the generated factory if its module is private to `database`.

### Factory Extensions {#factory-extensions}

Define an extension trait for reusable factory methods and import it where you build the factory. Rust resolves the method at compile time. In this example, `scheduled()` adds a state before `count(2)`, and the model chooses its factory through `HasFactory`. The table needs a `status` column.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, factories::{Factory, HasFactory}},
};

trait FlightFactoryExt {
    fn scheduled(self) -> Self;
}

impl<M: Model> FlightFactoryExt for Factory<M> {
    fn scheduled(self) -> Self {
        self.set("status", "scheduled")
    }
}

async fn seed_scheduled<M: HasFactory>() -> DatabaseResult<()> {
    M::factory().scheduled().count(2).create().await?;
    Ok(())
}
```

### Localized Fake Data {#localized-fake-data}

The catalog below lists the supported formatters.

The [upstream Faker catalog](https://github.com/FakerPHP/Faker/tree/v1.24.1) is the reference for the formats described below.

Use `Factory::withFaker` to define generated model attributes. It reads `app.faker_locale` from the application owning the selected connection. Use `fake(Some("fr_FR"))` to select a locale explicitly, or `fake(None)` to use the current application. The Rust generator uses fake-rs data and exposes `name`, `safeEmail`, `city`, and `sentence`. Unsupported locales use English data.

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


Within an application, `fake()` returns an `Arc<Faker>` shared by calls with the same normalized locale. Its `unique(false)` history and timezone override carry across those calls. `unique(true)` clears the shared history. Factories use a separate generator owned by the selected connection’s application, and batches and subsequent factory calls on that application share its history. If a formatter exhausts its values while constructing a batch, the batch returns an error before saving those models. Outside an application, `fake()` creates a fresh generator. Use `Faker::new` for an independent generator.

Use `seed(2026)` to restart the random sequence on a generator. Reuse the locale, formatter order, and arguments to replay the same sequence. Supply fixed bounds to date formatters that otherwise use the current time. In concurrent code, give each task its own `Faker::new` if the sequence must be independent of task scheduling.

Pass `None` to reseed from fresh entropy. Seeded values use Rust’s random generators. Seeding leaves uniqueness history intact, so use `unique(true)` separately when the history also needs to restart.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::Faker};

fn seeded_names() -> DatabaseResult<Vec<String>> {
    let faker = Faker::new("en-US")?;
    faker.seed(2026);
    Ok((0..3).map(|_| faker.name()).collect())
}
```
Other built-in formatters include `firstName`, `lastName`, `freeEmail`, `userName`, `streetName`, `postcode`, `phoneNumber`, `company`, and `word`. The same methods are available through `unique`. The selected locale supplies sample data where fake-rs has it.

Use `numberBetween(first, second)` for an inclusive integer range in either order. `randomDigit()` returns 0 through 9, and `randomDigitNotNull()` returns 1 through 9. `dateTimeBetween(start, end)` takes explicit UTC date-times, samples at whole-second precision including both endpoints, and returns an error when the start second follows the end second. All four formatters support `unique`.

`randomDigitNot(except)` excludes the supplied digit. `randomNumber(digits, strict)` uses a random width from one to nine when given `None`. Strict mode requires exactly the selected width. Non-strict mode also accepts zero digits and returns zero. Explicit widths above nine and strict zero digits return errors. Both methods support `unique`.

`randomFloat(decimals, min, max)` samples between the bounds and then rounds to the requested decimal place, with ties away from zero. Use `None` to choose a precision from zero to nine or to generate an upper bound. Reversed bounds are accepted. Rounding can move the result outside the original interval. Non-finite bounds return an error. Like PHP 8.5, the formatter keeps the sampled value when positive precision exceeds 308 or rescaling would overflow. Precision below −308 yields zero. `unique` treats positive and negative zero as the same result.

Use `unixTime(max)` to sample Unix seconds between the epoch and the specified endpoint, or `dateTime(max)` to return a UTC date-time. With `None`, the endpoint is the current time. `date(format, max)` and `time(format, max)` accept Chrono format strings, default to `%Y-%m-%d` and `%H:%M:%S`, and return an error for invalid formats. `iso8601(max)` includes the effective time-zone offset. All five methods support `unique`.

To reuse a PHP DateTime format from Laravel, call `dateWithPhpFormat(format, max)` or `timeWithPhpFormat(format, max)`. Both render in the effective time zone. For example, `Y-m-d` uses PHP tokens and `%Y-%m-%d` uses Chrono tokens. Each PHP variant shares `unique` history with its corresponding Chrono method.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn php_date_sample() -> DatabaseResult<String> {
    fake(None)?.dateWithPhpFormat(r"Y-m-d\TH:i:sP", Some(chrono::DateTime::UNIX_EPOCH))
}
```

Use `dateTimeInInterval(date, interval)` to sample whole seconds between a UTC instant and that instant plus a signed Chrono `TimeDelta`. A negative duration samples backward. `None` defaults to 30 years ago for the instant and five days forward for the duration. Use `dateTimeThisCentury(max)`, `dateTimeThisDecade(max)`, or `dateTimeThisMonth(max)` to start 100 years, 10 years, or one month before now. `dateTimeThisYear(max)` starts at midnight on January 1 in the application time zone. The other calendar lookbacks and the omitted start of `dateTimeInInterval` also use that time zone. When both interval inputs are omitted, the default five days are local calendar days. An explicit `TimeDelta` is elapsed time. All four use now when `max` is `None`, reject an end before the start, and normalize calendar overflow as upstream Faker does. All five methods support `unique`. Supply typed dates and durations in Rust. For an application-defined calendar interval, compute its endpoint and use `dateTimeBetween`.

Use `amPm(max)`, `dayOfMonth(max)`, `dayOfWeek(max)`, `month(max)`, `monthName(max)`, and `year(max)` to format parts of a sampled date in the effective time zone. The numeric month is zero-padded. The numeric day is also zero-padded except in `cs-CZ`. Czech, Turkish, Simplified Chinese, Traditional Chinese, and Georgian use local weekday and month names. Turkish and both Chinese locales also localize `amPm`. `dateTimeAD(max)` samples from upstream Faker’s 64-bit AD lower bound. These methods use the current time when `max` is `None`. `century()` samples Roman numerals from I through XXI. All support `unique`.

With `cs-CZ`, use `monthNameGenitive(max)` for the genitive month and `formattedDate()` for a display string such as `16. listopadu 2003`. The day, month, and year are sampled independently, so the result may fall outside the calendar. Both methods support `unique` and return an error in other locales.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_date_sample() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).formattedDate()
}
```

Use `timezone(countryCode)` to sample an IANA time-zone identifier. Pass `None` for the full list, or a country code such as `Some("TW")` to restrict the candidates. Country codes are case-sensitive. Unknown codes return `None`. The data is bundled with the framework and matches the IANA version used by `chrono-tz`. The formatter also supports `unique`. You can parse the returned identifier and use it to convert a UTC date.

Date strings use the generator’s `setDefaultTimezone(Some(zone))` override, then `app.timezone` from the application or factory connection, then UTC. Pass `None` to clear the override and use `getDefaultTimezone()` to inspect it. Use `dateTimeInTimezone(max, timezone)` for a typed zoned result, with `Some(zone)` taking precedence for that call. The date-time methods continue to return UTC instants. The generator override controls output formatting and leaves the application’s calendar boundaries in effect.

Use the `InTimezone` variants of `dateTimeAD`, `dateTimeBetween`, `dateTimeInInterval`, and the four `dateTimeThis…` methods when the result needs to retain its output zone. The two expression-based range methods have zoned variants too. An explicit zone controls the result only. Relative expressions keep using the application time zone unless the expression specifies its own. Zoneless calls return UTC. `unique` shares history by formatter and distinguishes named output zones, even when their UTC offsets match. A repeated local time in the same zone during a daylight-saving rollback counts as a repeat.

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

Use `dateTimeBetweenExpressions(start, end)` for PHP-style date expressions, such as `"-30 years"` and `"now"`. Both bounds use one reference time and the application time zone. An explicit zone in an expression takes precedence. A numeric end string is a Unix timestamp, and `"@-1"` is an explicit timestamp expression. When an endpoint fails to parse, this method uses Unix second zero, matching upstream Faker’s numeric use of PHP’s `false`. A start later than the end still returns an error. This method shares `unique` history with `dateTimeBetween`.

For named zones in date expressions, use PHP’s spelling rules. For example, `America/New_York` and `Us/Eastern` are accepted. An uppercase first segment such as `US/Eastern` fails parsing. To select a fixed numeric offset in an expression, write it directly, such as `-05:00`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn date_expression_sample() -> DatabaseResult<i64> {
    let date = fake(None)?.resolveDateMax("2024-01-01 12:34:56 America/New_York")?;
    Ok(date.timestamp())
}
```

Use `dateTimeInIntervalExpressions(date, interval)` for a PHP-style relative interval, such as `Some("+1 month")` or `Some("5 days ago")`. The end is calculated in the starting date’s time zone, using civil time across daylight-saving changes. The sampled instant is returned in UTC. Omitting the date uses 30 years ago, and omitting the interval uses five calendar days. The interval string accepts relative expressions and rejects absolute dates, times, and zone identifiers. It shares `unique` history with `dateTimeInInterval`.

Use `resolveDateMax(expression)` to pass a PHP-style upper bound to any date formatter that takes `max`. Resolve once to share the same bound across calls, then pass it as `Some(max)`. Decimal and exponent-form numeric strings are truncated toward zero as Unix seconds. An empty string uses the current time. Date text uses the application time zone unless it specifies a zone. Invalid or out-of-range values return an error.

Use `unixTimeExpression(max)` to sample Unix seconds directly from a string bound, including timestamps and expanded years outside Chrono’s date range. It shares `unique` history with `unixTime`. A malformed bound uses Unix second zero. Use `resolveDateMax` to validate an input before sampling. Parser limits and out-of-range values still return errors. Finite numeric strings beyond the signed 64-bit range saturate, and numeric strings that overflow to infinity resolve to zero, following PHP’s integer cast. Date-returning methods still require a bound representable by Chrono.

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

### Custom Faker Providers {#custom-faker-providers}

Build a `FakerProvider` with named Rust callbacks and install it with `addProvider`. Registration applies to that generator. A newer provider wins for a matching name regardless of ASCII case. `getFormatter` returns the callback resolved at the time of lookup. Later calls to `format` resolve again. Custom callbacks receive the active generator and a slice of `Value` arguments, and any error they return propagates to the caller.

Use `parse` to expand tokens such as `{{ code }}` from zero-argument scalar formatters. Unknown names return `UnknownFakerFormatter`. The named call supports registered formatters and the built-in defaults `address`, `city`, `company`, `companyEn`, `email`, `firstName`, `freeEmail`, `lastName`, `name`, `phoneNumber`, `postcode`, `safeEmail`, `userName`, and `uuid`. The generator methods `parse`, `mimeType`, `fileExtension`, `bloodType`, and `bloodRh` take precedence over providers in named calls. Typed calls such as `faker.name()` continue to use the built-in implementation. Use the generator you intend to customize, since `fake()` and factory definitions use separate generators.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::{Faker, FakerProvider}};

fn sample_provider() -> DatabaseResult<String> {
    let faker = Faker::new("en-US")?;
    faker.addProvider(FakerProvider::new().formatter("code", |_, _| Ok("sample".into())));
    faker.addProvider(FakerProvider::new().formatter("Code", |_, _| Ok("demo".into())));
    faker.parse("account-{{ code }}")
}
```

Use `unique(reset).format(name, arguments)` to require a distinct result. As in upstream Faker, the caller’s spelling identifies the history, so `code` and `Code` have separate histories even when both resolve to the same formatter. Changing arguments or replacing the provider keeps the history for that spelling. Values retain their Rust types, so the integer `7` and the text `"7"` are distinct. Named and typed calls keep separate histories. `unique(true)` clears both on that generator. Use the existing `optional` and `valid` closures to call `format` with a default or a predicate. A skipped optional call does not look up the formatter.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::factories::{Faker, FakerProvider},
};

fn sample_named_modifiers() -> DatabaseResult<(Value, Option<Value>, Value)> {
    let faker = Faker::new("en-US")?;
    faker.addProvider(FakerProvider::new().formatter("code", |_, _| Ok("demo".into())));

    let distinct = faker.unique(true).format("code", &[])?;
    let fallback = faker.optional(Some(0.0), Some("skip".into()), |faker| {
        faker.format("code", &[])
    })?;
    let valid = faker.valid(
        |value| matches!(value, Value::Text(text) if text.starts_with('d')),
        Some(2),
        |faker| faker.format("code", &[]),
    )?;
    Ok((distinct, fallback, valid))
}
```


### Numeric and Pattern Formatter Arguments {#numeric-pattern-formatter-arguments}

Pass positional arguments to `format` or a callback from `getFormatter`. The numeric generator methods `numberBetween`, `randomDigit`, `randomDigitNot`, `randomDigitNotZero`, `randomNumber`, and `randomFloat` take precedence over providers. `boolean`, `latitude`, `longitude`, `randomDigitNotNull`, `randomLetter`, `randomAscii`, `numerify`, `lexify`, `bothify`, `asciify`, and `regexify` are provider defaults.

Omitted arguments use upstream Faker defaults. An explicit `Value::Null` retains the formatter’s null behavior where it differs from omission. For example, `numberBetween(7, null)` samples from zero through seven, and `numerify(null)` returns null. Use integer values for counts and percentages, boolean values for strict mode, numeric values for float bounds, and text values for patterns. Rust validates these types and rejects extra arguments for parameterized formatters.

```rust
use openrust::database::{DatabaseResult, Value, eloquent::factories::Faker};

fn sample_named_arguments() -> DatabaseResult<(Value, Value)> {
    let faker = Faker::new("en-US")?;
    let number = faker.format("numberBetween", &[7.into(), 7.into()])?;
    let pattern = faker.getFormatter("regexify")?;
    let code = pattern(&["^X{2}$".into()])?;
    Ok((number, code))
}
```

### Formatter Extensions {#formatter-extensions}

Register extension methods with `addExtension` and resolve them with `ext(id)` or `format("Vendor\Package->method", args)`. Extension identifiers are case-sensitive. Method lookup ignores ASCII case. Qualified lookup goes directly to the extension. An unknown extension returns `UnknownFakerExtension`. A missing method on a registered extension returns `UnknownFakerFormatter`.

Built-in identifiers under `Faker\Extension\` cover `BarcodeExtension`, `BloodExtension`, `ColorExtension`, `FileExtension` (`mimeType` and `extension`), `NumberExtension`, and `UuidExtension` (`uuid3`). Replacing an identifier replaces its method set. The named generator methods for numbers, barcodes, blood groups, and files use the registered extension. Typed Rust methods continue to use their built-in implementations. A handle or formatter obtained before replacement retains its resolved callbacks.

The qualified name also works in `parse` and `unique(false).format(name, args)`. Pass it to `format` inside the existing `optional` and `valid` closures.

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::factories::{Faker, FakerProvider},
};

fn sample_extension() -> DatabaseResult<(Value, String)> {
    let faker = Faker::new("en-US")?;
    faker.addExtension(
        r"App\Faker\Code",
        FakerProvider::new().formatter("value", |_, _| Ok("demo".into())),
    );
    let value = faker.unique(true).format(r"App\Faker\Code->value", &[])?;
    let label = faker.parse(r"account-{{ App\Faker\Code->value }}")?;
    Ok((value, label))
}
```

### Text Generation {#text-generation}

Use `words(count)`, `sentences(count)`, and `paragraphs(count)` for vectors of exactly that size. Their `AsText` variants join words or sentences with spaces and paragraphs with blank lines. Use `sentenceWithWordCount(count, variable)` and `paragraph(count, variable)` for a fixed count or the same count variation rule as upstream Faker. The range-based `sentence(words)` remains available. `text(max)` uses a UTF-8 byte limit. Values below five or a limit that cannot fit a generated unit return an error. These methods support `unique`. Vocabulary comes from fake-rs and varies by locale.

### Pattern Formatters {#pattern-formatters}

Use `numerify(Some(pattern))` to replace `#` with digits and `%` with nonzero digits. `lexify` replaces `?` with lowercase ASCII letters. `bothify` expands all three and replaces each `*` with a digit or lowercase letter, with equal probability for the two classes. `asciify` replaces `*` with printable ASCII from `!` through `~`. Other characters are preserved. Pass `None` to use the upstream Faker default pattern. `randomLetter()` and `randomAscii()` return individual Rust characters. All six methods support `unique`.

Use `regexify(pattern)?` to generate a sample from a basic regular expression. It accepts optional slash delimiters and outer anchors, literals, ASCII classes and ranges, `\d`, `\w`, dots, ordinary groups, alternatives and bounded repeats. As in upstream Faker, `*` and `+` choose a per-pattern upper bound from one through nine. Invalid syntax, unsupported features such as lookarounds and backreferences, and oversized generation return errors. `unique(false).regexify(pattern)?` deduplicates generated strings.

### Collection Sampling {#collection-sampling}

Use `randomElement(values)` to borrow one item or get `None` from an empty slice. `randomKey(values)` returns its zero-based position. To choose from map keys or enum variants, pass a slice of the keys or variants to `randomElement`. With `randomElements(values, count, allowDuplicates)?`, use `Some(n)` for a fixed count or `None` for a random count from one through the slice length. With replacement disabled, each position is selected at most once. Equal values at different positions can still appear together. A zero count returns an empty vector. A nonzero sample requires a nonempty slice, and sampling without replacement rejects counts above its length.

Use `shuffleArray(values)` to return a shuffled clone and `shuffleString(value)` to shuffle Unicode scalar values. `boolean(None)` uses a 50% chance of true. Pass `Some(75)` for a 75% chance. These methods support `unique`, which compares JSON-serialized values for generic samples and shuffled arrays.

### Locale, Country and Currency Samples {#locale-and-code-samples}

Use `languageCode()`, `countryCode()`, `countryISOAlpha3()`, `currencyCode()`, and `emoji()` for fixture values from the pinned upstream Faker catalog. `randomLocale()` samples a locale identifier such as `en_GB` or `zh_TW`. It corresponds to upstream Faker’s `locale()` formatter. OpenRust’s existing `locale()` reads the configured generator locale. The sample catalog includes historical identifiers. All six formatters support `unique`.

### Digest Samples {#digest-samples}

Use `md5()`, `sha1()`, and `sha256()` to generate lowercase hexadecimal fixture values of 32, 40, and 64 characters. Like upstream Faker, each formatter hashes the decimal text of a random integer from 0 through 2,147,483,647. All three support `unique` and are intended for test and seed data.

### Network Address Samples {#network-address-samples}

Use `ipv4()`, `localIpv4()`, `ipv6()`, and `macAddress()` for network fixtures. `ipv4()` samples the ranges `1.0.0.0`–`127.255.255.255` and `128.0.0.0`–`255.255.255.254` with equal probability for each range. The result can include private or special-use addresses. `localIpv4()` chooses among `10/8`, `172.16/12`, and `192.168/16` with equal probability. IPv6 uses eight uncompressed lowercase hexadecimal groups without leading zeros. MAC addresses use six uppercase two-digit octets separated by colons. All four support `unique`.

### Domains, URLs and Slugs {#domain-url-slug-samples}

Use `domainWord()?` to transliterate a generated surname to lowercase ASCII, and `tld()` to sample a suffix using the upstream Faker weights for supported locales. `domainName()?` joins them. `url()?` uses the upstream HTTP and HTTPS templates with optional `www`, slug and `.html`. Names use OpenRust’s locale data and transliteration, which can differ from upstream Faker’s ICU or fallback mappings. If the surname yields no usable characters, the domain and URL formatters return an error. `slug(count, variable)` joins generated words with hyphens. Pass `None` for six base words, `Some(0)` for an empty result, or `false` for an exact count. All five formatters support `unique`.

### Email and Password Samples {#email-and-password-samples}

Use `email()?` for a general address or `companyEmail()?` for an address on a generated domain. The simplified Chinese locale uses its free-provider list for general email. `safeEmail()` uses reserved example domains and `freeEmail()` uses localized provider domains. Use `safeEmailDomain()` and `freeEmailDomain()` to sample only the domain. Usernames use localized names or words with ASCII transliteration. If the result contains no usable characters, OpenRust generates an ASCII letter to preserve the infallible username and email methods. `password(min, max)?` generates printable ASCII from `!` through `~` with inclusive bounds in either order. The defaults are 6 and 20. These formatters support `unique`.

### Regional URL Templates {#regional-url-templates}

`url()` with `ne-NP` uses the pinned HTTP templates, which can add a `www` prefix, a `.np` suffix, or a path. `companyUrl()` with `uk-UA` returns the original Cyrillic company label with an optional hyphenated suffix. The label has no scheme or domain suffix. Both formatters support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_url_sample() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("ne-NP"))?.url()?, fake(Some("uk-UA"))?.companyUrl()?))
}
```

### Regional Domains and Email {#regional-domains-and-email}

Use `tld()` and `freeEmailDomain()` to sample the pinned Internet catalogs. Repeated entries retain their upstream weight. `domainName()` uses the Romanized surname catalog in Saudi Arabia, Jordan, Egypt, Iran, Korea, and Japan. Other locales use `domainWord()` followed by a suffix. General email in Nepal uses two company, one free-provider, and three company-with-`.np` template slots. Direct `companyEmail()` uses the ordinary domain there.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_email_sample() -> DatabaseResult<String> {
    fake(Some("ne-NP"))?.unique(false).email()
}
```

### Regional Usernames {#regional-usernames}

`userName()` uses upstream Faker’s romanized name catalogs in Saudi Arabia, Jordan, Egypt, Iran, Korea, and Japan. Lithuanian templates pair names and surnames from the same gender group. Simplified Chinese uses the pinned Lorem word list and includes an underscore template. The same generator supplies the local part for `safeEmail()` and `freeEmail()`. The general path uses Rust transliteration. When transliteration leaves only separators, it generates a letter so the username and email APIs remain infallible.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn username_sample() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).userName()
}
```

The same six regional Internet providers expose `firstNameAscii()` and `lastNameAscii()` for direct sampling from their name catalogs. They preserve the stored spellings and share those catalogs with usernames and domains. These formatters support `unique()` and return an error for other locales.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ascii_name_sample() -> DatabaseResult<(&'static str, &'static str)> {
    let faker = fake(Some("ar-EG"))?;
    Ok((faker.unique(false).firstNameAscii()?, faker.lastNameAscii()?))
}
```

### Phone and IMEI Samples {#phone-and-imei-samples}

Use `phoneNumber()` for display numbers from the pinned upstream Faker regional providers. Where a locale has no PhoneNumber provider, it uses the US display templates. Bangladesh uses Bangla digits, while Vietnamese subscriber length follows the selected area code. Brazil also provides `phoneNumberCleared()` for digits-only numbers and supports `unique()`. `e164PhoneNumber()` uses the pinned international templates and available regional overrides. It returns a plus sign followed by digits for test data. `imei()` returns 15 digits, including the Luhn check digit. All three support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn display_phone_sample() -> DatabaseResult<(String, String)> {
    let bangladesh = fake(Some("bn-BD"))?;
    let brazil = fake(Some("pt-BR"))?;
    Ok((bangladesh.phoneNumber(), brazil.unique(false).phoneNumberCleared()?))
}
```

### Regional mobile, landline, and extension samples {#regional-phone-samples}

Use `mobileNumber()` and `fixedLineNumber()` for the regions that expose those formatters. Malaysia also provides `mobileNumberWithOptions(countryCodePrefix, formatting)` and `fixedLineNumberWithOptions(countryCodePrefix, formatting)`. Its default includes the country prefix and separators. Brazil provides `cellphoneNumber(formatted)` and `landlineNumber(formatted)`, with formatting enabled by default. Hong Kong provides `landlineNumber(None)`, Korea uses `cellPhoneNumber()`, and Greece retains `mobilePhoneNumber()`.

Use `phoneNumberWithExtension()` for the US or Canadian PhoneNumber provider. The same US extension and E.164 behavior applies when the chosen locale lacks a PhoneNumber provider and falls back to `en-US`. Regional methods return an error when unavailable. Each supports `unique()`.

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

### Toll-free, service, and VoIP samples {#phone-service-samples}

Use `tollFreePhoneNumber()` for US, Canadian, German, and Romanian formats. Locales without a PhoneNumber provider inherit the US formatter. Greece, New Zealand, South Africa, and Spain use `tollFreeNumber()`. Singapore provides `tollFreeLineNumber()`, `tollFreeInternationalNumber()`, and `premiumPhoneNumber()`. France provides `serviceNumber()`, Romania provides `premiumRatePhoneNumber()`, and Greece provides `personalNumber()`, `sharedCostNumber()`, and `premiumRateNumber()`.

Singapore and Malaysia provide `voipNumber()`. Malaysia also supports `voipNumberWithOptions(countryCodePrefix, formatting)`. Its default includes the country prefix and separators. The formatters support `unique()` and return an error when the selected provider lacks the method.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn phone_service_sample() -> DatabaseResult<(String, String)> {
    Ok((
        fake(Some("en-SG"))?.unique(false).tollFreeLineNumber()?,
        fake(Some("ms-MY"))?.voipNumberWithOptions(false, false)?,
    ))
}
```

### Regional phone components {#regional-phone-components}

Use `areaCode()`, `exchangeCode()`, and `tollFreeAreaCode()` to sample supported phone components. The values are strings, so leading zeros are preserved. Finland provides local and international area code variants. Its `numberFormat()` returns a mask with placeholders. Brazil provides `cellphone(formatted)`, `landline(formatted)`, and `phone(formatted)` for subscriber numbers without an area code. These formatters also support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn phone_component_sample() -> DatabaseResult<(String, String)> {
    Ok((
        fake(Some("en-CA"))?.unique(false).tollFreeAreaCode()?,
        fake(Some("pt-BR"))?.cellphone(Some(false))?,
    ))
}
```

### Barcodes and ISBNs {#barcode-samples}

Use `ean8()`, `ean13()`, `isbn10()`, and `isbn13()` for checksum-valid sample codes. The returned strings preserve leading zeros. ISBN-10 can end in `X`; ISBN-13 starts with `978` or `979`. Each formatter has its own `unique` history.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn catalog_codes() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en"))?;
    Ok((faker.ean13(), faker.unique(false).isbn13()?))
}
```

### File Samples {#file-samples}

`mimeType()` samples a MIME type from the fixed upstream Faker catalog. `fileExtension()` samples a MIME group and returns one of its extensions without a dot. Both support `unique`, as does the version 3 `uuid()` formatter. `file(sourceDirectory, targetDirectory, fullPath)` copies a readable file from the top level of an existing directory to another existing directory. Its generated name retains the source extension. Rust uses explicit paths and returns an I/O error if copying fails.

```rust
use std::{io, path::{Path, PathBuf}};
use openrust::database::eloquent::factories::Faker;

fn copy_sample(faker: &Faker, source: &Path, target: &Path) -> io::Result<PathBuf> {
    faker.file(source, target, false)
}
```

### Placeholder Images {#placeholder-images}

Use `imageUrl(&ImageOptions)` to build a upstream Faker placeholder URL. The defaults are 640 × 480 PNG with randomized text and background. JPG and JPEG are also supported. `image(directory, fullPath, options).await` downloads from the provider, using the system temporary directory when omitted. A failed HTTP request or interrupted response returns no path. Invalid options and local write failures return an I/O error.

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

### Regional Identity Samples {#regional-identity-samples}

Use `ssn()` with `en-US`, `nino()` with `en-GB`, and `personalIdentityNumber(gender)` with `zh-TW` or `zh-Hant`. The US and British methods use the fixed upstream Faker ranges and prefix rules. The Taiwanese method computes the check digit and accepts `Some("male")`, `Some("female")`, or `None` for a random gender code. These methods generate test data, return an error for unsupported locales, and support `unique`.

The `ssn()` formatter also supports `th-TH`, `de-AT`, and `is-IS`. Thai uses the same upstream Faker format as US English. Austria and Iceland generate a date within the past 100 years and calculate the check digit. With `de-AT`, use `ssnWithBirthdate(Some(date))` to pass a `chrono::NaiveDate`, or `None` to sample a date. Both SSN methods share the `unique` history.

`personalIdentityNumber(gender)` also supports `lt-LT`, `lv-LV`, `fi-FI`, `nb-NO`, `sv-SE` and `pl-PL`. The first five accept a civil date through `personalIdentityNumberWithBirthdate(birthdate, gender)`. Lithuania accepts a serial prefix through `personalIdentityNumberWithSerial(birthdate, gender, serial)`. Poland uses the identity card format. The pinned Norwegian provider generates two random trailing digits.

For `pl-PL`, use `pesel(birthdate, sex)` for an 11-digit PESEL. Pass a `chrono::NaiveDate` from 1800 through 2299, or `None` to sample a birth date. `Some("F")` selects an even sex digit. `Some("M")` and `None` select an odd digit as in upstream Faker. `taxpayerIdentificationNumber()` generates a 10-digit NIP. Both methods calculate check digits and support `unique`.

For `pt-BR`, use `cpf(formatted)`, `rg(formatted)`, and `cnpj(formatted)`. Pass `None` for the default punctuation or `Some(false)` for digits only. RG follows the pinned São Paulo provider, with a numeric check digit. CNPJ uses branch `0001`. All three methods support `unique`.

Use `dni()` with `es-ES` for eight digits and a modulo-23 check letter, or with `es-PE` for eight digits. The Peruvian `ruc(isNaturalPerson)` defaults to the `20` prefix and uses `10` for `Some(true)`. The pinned provider appends nine random digits. Use `licenceCode()` for a driving licence category from `es-ES` or `en-ZA`. These methods support `unique`.

Use `idNumber()` with `nl-NL`, `en-ZA`, `ar-SA`, or `mn-MN`. South Africa also accepts a civil date, citizenship flag and gender through `idNumberWithBirthdate(birthdate, citizen, gender)`. With `ar-SA`, `nationalIdNumber()` fixes prefix `1` and `foreignerIdNumber()` fixes prefix `2`. With `ar-EG`, use `nationalIdNumberWithGender(gender)` to control the registration parity, or `nationalIdNumber()` for the default. The Egyptian provider generates its last digit randomly. The Mongolian `alphabet()` returns one Cyrillic character. The Egyptian `governorateId()` returns a two-character code. These methods support `unique`.

Use `passportNumber()` with `lt-LT` for eight digits or `lv-LV` for two lowercase letters followed by seven digits. `tcNo()` samples the pinned Turkish formatter. The generator permits a leading zero and preserves the upstream signed remainder, which can produce a minus sign in the suffix. `Faker::tcNoChecksum(prefix)` requires nine ASCII digits. `Faker::tcNoIsValid(number)` compares a two-character suffix and returns an error for malformed prefixes. Both generators support `unique`.

Use `ein()` with `en-US` for a two-digit provider prefix and a seven-digit suffix. With `ru-RU`, `inn10(area)` and its legacy name `inn(area)` generate a ten-digit INN. Pass `None`, an empty string, or zero to sample an area from 01 to 91. An explicit area must represent a number from 1 to 99 using ASCII digits. `kpp(inn)` uses the first four digits of an INN and appends `01001`. A missing or short input samples a fresh INN. Use `Faker::inn10Checksum` on a nine-digit prefix and `Faker::inn10IsValid` on a complete number. All four generators support `unique`.

Use `siret(None)` with `fr-FR` for a French establishment identifier formatted as 3–3–3–5 digits. Pass `Some(false)` for the compact form. The nine-digit SIREN prefix and the full fourteen-digit identifier each have a Luhn check digit. With `pl-PL`, `regon()` generates a nine-digit business register number and `regonLocal()` extends a valid REGON prefix to fourteen digits. Both use a weighted modulo-11 check digit, mapping a remainder of 10 to zero. All three generators support `unique`.

Use `ico()` with `cs-CZ` for an eight-digit Czech business number with a check digit. With `da-DK`, `cvr()` returns eight digits and `p()` returns ten. With `is-IS`, `vsk()` returns five digits. The Danish and Icelandic formats start with a nonzero digit and follow the upstream sample masks. All four generators support `unique`.

With `ar-SA`, `companyIdNumber()` generates a ten-digit Saudi company ID beginning with `700`. With `ar-EG`, `companyTaxIdNumber()` returns ten digits and `companyTradeRegisterNumber()` returns seven. The Egyptian formats may start with zero. All three append a Luhn check digit and support `unique`.

With `kk-KZ`, `businessIdentificationNumber(date)` returns a twelve-digit BIN. Pass a civil registration date to set its year and month, or `None` to sample a date from the start of this year through now. With `en-ZA`, `companyNumber()` samples a registration year from the last fifty years through now and returns `YYYY/NNNNNN/TT`. Both support `unique`.

With `zh-Hant`, `VAT()` returns the Taiwanese eight-digit sample as an integer. The existing `vat(...)` interface also accepts this locale and returns those digits as a string, without a country prefix. With `el-GR`, `doy()` samples one of 21 tax offices and `object()` samples one of 19 business activities. Each formatter supports `unique`.

For `en-US`, `fr-FR`, `it-IT`, and `pl-PL`, `company()` follows the upstream Faker name templates and `companySuffix()` samples the corresponding suffix list. Polish company names also use the pinned Polish surname lists and organization prefixes. The other three use the pinned Person surname catalogs. Locales without a pinned company template use the existing fake-rs company data and fallback. Both methods support `unique`.

For `zh-Hans`, `zh-Hant`, `ja-JP`, and `ko-KR`, `company()` follows the corresponding upstream Faker templates. `companyPrefix()` samples a prefix in these four locales and `pl-PL`. Use `companyModifier()` or `companyEnSuffix()` with `zh-Hant` to sample a Taiwanese company modifier or English suffix. These three methods return an error for unsupported locales and support `unique`. Japanese company names use the pinned Japanese surname catalog. Japanese `companySuffix()` returns the inherited `Ltd`, while the company name template uses a prefix.

With `ar-EG`, `ar-JO`, and `ar-SA`, company names use the pinned Arabic prefixes, regional surnames and suffixes. The Egyptian templates also use city names. The generators preserve spaces in the suffix catalog. All three locales support `companyPrefix()` and `companySuffix()` with `unique`.

With `de-DE`, `de-AT`, `de-CH`, and `cs-CZ`, `company()` uses the pinned regional surnames, templates, and suffixes. Czech names always include a suffix and can join two surnames with a space, hyphen, or `a`. The three German-language providers also generate names consisting of a surname alone. `companySuffix()` supports `unique` and preserves the original suffix spellings and sampling weights.

With `da-DK`, `is-IS`, `fi-FI`, `sv-SE`, and `nb-NO`, `company()` uses the pinned regional name and suffix catalogs. The templates retain their original conjunctions, punctuation, and sampling weights. Use `companySuffix()` to sample just the suffix. Both methods support `unique`.

For `es-AR`, `es-ES`, `es-PE`, `es-VE`, `pt-BR`, and `pt-PT`, `company()` uses the pinned regional surnames and company templates. `companySuffix()` uses the corresponding suffix catalog, including `Ltda.` in Brazil and `Lda.` in Portugal. `companyPrefix()` also supports Spain and Venezuela. All three methods support `unique`.

With `nl-NL`, `company()` combines product and shop terms or a trade followed by a surname. It can append a Dutch company suffix. With `nl-BE`, it uses a Belgian surname with an optional Belgian suffix. Dutch surnames retain their original capitalization, including particles such as `van`. Both locales support `companySuffix()` and `unique`.

With `hr-HR`, `sk-SK`, and `hu-HU`, `company()` uses the pinned regional name lists and templates. Croatian names can use a trade prefix or a suffix, and `companyPrefix()` returns a Croatian trade prefix. Slovak templates retain the hyphen and the literal `and`. Hungarian names can consist of a surname alone. All three support `companySuffix()` and `unique`.

With `el-GR`, `company()` uses a Greek surname followed by a legal suffix. With `me-ME`, it combines names and legal types from the pinned Montenegrin catalogs. Use `companyName()` and `companyType()` to sample those components. Montenegrin `companySuffix()` returns the inherited `Ltd`. All four methods support `unique`.

With `en-GB` or `en-ZA`, `company()` combines a regional surname with the inherited `Ltd` suffix. With `el-CY`, it combines a surname with a Cypriot legal suffix or joins two surnames with a hyphen. `companySuffix()` and `unique` are available in all three locales.

With `fr-BE`, `fr-CA`, `fr-CH`, and `it-CH`, `company()` uses the pinned regional surnames and templates. French Canada inherits the French company suffixes. The two Swiss providers use their own suffixes, including `Sàrl` in French and `Sarl` in Italian. Both `company()` and `companySuffix()` support `unique`.

For `lt-LT`, `sl-SI`, and `tr-TR`, `company()` uses the pinned regional templates. Lithuanian legal designations precede the name, Slovenian sole-proprietor names end in `s.p.`, and Turkish templates can include a business field. Use `companyField()` with `tr-TR` for a field on its own. It also supports `unique`.

With `ru-RU` and `kk-KZ`, company names combine a legal prefix with name elements and an optional name ending. Use `companyNameElement()` and `companyNameSuffix()` to sample those parts. `companySuffix()` still returns the inherited `Ltd`. With `uk-UA`, `companyName()` samples the base name, and `company()` can add a prefix, quoted name, or hyphenated suffix. The regional formatters support `unique`.

With `fa-IR`, `he-IL`, `hy-AM`, and `ka-GE`, `company()` uses the pinned regional templates and catalogs. Iranian companies can include a prefix and a business field. In this locale, `contract()` samples an employment contract type. Georgian company names use a prefix, name elements, and a name ending, which can also be sampled individually. These methods support `unique`.

With `bn-BD`, `company()` combines a catalog name and type, also available from `companyName()` and `companyType()`. Indonesian companies (`id-ID`) use a legal prefix and one or two surnames, with an optional suffix. Malaysian companies (`ms-MY`) use a catalog name or an industry and a regional name. The suffix sampler preserves the upstream weights. Use `companyName()` and `industry()` independently. Thai companies (`th-TH`) use a local surname and the inherited `Ltd` suffix. Use `slogan()` for a three-part Thai slogan. These formatters support `unique`.

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

### Taiwanese English Company Names {#taiwanese-english-company-names}

With `zh-TW` or `zh-Hant`, `companyEn()` looks up `lastNameEn` on the same generator. Register a formatter that returns text to enable it. Without a registration, the default missing formatter error is preserved. Use `companyEnWithLastName` to supply a fallible Rust closure for the surname formatter. The closure runs once per surname in the selected template. The `unique` variants share the `companyEn` history.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwanese_company_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant"))?
        .unique(false)
        .companyEnWithLastName(|| Ok("Lin".into()))
}
```

### Job Titles {#job-titles}

Use `jobTitle()` for a profession from the locale’s Company provider. Dedicated lists are available for `de-DE`, `en-US`, `fr-FR`, `id-ID`, `nb-NO`, `nl-NL`, `ru-RU`, `sv-SE`, `tr-TR`, and `uk-UA`. Belgium, Canada, and Switzerland use the French catalog for their French locales. A locale whose Company provider inherits the base job format yields a single Lorem word. A locale without a Company provider uses the US catalog. Use `unique(false).jobTitle()` to avoid repeats.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn job_title_sample() -> DatabaseResult<String> {
    fake(Some("id-ID"))?.unique(false).jobTitle()
}
```

### Biased Sampling and Modifiers {#faker-biased-and-modifiers}

Use `biasedNumberBetween(min, max, function)?` to sample an inclusive integer range. Pass `None` for the defaults of 0, 100, and a square-root curve. A custom curve takes a number in [0, 1) and returns a probability between 0 and 1. Reversed ranges, invalid probabilities, and exhausted sampling return errors. The formatter supports `unique`.

Use `optional(weight, default, generate)?` to call a typed formatter or return the optional default. The default weight is 0.5. Values above one use upstream Faker percentage interpretation. Use `valid(predicate, maxRetries, generate)?` to reject samples until the predicate passes. The default retry limit is 10,000. Both closures propagate formatter errors. Use `uniqueWithRetries(reset, maxRetries)` to set the attempt limit on a handle that shares the Faker formatter history.

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

### Corpus-Based Text {#corpus-text-samples}

Use `realText(maxNbChars, indexSize)?` or `realTextBetween(minNbChars, maxNbChars, indexSize)?` to generate text from the fixed upstream Faker corpus with a weighted Markov chain. Use `None` for the default maximum of 200, minimum of 160, or context size of two, respectively. The context size accepts one through five. For `realTextBetween`, the output has more than the minimum and at most the maximum number of Unicode characters. Unsupported locales, including Simplified Chinese, use the upstream English fallback. Traditional Chinese and Japanese sample by character. Persian `realText` uses the upstream byte limit. Its `realTextBetween` returns a retry error because the corpus has no uppercase starting words. Both methods support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn corpus_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant"))?.realTextBetween(Some(80), Some(120), Some(2))
}
```

### Sample HTML {#html-samples}

Use `randomHtml(maxDepth, maxWidth)?` to generate an HTML document with a title, a sample login form and a random body subtree. Pass `None` for either limit to use four. A depth of one or less produces only the title and form. Width bounds each random sibling draw. The generated form and links use reserved example domains. Numeric IDs are unique within each document, and `unique` tracks entire documents separately.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn html_sample() -> DatabaseResult<String> {
    let faker = fake(None)?;
    faker.randomHtml(Some(3), None)
}
```

### Blood Group Samples {#blood-group-samples}

Use `bloodType()` for A, AB, B or O, `bloodRh()` for the Rh sign, and `bloodGroup()` to sample both independently and join them without a separator. The same sample values are available in every locale. All three formatters support `unique` with separate histories.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn blood_samples() -> DatabaseResult<(&'static str, &'static str, String)> {
    let faker = fake(None)?;
    Ok((faker.bloodType(), faker.bloodRh(), faker.unique(false).bloodGroup()?))
}
```

### Color Samples {#color-samples}

Use `hexColor()` for six-digit RGB hex, or `safeHexColor()` for the fixed upstream Faker range `#000000` through `#00ffff` with paired hex digits. `rgbColor()` and `hslColor()` return comma-separated values, and their `AsArray` variants return three integers. `rgbCssColor()` and `rgbaCssColor()` produce CSS strings. `safeColorName()` and `colorName()` use the upstream Faker catalog for the resolved locale when available and fall back to the default catalog. All ten formatters support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn color_samples() -> DatabaseResult<(String, [u8; 3], &'static str)> {
    let faker = fake(Some("zh-Hant"))?;
    let unique = faker.unique(false);
    Ok((faker.rgbaCssColor(), unique.rgbColorAsArray()?, unique.colorName()?))
}
```

### Browser User Agents {#browser-user-agent-samples}

Use `userAgent()?` for a random browser sample. Select a specific format with `firefox()?`, `chrome()`, `internetExplorer()`, `opera()`, `safari()` or `msedge()`. The samples use the fixed upstream Faker browser versions and platform tokens. Firefox samples a Gecko date in the application time zone. Use `macProcessor()`, `linuxProcessor()`, `windowsPlatformToken()`, `macPlatformToken()`, `linuxPlatformToken()` and `iosMobileToken()` for individual tokens. All formatters support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn browser_samples() -> DatabaseResult<(String, String)> {
    let faker = fake(None)?;
    Ok((faker.userAgent()?, faker.unique(false).msedge()?))
}
```

### Bank Identifiers {#bank-identifier-samples}

Use `iban(countryCode, prefix, length)` for IBAN samples with a modulo 97 checksum. Pass `None` for a random country or supply a two-letter code. Known countries use the fixed upstream Faker layouts. An unlisted code uses a numeric account body with the requested length, defaulting to 24 digits. The prefix replaces the beginning of that body and preserves leading zeros. Rust accepts uppercase ASCII letters and digits in the prefix and caps the account body at 30 characters. `swiftBicNumber()` returns an 8- or 11-character sample. Both methods support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn bank_samples() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en"))?;
    let unique = faker.unique(false);
    Ok((unique.iban(Some("DE"), "0012", None)?, unique.swiftBicNumber()?))
}
```

### Regional Bank Accounts {#regional-bank-account-samples}

Use `bankAccountNumber(prefix, countryCode, length)` to select the account format for the Faker locale. The supported IBAN aliases default to their own country and accept explicit overrides. The Egyptian provider always generates an Egyptian IBAN. US English, Malaysian Malay and Nepali generate domestic formats. For these four providers, pass `("", None, None)`. `bankRoutingNumber()` generates a nine-digit routing sample for `en_US`. `Faker::calculateRoutingNumberChecksum` reads the first eight digits and returns the check digit. Both generators support `unique`.

If a locale lacks a local Payment provider, it uses the US provider for domestic accounts and routing numbers. A local Payment provider that lacks either method returns an error for that method. Use `("", None, None)` for a fallback bank account. The legacy `at-AT` alias uses Austrian account and VAT formats.

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

### Financial Institutions {#institution-samples}

`bank()` uses the fixed locale catalog for `de_DE`, `el_CY`, `ka_GE`, `kk_KZ`, `ms_MY`, `pl_PL`, `pt_BR`, `ru_RU`, `th_TH`, `uk_UA`, and `zh_CN`. Malaysian Malay also provides `localBank`, `foreignBank`, `governmentBank`, `insurance`, and `currencySymbol`. Nepali provides `commercialBank`, `developmentBank`, `financeCompany`, `microFinance`, and `digitalWallet`. Both provide `swiftCode()` from their respective catalogs. These formatters support `unique` and return errors for unsupported locales.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn institution_samples() -> DatabaseResult<(&'static str, String)> {
    let faker = fake(Some("ms-MY"))?;
    Ok((faker.foreignBank()?, faker.unique(false).swiftCode()?))
}
```

### VAT Samples {#vat-samples}

Use `vat(None)` for the locale default and `vat(Some(false))` for compact output where the upstream provider accepts spacing. Supported locales are `bg_BG`, `de_AT`, `es_ES`, `fr_BE`, `nl_BE`, `fr_FR`, `en_GB`, `it_IT`, and `nl_NL`. For the UK, `vatWithType` accepts `branch`, `gov`, and `health`; other values use the standard format. French VAT uses `siren`, and the Italian `vatId` and Dutch `btw` aliases follow their own providers. The generators support `unique` and preserve their upstream sample rules.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vat_samples() -> DatabaseResult<(String, String)> {
    let french = fake(Some("fr-FR"))?;
    let uk = fake(Some("en-GB"))?;
    Ok((french.unique(false).vat(None)?, uk.vatWithType(Some("branch"))?))
}
```

### Credit Card Samples {#credit-card-samples}

The credit card formatters generate vendor names, numbers with Luhn check digits, expiration dates and card details. Formatted numbers default to hyphen-separated groups. The `pt-BR` locale uses Brazilian vendor masks. Pass `true` for an expiration between now and 36 months ahead, or `false` for a date between 36 months ago and 36 months ahead. `creditCardExpirationDateString` accepts Chrono directives and defaults to `%m/%y`. All five formatters support `unique`.

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

### Postal Addresses and Titles {#address-and-title-samples}

Use `streetAddress()` for a street line and `address()` for a complete address sample. The US, French, simplified Chinese and traditional Chinese forms follow their respective upstream Faker layouts. US English, Simplified Chinese, and Traditional Chinese use the pinned upstream Faker city and street templates and address catalogs. Other locales may still use fake-rs components. Use `buildingNumber()`, `secondaryAddress()`, `postcode()`, `state()`, and `stateAbbr()` for individual fields. `title(Some("male"))` and `title(Some("female"))` select an honorific group. Use `title(None)` to sample either group. `titleMale()` and `titleFemale()` select directly. These methods support `unique`.

Use `area()` with Simplified Chinese to sample a district, and `street()` with Traditional Chinese to sample the street stem. Taiwanese city samples combine a county or municipality with one of its own districts. Both direct formatters support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwan_address_sample() -> DatabaseResult<String> {
    fake(Some("zh-Hant-TW"))?.unique(false).address()
}
```

The `fr-FR` formatter uses the pinned city and street templates and country vocabulary. `streetPrefix()` and `region()` sample the French catalogs. `department()` returns a pair of strings for the code and name, preserving leading zeroes and codes such as `2A`. `departmentNumber()` and `departmentName()` sample the corresponding field independently. For `zh-Hant-TW`, `randomChineseNumber()` returns a digit from 一 to 九, and `randomNumber2()` and `randomNumber3()` return integers in the inclusive ranges 1–100 and 1–1000. All these methods support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn french_department_sample() -> DatabaseResult<(&'static str, &'static str)> {
    fake(Some("fr-FR"))?.unique(false).department()
}
```

Belgian, Swiss, and Canadian French use their regional city, street, and postal layouts. Use `cityName()` for Belgium or Switzerland, `province()` for Belgium, and `canton()` for a Swiss code/name pair. `cantonShort()` and `cantonName()` sample their field independently. The three providers inherit the French country, region, and department catalogs, while Swiss street prefixes override the inherited list.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn swiss_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("fr-CH"))?;
    Ok((faker.address(), faker.unique(false).cantonShort()?))
}
```

Canadian, Australian, and New Zealand English reuse the US Address catalogs where the pinned providers inherit them. Canada uses `province()`, `provinceAbbr()`, and a restricted postcode alphabet exposed by `randomPostcodeLetter()`. Australia uses its own state and postcode catalogs and offers `buildingLetter()`. New Zealand uses `region()` and four-digit postcodes. The regional methods support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn canadian_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("en-CA"))?;
    Ok((faker.address(), faker.unique(false).provinceAbbr()?))
}
```

British English uses the fixed `en-GB` postcode catalog, city and street templates. The street line starts with a building number or with a separate `Flat` or `Studio` line. Use `county()` to sample the British county catalog. Both `postcode()` and `county()` support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn british_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("en-GB"))?;
    Ok((faker.address(), faker.unique(false).county()?))
}
```

South African English uses its own province names, abbreviations, city suffixes, and four-digit postcodes. Ugandan English offers `cityName()`, `district()`, and `region()` and samples one- to five-digit postcodes. Nigerian English offers `county()` and `region()` and supports both spaced and unspaced five-digit postcodes. All these regional formatters support `unique`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ugandan_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("en-UG"))?;
    Ok((faker.address(), faker.unique(false).district()?))
}
```

Singapore English uses inherited surname-based street names, local street suffixes, and six-digit postcodes. Full addresses use a street number or a block number and unit line. The pinned upstream Faker provider returns literal patterns from `floorNumber()` and `apartmentNumber()`, including in the full address. The regional formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn singapore_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en-SG"))?;
    Ok((faker.address(), faker.blockNumber()?))
}
```

Indian English uses local city, state, and locality catalogs. A full address ends with a six-digit postcode starting with 1–5. The street line uses a building and locality, optionally with a housing society. `areaSuffix()` preserves leading spaces in the catalog, and the five locality and society formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn indian_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("en-IN"))?;
    Ok((faker.address(), faker.unique(false).societyName()?))
}
```

Philippine English uses local provinces, cities and municipalities, with a four-digit postcode and optional apartment or suite. Use `municipality()` and `barangay()` for local fields. The independent `state()` and `stateAbbr()` retain their inherited U.S. catalogs. Hong Kong English uses a street, village or estate on the first line, then the town and city. The inherited five-digit `postcode()` formatter is available separately. The regional formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn philippine_and_hong_kong_address_samples() -> DatabaseResult<(String, String)> {
    let philippines = fake(Some("en-PH"))?;
    let hong_kong = fake(Some("en-HK"))?;
    Ok((philippines.address(), hong_kong.address()))
}
```

Japanese addresses start with a seven-digit postcode, followed by two spaces, then the prefecture, city, ward, and street. A building name and number may follow. `postcode1()` and `postcode2()` return the three- and four-digit components. `prefecture()`, `ward()`, and `areaNumber()` are also available separately. These regional formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn japanese_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("ja-JP"))?;
    Ok((faker.address(), faker.unique(false).prefecture()?))
}
```

Korean full addresses use a metropolitan city and borough, or a province and city, followed by the street name and building number. The inherited `streetAddress()` formatter puts the building number first. `postcode()` samples five digits separately. The regional `metropolitanCity()`, `province()`, and `borough()` formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn korean_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("ko-KR"))?;
    Ok((faker.address(), faker.unique(false).metropolitanCity()?))
}
```

Vietnamese addresses use either a street, ward, and district within a city or province, or a building number and hamlet within a province. The second line names the city or province. The pinned ward templates can start with a hamlet prefix. The hamlet, ward, and district names and prefixes support `unique()`. `postcode()` generates the separate five-digit, six-digit, or 5–4 digit form.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vietnamese_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("vi-VN"))?;
    Ok((faker.address(), faker.unique(false).districtName()?))
}
```

Thai addresses use two lines. The first contains the building number and a road name made from a road prefix and a catalog entry. The second contains a name-based city followed by a comma and five-digit postcode. Building numbers can include a slash and leading zeroes. Sample a province separately with `province()`. The road prefixes and provinces support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn thai_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("th-TH"))?;
    Ok((faker.address(), faker.unique(false).province()?))
}
```

Indonesian addresses place `No.` before the building number, use a five-digit postcode with a nonzero first digit, and end with a state abbreviation. Malaysian addresses keep the town, postcode, and state in the same region. Use `postcodeWithState("perlis")` for a specific upstream Faker state key. `postcode()` samples a region, and postcodes preserve leading zeroes. Malaysian building and township formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn indonesian_and_malaysian_address_samples() -> DatabaseResult<(String, String, String)> {
    let indonesia = fake(Some("id-ID"))?;
    let malaysia = fake(Some("ms-MY"))?;
    Ok((indonesia.address(), malaysia.address(), malaysia.postcodeWithState("perlis")?))
}
```

Bengali addresses use Bengali digits for the street number. The independent `postcode()` formatter uses ASCII digits with an optional four-digit extension. Nepali addresses join the city and one- or two-digit ward with a hyphen, then add the street name, district, and five-digit postcode. These regional formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn bengali_and_nepali_address_samples() -> DatabaseResult<(String, String)> {
    let bengali = fake(Some("bn-BD"))?;
    let nepali = fake(Some("ne-NP"))?;
    Ok((bengali.address(), nepali.address()))
}
```

Egyptian, Jordanian, and Saudi Arabic addresses place the street and city on separate lines. An apartment or building detail is optional. Jordanian city names can have a directional prefix, and the independent state formatters use the pinned U.S. state names and abbreviations. Egypt provides `governorate()` and `governorateId()`, and Saudi Arabia provides `governorate()` and `subdivision()`. These regional formatters support `unique()`. The pinned Saudi provider has no city-prefix catalog, so a standalone `cityPrefix()` uses the existing Rust fallback.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn arabic_address_samples() -> DatabaseResult<(String, String, String)> {
    Ok((fake(Some("ar-EG"))?.address(), fake(Some("ar-JO"))?.address(), fake(Some("ar-SA"))?.address()))
}
```

Persian addresses combine a city, street, and building name. A labeled ten-digit postcode can follow. Building and postcode prefix formatters can be called separately and support `unique()`. Hebrew addresses use two lines and five- or seven-digit postcodes. The street line ends with a space when the optional apartment or floor is absent.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn persian_and_hebrew_address_samples() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("fa-IR"))?.address(), fake(Some("he-IL"))?.address()))
}
```

Turkish addresses put the building number after the street name. The second line starts with a five-digit postcode, followed by a city from the 81-entry catalog. Street names use the pinned given-name and surname weights. Use `cityName()` to sample the city catalog directly, including through `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn turkish_address_sample() -> DatabaseResult<(String, &'static str)> {
    let faker = fake(Some("tr-TR"))?;
    Ok((faker.address(), faker.unique(false).cityName()?))
}
```

Russian addresses use six-digit postcodes and sample cities and regions independently. Ukrainian addresses use five-digit postcodes and keep the city and region paired. Use `cityAndRegion()` to sample the pair, including through `unique()`. Kazakh addresses start with a six-digit postcode whose first digit is zero and put the street suffix after the street name.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ukrainian_address_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("uk-UA"))?;
    Ok((faker.address(), faker.unique(false).cityAndRegion()?))
}
```

Polish postcodes use two digits, a hyphen, and three digits. Czech addresses may include a region line or a country line, and the building number can be omitted. Slovak postcodes use a space after the first three digits. The fixed Slovak address template emits the literal characters `\n` as separators.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn central_european_address_samples() -> DatabaseResult<(String, String, String)> {
    Ok((fake(Some("pl-PL"))?.address(), fake(Some("cs-CZ"))?.address(), fake(Some("sk-SK"))?.address()))
}
```

German, Austrian, and Swiss German addresses put the building number after the street name and the postcode before the city on the second line. Austria samples complete postcodes from its catalog. Swiss German street names can use a city stem. Use `streetSuffixShort()` and `streetSuffixLong()` for the regional suffixes. Swiss German `canton()` returns the abbreviation and German name from the same entry.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn swiss_german_address_sample() -> DatabaseResult<(String, (&'static str, &'static str))> {
    let faker = fake(Some("de-CH"))?;
    Ok((faker.address(), faker.unique(false).canton()?))
}
```

Dutch addresses in the Netherlands and Belgium join a surname and street suffix, followed by the building number. Both locales sample complete postcodes from their own catalogs. Belgian Dutch preserves a leading space before the postcode on the second line. `cityName()` and `state()` sample the local city and province catalogs.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn dutch_address_samples() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("nl-NL"))?.address(), fake(Some("nl-BE"))?.address()))
}
```

Norwegian and Swedish addresses put the building number after the street name. Letter suffixes on building numbers are uppercase. Norwegian postcodes use four digits and may start with zero. Swedish postcodes use five digits with an optional space after the third digit. `streetSuffixWord()` samples each locale’s standalone street suffix and supports `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn norwegian_and_swedish_address_samples() -> DatabaseResult<(String, String)> {
    Ok((fake(Some("nb-NO"))?.address(), fake(Some("sv-SE"))?.address()))
}
```

Danish addresses can append a floor and building side. Use `buildingLevel()`, `buildingSide()`, and `kommune()` for the individual fields, including through `unique()`. Finnish addresses can include a three-digit apartment number and a region. Icelandic addresses use a three-digit postcode and can put an uppercase letter after the building number.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nordic_address_samples() -> DatabaseResult<(String, String, String)> {
    Ok((fake(Some("da-DK"))?.address(), fake(Some("fi-FI"))?.address(), fake(Some("is-IS"))?.address()))
}
```

The Spanish Address providers cover Spain, Argentina, Peru, and Venezuela. Argentina uses province abbreviations in complete addresses. Peru preserves the literal ` # ` before the building number and the final space when the secondary address is absent. Venezuela can omit the postcode. The four locales share the country and autonomous community catalogs inherited from Spain. Use `community()` to sample a community, including through `unique()`.

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

Brazilian addresses place the postcode first and include the state abbreviation on the second line. Use `region()` and `regionAbbr()` for the five Brazilian regions, including through `unique()`. Portuguese addresses use the pinned city list, four- or seven-digit postcodes, and optional building and floor details.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn portuguese_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("pt-BR"))?.address(), fake(Some("pt-PT"))?.address()])
}
```

Italian addresses include a five-digit postcode and province fields. The province name and abbreviation are sampled independently, as in upstream Faker. Swiss Italian addresses use four-digit postcodes, a fixed city list, and may use a lowercase letter in the building number. `canton()` returns the canton abbreviation and Italian name as a pair.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn italian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("it-IT"))?.address(), fake(Some("it-CH"))?.address()])
}
```

Greek addresses preserve two postcode formats, optional hyphenated building numbers, and the prefecture catalog available through `prefecture()`. Street names sample the masculine and feminine surname forms. Cypriot Greek addresses use four-digit postcodes and one of four city names. Their upstream street suffix is absent and appears as an empty string in Rust.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn greek_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("el-GR"))?.address(), fake(Some("el-CY"))?.address()])
}
```

Romanian and Moldovan Romanian addresses preserve four street layouts, including optional block, stairwell, floor, and apartment fields. Romanian addresses include a county and a six-digit postcode. Moldovan Romanian addresses use a four-digit postcode. The numeric floor and apartment fields can contain a leading zero. Use `block()`, `blockSegment()`, `floor()`, and `apartmentNumber()` to sample these fields individually. `streetPlainName()` returns a street name without its prefix, and `county()` samples a Romanian county.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn romanian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("ro-RO"))?.address(), fake(Some("ro-MD"))?.address()])
}
```

Croatian addresses sample street and city names from local catalogs and use five-digit postcodes. The Croatian `street()` formatter also exposes the street catalog. Slovenian postcodes have four digits and end in zero. Slovenian full addresses preserve the two literal `\n` separators from upstream Faker.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn croatian_slovenian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("hr-HR"))?.address(), fake(Some("sl-SI"))?.address()])
}
```

Lithuanian addresses use the `LT-` postcode prefix and three address layouts, including one with a hyphenated building and unit number. Use `municipality()` to sample a Lithuanian municipality. Latvian addresses begin with an `LV-` postcode followed by the region and city. Both locales expose their local region label through `regionSuffix()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn lithuanian_latvian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("lt-LT"))?.address(), fake(Some("lv-LV"))?.address()])
}
```

Armenian addresses place the region first and end with a four-digit postcode beginning with `00`. Georgian addresses preserve eight layouts with optional country and region fields. Use `regionGenitiveForm()` to sample the Georgian region name used with the region suffix.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn armenian_georgian_address_samples() -> DatabaseResult<[String; 2]> {
    Ok([fake(Some("hy-AM"))?.address(), fake(Some("ka-GE"))?.address()])
}
```

Serbian addresses use Cyrillic for `sr-RS` and `sr-Cyrl-RS`, and Latin for `sr-Latn-RS`. Montenegrin addresses use their own city and street catalogs. The Montenegrin street prefix is empty, so the prefixed street template preserves a leading space.

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

Hungarian addresses begin with a four-digit postcode and use weighted capital, large city, and smaller city templates. Use `capital()`, `bigCity()`, or `smallerCity()` to select a tier explicitly. For locales without a upstream Faker Address provider, `address()` uses the US address layout and address catalogs. Person fields within the address still use the requested locale when its Person provider exists.

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

OpenRust exercises all 71 local Address providers from the pinned upstream Faker version. Address samples follow the local format and catalogs. Rust and PHP use independent random streams, so the same seed can produce different sample values.

### Given Names by Gender {#given-names-by-gender}

Use `firstNameMale()` or `firstNameFemale()` to draw from the corresponding upstream Faker catalog. `firstNameWithGender(Some("male"))` and `nameWithGender(Some("female"))` select a catalog explicitly. Pass `None` to sample both groups. Unrecognized group strings follow the same rule. Full names use the regional templates and existing surname data. Traditional Chinese given names combine one or two sampled characters. Missing region catalogs use English given names and templates.

With `en-US`, `fr-FR`, `it-IT`, and `ja-JP`, `name()`, `firstName()`, and `lastName()` use the pinned Person templates and catalogs. The Company templates use the same surnames. `firstNameWithGender` and `nameWithGender` share their corresponding `unique` history with the no-argument methods.

The same Person behavior also applies to `ar-SA`, `de-DE`, `fa-IR`, `nl-NL`, `pt-BR`, `pt-PT`, `tr-TR`, `zh-Hans`, and `zh-Hant`. Dutch Person and Company formatters share the pinned surname categories, weighted 1:10:5:10 for long, common Dutch, common foreign, and general Dutch surnames.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_name_sample() -> DatabaseResult<String> {
    fake(Some("nl-NL"))?.unique(false).name()
}
```

### Nordic Names {#nordic-names}

Danish and Icelandic templates can include a middle name. Use `middleName()` with `da-DK` or `is-IS`. In Icelandic, `lastName()` returns the patronymic stem, and `lastNameMale()` and `lastNameFemale()` append the respective endings. Finnish names may include a local honorific. Swedish and Norwegian Bokmål names use a given name followed by a surname.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nordic_name_sample() -> DatabaseResult<String> {
    fake(Some("is-IS"))?.unique(false).nameWithGender(Some("female"))
}
```

### Polish and Lithuanian Names {#polish-and-lithuanian-names}

Use `lastNameWithGender(Some("male"))` or `lastNameWithGender(Some("female"))` with `pl-PL` and `lt-LT` to select a surname catalog. `lastNameMale()` and `lastNameFemale()` sample the same catalogs. In the pinned Lithuanian provider, the no-argument `lastName()` samples given names, while full names use gendered surnames. `lastNameWithGender` and `lastName` share uniqueness history.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn gendered_surname_sample() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).lastNameWithGender(Some("female"))
}
```

### British, South African, Cypriot, and Regional French and Italian Names {#additional-regional-names}

The Person catalogs for `en-GB`, `en-ZA`, `el-CY`, `fr-BE`, `fr-CA`, `fr-CH`, and `it-CH` provide local first names, surnames, and effective full-name templates. Swiss French and Italian inherit their respective full-name templates and honorifics with Swiss name lists. Person and Company share the surname catalogs.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn regional_person_sample() -> DatabaseResult<String> {
    fake(Some("fr-CH"))?.unique(false).nameWithGender(Some("female"))
}
```

### Spanish-Language Names {#spanish-language-names}

The Person catalogs for `es-AR`, `es-ES`, `es-PE`, and `es-VE` use the pinned given names, surnames, and honorifics. Full names preserve each locale’s optional name suffixes and template weights. Venezuelan templates can combine multiple given names or surnames. Company names share the surname catalogs.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn spanish_name_sample() -> DatabaseResult<String> {
    fake(Some("es-VE"))?.unique(false).nameWithGender(Some("female"))
}
```

### Central European Names {#central-european-names}

The `de-AT`, `de-CH`, `cs-CZ`, `sk-SK`, `hu-HU`, and `hr-HR` name formatters use the pinned Person templates and share surname catalogs with Company. Czech and Slovak surnames support `lastNameWithGender`, `lastNameMale`, and `lastNameFemale`. Hungarian templates preserve surname-first order and married-name forms. Use `firstNameMaleNe()` and `lastNameFemaleMarried()` to sample those forms directly.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn central_european_name_sample() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).lastNameWithGender(Some("female"))
}
```

### Greek, Bulgarian, and Slovenian Names {#balkan-names}

The `el-GR` and `bg-BG` providers use gender-specific surname catalogs and full-name templates. Use `lastNameWithGender`, `lastNameMale`, or `lastNameFemale` to select the catalog. In `sl-SI`, both surname methods use the common catalog. Slovenian `title(gender)` uses the same local catalog for any gender, while `titleMale()` and `titleFemale()` use the inherited title lists.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn balkan_name_sample() -> DatabaseResult<String> {
    fake(Some("bg-BG"))?.unique(false).nameWithGender(Some("female"))
}
```

### Egyptian and West Asian Names {#egyptian-and-west-asian-names}

The `ar-EG`, `ar-JO`, `he-IL`, `hy-AM`, and `ka-GE` Person providers use the pinned regional first names, surnames, and honorifics. Egyptian full-name templates can include an abbreviated prefix. Armenian and Georgian full names follow their given-name-and-surname templates.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn west_asian_name_sample() -> DatabaseResult<String> {
    fake(Some("ka-GE"))?.unique(false).name()
}
```

### Russian, Ukrainian, and Kazakh Names {#russian-ukrainian-and-kazakh-names}

The `ru-RU`, `uk-UA`, and `kk-KZ` Person providers use pinned regional name catalogs and full-name templates. With Russian and Ukrainian, `middleName()` samples both patronymic groups, while `middleNameWithGender`, `middleNameMale()`, and `middleNameFemale()` select a group. Russian surnames also support gender selection and keep the fixed feminine ending. Kazakh PHP uses numeric gender constants. Rust uses the shared `"male"` and `"female"` selectors to select the corresponding templates.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn patronymic_sample() -> DatabaseResult<&'static str> {
    fake(Some("uk-UA"))?.unique(false).middleNameWithGender(Some("female"))
}
```

### South and Southeast Asian and Korean Names {#south-southeast-asian-and-korean-names}

The Person catalogs for `bn-BD`, `id-ID`, `ms-MY`, `ko-KR`, `th-TH`, and `vi-VN` use local spellings and full-name templates. Indonesian surnames support gender selection. Korean full names put the surname first and omit the space. Vietnamese `middleName()` draws from the given-name catalogs, while `middleNameWithGender(Some("male"))` and its female counterpart use the middle-name catalogs. Malaysian `name()` uses Malay, Chinese, and Indian templates. Malaysian `firstName()` also includes Christian given names. Its explicit male and female first-name formatters inherit `John` and `Jane` from the pinned provider.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn vietnamese_middle_name_sample() -> DatabaseResult<&'static str> {
    fake(Some("vi-VN"))?.unique(false).middleNameWithGender(Some("female"))
}
```

The `ms-MY` provider also exposes its template parts directly. Use `firstNameMaleMalay`, `firstNameFemaleMalay`, `lastNameMalay`, `muhammadName`, `nurName`, `haji`, `hajjah`, and `titleMaleMalay` for Malay name components. The other formatters are `lastNameChinese`, `firstNameMaleChinese`, `firstNameFemaleChinese`, `firstNameMaleChristian`, `firstNameFemaleChristian`, `initialIndian`, `firstNameMaleIndian`, `firstNameFemaleIndian`, and `lastNameIndian`. Optional components can be empty and preserve their trailing space when present.

`myKadNumber(gender, formatted, separator)` generates sample data. It emits 12 digits when `formatted` is false. When true, it inserts a hyphen or the supplied separator between the six-, two-, and four-digit groups. The last digit is odd for `"male"`, even for `"female"`, and unrestricted otherwise. The month and day are sampled independently. All these formatters support `unique()` and return an error outside `ms-MY`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn malaysian_person_sample() -> DatabaseResult<(String, String)> {
    let faker = fake(Some("ms-MY"))?;
    let unique = faker.unique(false);
    Ok((unique.firstNameMaleMalay()?, unique.myKadNumber(Some("male"), true, None)?))
}
```

### Romanian and Serbian Names {#romanian-and-serbian-names}

The `ro-RO` and `ro-MD` providers share Romanian name and title catalogs. `sr-RS` and `sr-Cyrl-RS` use separate Cyrillic catalogs, while `sr-Latn-RS` uses its Latin catalog.

Use `cnp(gender, dateOfBirth, county, isResident)` with `ro-RO` to generate a sample 13-digit identifier. The optional birth date accepts `YYYY`, `YYYY-MM`, or `YYYY-MM-DD` within 1800–2099. A partial date draws missing fields from a random day in that year. When the sampled day does not exist in the supplied month, the call returns an error. County codes are case-sensitive. The method calculates the checksum and supports `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn romanian_cnp_sample() -> DatabaseResult<String> {
    fake(Some("ro-RO"))?.unique(false).cnp(Some("female"), Some("1981-06-16"), Some("B1"), true)
}
```

### Estonian, Latvian, and Montenegrin Names {#estonian-latvian-and-montenegrin-names}

The `et-EE`, `lv-LV`, and `me-ME` providers use their own given names and surnames with the base given-name-first template. Latvian `driverLicence()` emits two lower-case ASCII letters followed by six digits. Its passport formatter uses seven digits. Use `personalIdentityNumberWithBirthdate` to supply a civil birth date. These identity formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn latvian_driver_licence() -> DatabaseResult<String> {
    fake(Some("lv-LV"))?.unique(false).driverLicence()
}
```

### Indian English and Nepali Names {#indian-english-and-nepali-names}

The `en-IN` and `ne-NP` providers use regional given names, surnames, and full-name templates. Indian English exposes `middleNameMale()`. Nepali exposes both `middleNameMale()` and `middleNameFemale()`. Use these methods directly or pass the corresponding gender to `middleNameWithGender`. The no-argument `middleName()` returns an unsupported-locale error for these providers. The gendered methods support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn nepali_middle_name() -> DatabaseResult<&'static str> {
    fake(Some("ne-NP"))?.unique(false).middleNameFemale()
}
```

### Nigerian and Ugandan English Names {#nigerian-and-ugandan-english-names}

The `en-NG` and `en-UG` providers use regional names and templates. Nigerian full names can include one or two given names, followed by a surname. Ugandan templates can put either the given name or the surname first. Ugandan `lastName()` samples the general surname catalog. Use `lastNameMale()`, `lastNameFemale()`, or `lastNameWithGender` with an explicit gender for the corresponding catalog. The formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn ugandan_surname() -> DatabaseResult<String> {
    fake(Some("en-UG"))?.unique(false).lastNameFemale()
}
```

### Mongolian Names {#mongolian-names}

For `mn-MN`, full names use one Cyrillic initial, a period, and a given name without an intervening space. `namePrefix()` samples the set of 30 initials. `alphabet()` samples the separate set of 35 letters used by `idNumber()`. Both return a Rust `char` and support `unique()`. The standalone surname formatter inherits `Doe` from the base provider.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn mongolian_initial() -> DatabaseResult<char> {
    fake(Some("mn-MN"))?.unique(false).namePrefix()
}
```

### Singapore English Names and Identity Samples {#singapore-english-names-and-identity-samples}

The `en-SG` provider inherits the base names `John Doe` and `Jane Doe`. Use `nric(birthDate)` or `fin(issueDate)` to generate a sample identifier, or `singaporeId(issueDate, foreigner)` to select the type explicitly. Dates are optional civil dates. Omitting the date uses the same date range as `dateTimeThisCentury()`. The prefix changes at 2000. NRICs use the year’s last two digits from 1968 onward and sample `00` or `01` for earlier dates. Each formatter calculates the final checksum letter and supports `unique()`.

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn singapore_identity_sample() -> DatabaseResult<String> {
    fake(Some("en-SG"))?.unique(false).nric(NaiveDate::from_ymd_opt(2000, 1, 1))
}
```

### Belgian Dutch Names and RRN Samples {#belgian-dutch-names-and-rrn-samples}

The `nl-BE` provider uses Belgian Dutch given-name and surname catalogs with the base given-name-first template. `rrn(gender)` generates an 11-digit sample from a birth date drawn by `dateTimeThisCentury()`. The three-digit serial is odd for `"male"`, even for `"female"`, and unrestricted otherwise. The final two digits are a modulo-97 checksum that accounts for birth years from 2000 onward. The formatter supports `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn belgian_rrn_sample() -> DatabaseResult<String> {
    fake(Some("nl-BE"))?.unique(false).rrn(Some("female"))
}
```

### Person Name Prefixes and Suffixes {#person-name-prefixes-and-suffixes}

Use `prefix()` and `suffix()` with a locale that provides those formatters. The default `en-US` provider supports `suffix()` and uses the same suffix catalog in full names. Italian `it-IT` and `it-CH` return `None` for the upstream null. Armenian and Taiwanese Mandarin return `Some("")`. Unsupported locales return an error. Both methods support `unique()`, including the null or empty result once per uniqueness history.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_affixes() -> DatabaseResult<(&'static str, Option<&'static str>)> {
    Ok((fake(Some("fr-FR"))?.prefix()?, fake(None)?.suffix()?))
}
```

### Japanese Kana Names {#japanese-kana-names}

Use `kanaName(gender)` with `ja-JP` to generate a katakana surname followed by a space and a given name. `firstKanaName(gender)` returns a given name, and `lastKanaName()` returns a surname. Pass `Some("male")`, `Some("female")`, or `None` for either given-name catalog. `firstKanaNameMale()` and `firstKanaNameFemale()` select a catalog directly. All five formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn japanese_kana_name() -> DatabaseResult<String> {
    fake(Some("ja-JP"))?.unique(false).kanaName(Some("female"))
}
```

### Swiss AHV and AVS Samples {#swiss-ahv-and-avs-samples}

Use `avs13()` with `de-CH`, `fr-CH`, or `it-CH` to generate a Swiss social security sample in the form `756.####.####.##`. Swiss German also provides `ahv13()`, with the same format and EAN check digit. Both formatter names support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn swiss_avs_sample() -> DatabaseResult<String> {
    fake(Some("fr-CH"))?.unique(false).avs13()
}
```

### Czech Birth Numbers and Danish CPR {#czech-birth-numbers-and-danish-cpr}

Use `birthNumber(gender, minAge, maxAge, slashProbability)` with `cs-CZ` to sample a Czech birth number. The numeric arguments specify age bounds and the percent chance of a slash. `birthNumberMale()` and `birthNumberFemale()` use ages 0 to 100 and a 50% slash probability. The formatter applies the female month offset, the additional month offset available from 2004, and the check digit from 1954. Inverted age bounds return an error. With `da-DK`, `cpr()` samples a civil birth date and a four-digit serial whose first digit is nonzero. Both providers support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn czech_birth_number() -> DatabaseResult<String> {
    fake(Some("cs-CZ"))?.unique(false).birthNumber(Some("female"), 18, 65, 100)
}
```

### French NIR and Italian Tax ID Samples {#french-nir-and-italian-tax-id-samples}

Use `nir(gender, formatted)` with `fr-FR` or `fr-CH`. Pass `Some("male")` or `Some("female")` to select the first digit, or `None` to sample it. The NIR uses the French department catalog and a modulo-97 check key. The compact form has 15 characters. The formatted form separates the same characters as 1–2–2–2–3–3–2. Use `taxId()` with `it-IT` or `it-CH` for the uppercase 16-character sample mask used by upstream Faker. Both formatters support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn french_nir_sample() -> DatabaseResult<String> {
    fake(Some("fr-FR"))?.unique(false).nir(Some("female"), true)
}
```

### Iranian National Codes and Indonesian NIK Samples {#iranian-national-codes-and-indonesian-nik-samples}

Use `nationalCode()` with `fa-IR` to generate a ten-digit sample with a nonzero area code and a modulo-11 check digit. Use `nik(gender, birthDate)` with `id-ID` to generate a 16-digit sample. It starts with a code from the pinned birthplace catalog and includes the birth date. For female samples the day is increased by 40. Pass `None` for the date to sample from the past 30 years, or pass a `NaiveDate` to choose a civil date. Both formatters support `unique()`.

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn indonesian_nik_sample() -> DatabaseResult<String> {
    fake(Some("id-ID"))?.unique(false).nik(Some("female"), NaiveDate::from_ymd_opt(2000, 2, 29))
}
```

### Kazakh IIN and Venezuelan National ID Samples {#kazakh-iin-and-venezuelan-national-id-samples}

Use `individualIdentificationNumber(birthDate, gender)` with `kk-KZ`. Birth dates are optional civil dates in 1800–2099. The default is a date from the past 30 years and male gender. `Faker::checkSum(prefix)` takes eleven ASCII digits and returns the control value. When both checksum passes yield 10, the generator retries with another serial. Use `nationalId(separator)` with `es-VE` to select the separator after the `V` or `E` prefix. Both generators support `unique()`.

```rust
use chrono::NaiveDate;
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn kazakh_iin_sample() -> DatabaseResult<String> {
    fake(Some("kk-KZ"))?.unique(false)
        .individualIdentificationNumber(NaiveDate::from_ymd_opt(2000, 2, 29), Some("female"))
}
```

For Venezuelan RIF samples, use `taxpayerIdentificationNumber()` with `es-VE`. The format has one of six prefixes (`J`, `G`, `V`, `E`, `P`, `C`), eight body digits and a final random digit. Use `taxpayerIdentificationNumberWithSeparator(Some("-"))` to insert a separator on both sides of the body. Both calls share the `unique()` formatter history.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn venezuelan_rif_sample() -> DatabaseResult<String> {
    fake(Some("es-VE"))?
        .unique(false)
        .taxpayerIdentificationNumberWithSeparator(Some("-"))
}
```

### Portuguese NIF and Taiwanese Name Pools {#portuguese-nif-and-taiwanese-name-pools}

Use `taxpayerIdentificationNumber()` with `pt-PT` for a nine-digit NIF sample. `Faker::dvCalcMod11(number)` computes the check digit from ASCII digits, preserving leading zeros in the input. With `zh-TW`, `randomName(pool, count)` concatenates independent draws from a slice of strings. Entries can repeat. An empty pool or a count of zero yields an empty string. Both generators support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn taiwanese_name_sample() -> DatabaseResult<String> {
    fake(Some("zh-TW"))?.randomName(&["明", "安"], 2)
}
```

### Baltic Driving Licence and Passport Samples {#baltic-driving-licence-and-passport-samples}

Use `driverLicence()` and `passportNumber()` with `lt-LT` or `lv-LV`. Lithuanian samples for both methods contain eight ASCII digits and can start with zero. Latvian samples start with two lower-case ASCII letters followed by six digits for a driving licence or seven digits for a passport. Both methods support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn baltic_driver_licence() -> DatabaseResult<String> {
    fake(Some("lt-LT"))?.unique(false).driverLicence()
}
```

### Malaysian registration plates {#malaysian-registration-plates}

Use `jpjNumberPlate()` with `ms-MY` to sample Malaysian registration plates. The weighted prefix catalogs, mixed-case special prefixes, and optional empty letters follow the pinned upstream Faker provider. The provider also exposes `peninsularPrefix()`, `sarawakPrefix()`, `sabahPrefix()`, `specialPrefix()`, `validAlphabet()`, and `numberSequence()`. All support `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn malaysian_plate_sample() -> DatabaseResult<String> {
    fake(Some("ms-MY"))?.unique(false).jpjNumberPlate()
}
```

### Polish registration plates {#polish-registration-plates}

Use `licensePlate()` with `pl-PL` for ordinary Polish plates. `licensePlateWithOptions(special, voivodeships, counties)` also accepts province and county names. When you enable special plates, include `army` or `services` in the province filter to select military or agency plates. A filter that matches no available name falls back to the available choices at that level. OpenRust selects the provider by locale. upstream Faker requires registering the Polish LicensePlate provider explicitly. Both Rust methods share the same `unique()` history.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn polish_plate_sample() -> DatabaseResult<String> {
    fake(Some("pl-PL"))?.unique(false).licensePlateWithOptions(
        false, Some(&["dolnośląskie"]), Some(&["Wrocław"]),
    )
}
```

### Person Locale Fallback {#person-locale-fallback}

When a locale has no Person provider, name formatters, `suffix()`, and `ssn()` use the `en-US` Person provider. A locale with its own Person provider uses that provider’s formatters, including the ones it inherits. For example, `en-CA` falls back to the US Person provider, while `en-SG` has its own provider and returns an unsupported-locale error for `ssn()`. The Person fallback applies only to Person formatters.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn person_fallback_sample() -> DatabaseResult<String> {
    fake(Some("en-CA"))?.unique(false).ssn()
}
```

### Company slogans and business phrases {#company-slogans-and-business-phrases}

Use `catchPhrase()` for company slogans and `bs()` for business phrases. Both use the selected Company provider. A locale without a Company provider falls back to `en-US`. If a locale has its own provider but it lacks the formatter, the call returns an error. For `ar-SA`, `ar-JO`, and `hy-AM`, `bs()` returns an empty string. Armenian `catchPhrase()` also returns an empty string. In `zh-TW`, `bs()` joins words without spaces.

French and Czech Company providers also expose `catchPhraseNoun()`, `catchPhraseVerb()`, and `catchPhraseAttribute()`. This includes the French providers for Belgium, Canada, and Switzerland. French slogans filter out repeated occurrences of `sécurité` and `simpl`. All five formatters work with `unique()`.

```rust
use openrust::database::{DatabaseResult, eloquent::factories::fake};

fn company_phrase_sample() -> DatabaseResult<(String, String)> {
    let french = fake(Some("fr-CA"))?;
    let taiwanese = fake(Some("zh-TW"))?;
    Ok((french.unique(false).catchPhrase()?, taiwanese.bs()?))
}
```

### Coordinates {#coordinate-samples}

Use `latitude(None, None)?` and `longitude(None, None)?` for signed decimal degrees, rounded to six decimal places. Pass `Some(value)` for either bound. The defaults are −90 to 90 and −180 to 180, respectively. Reversed bounds are accepted and non-finite bounds return an error. `localCoordinates()?` returns a serializable `Coordinates` with `latitude` and `longitude` fields. It uses the pinned Hungarian and Montenegrin ranges, with global ranges elsewhere. For Taiwan bounds, use the separate `localLatitude()?` and `localLongitude()?` methods with `zh_TW`. All five methods support `unique`.

### Soft-deleted State {#trashed-state}

Use `trashed(None)?` on a factory or counted batch for a model configured with `softDeletes`. It appends the model’s deletion column as a state, using the previous calendar day in the selected connection’s application time zone. Use `Some(value)` for an explicit deletion date. Later states and explicit creation attributes can override it. Unsupported models return an error. The factory creates a new model in this state through its ordinary creation lifecycle.

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

### Conditional States {#conditional-states}

Use `when(condition, transform)` to transform a factory when a boolean is true, or `unless(condition, transform)` when it is false. `whenElse` and `unlessElse` take a fallback transformation as the third argument. The selected closure runs once when configuring the factory and returns it for chaining. These methods work before or after `count`.

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

## Creating Models Using Factories

Apply `state` in declaration order. Each `sequence` starts at zero and retains its index across operations on the same builder. A fresh builder starts again at zero. Concurrent operations receive distinct indices, with assignment following execution order. `make` builds the model and runs `afterMaking`. `create` saves it, creates its declared children, then runs `afterCreating`. Calling `count` switches the result to an Eloquent collection. Factory attributes bypass mass-assignment filtering and still pass through the model’s setters and casts.

Use `prependState` to run a transformation before all currently registered states. Each transformation sees the attributes produced so far. Use `set(name, value)` to append a single attribute override. Both work before or after `count`. Attributes passed directly to a creation method are applied after the registered states.

For a counted batch, all models are constructed before `afterMaking` runs. `create` then saves each model and its declared relations. The batch runs `afterCreating` after those writes finish. If `afterMaking` fails, the batch itself has yet to be saved. Owner factories resolved during construction may already have written records.

Use `raw()` or `rawOn(connection)` to get expanded attributes before model setters and casts. A counted factory returns a vector of attribute maps. Raw expansion skips the target factory’s callbacks and child relationships. It can still persist a declared parent to resolve its key. Use `createQuietly()` or `createQuietlyOn(connection)` to persist models and relationships while suppressing Eloquent model events. Factory callbacks still run.

Use `rawWith(attributes)` or `rawWithState(callback)` to append a state for one call. The callback receives the accumulated attributes for each row before relationship keys are resolved. A foreign-key override, including null, skips resolution for that key. Use the corresponding `On` methods to select a connection.

Use `rawWithParent(parent, callback)` or `rawWithParentOn(connection, parent, callback)` on either builder to supply a borrowed parent. Matching `stateWithParent` callbacks and the final callback can read it. Supplying the parent adds no relationship attributes by itself.

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

Use `makeWithState`, `createWithState`, or `createQuietlyWithState` to apply a final state for one operation. Counted factories also accept an attribute map through `makeWith`, `createWith`, or `createQuietlyWith`. For one model, the map forms are `makeOne`, `createOne`, and `createOneQuietly`. The corresponding `On` forms select a connection.

The callback receives the attributes accumulated for each model before deferred attributes and parent keys are resolved. The final state applies only to that call.

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

Use `createOne(attributes)` to create one model even on a counted factory. Use `createMany(records)` to create records one at a time, in order. Pass `FactoryRecords::Default` for the configured count (one on an uncounted factory), `FactoryRecords::Count(n)` for an explicit count, or `FactoryRecords::Attributes(vec![…])` for per-record overrides. The overrides apply after factory states. Each record runs both factory callbacks before the next one starts. If a later record fails, earlier writes can remain. Use `createOneQuietly` and `createManyQuietly` to suppress model events, or the corresponding `On` methods to select a connection.

Use `makeOne(attributes)` to build one unsaved model, or `makeMany(records)` to build unsaved models in order. Both run `afterMaking` and accept explicit attributes. Use the same `FactoryRecords` variants with `makeMany`. A zero count yields an empty collection. Both `makeMany` and `createMany` consume the configured default count. This also applies to explicit input, zero records, and errors during model generation. Subsequent calls default to one model, and Rust batch methods continue to return collection-shaped results. The make methods leave the returned models unsaved. Resolving a declared parent can still write that parent. The `On` variants take a connection. `lazy(attributes)` returns an async callback. Await a call to create the model or counted batch using the connection resolved at that time. Calling it again creates a fresh result.

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

`makeOne` leaves the configured batch count available for the next `makeMany` call. In the example, the two default batch calls return two models and then one. Use a separate counted factory for `lazy` when the preview has consumed its count. Preparing or invoking the callback leaves the target table untouched until the returned future is awaited. Each awaited invocation on the factory below writes two new models. A `lazy` callback from an uncounted factory returns one model per successful call. The example checks that per-model overrides preserve input order and that both forms of the callback return saved models.

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
    let pending_batch = seed();

    let single = factory();
    let seed_one = single.lazy(Attributes::new());
    let pending_one = seed_one();

    let created = pending_batch.await?;
    assert_eq!(created.len(), 2);
    assert!(created.iter().all(|model| model.state().exists()));
    assert_eq!(seed().await?.len(), 2);
    assert!(pending_one.await?.state().exists());
    assert!(seed_one().await?.state().exists());
    Ok(())
}
```

Factories suspend default relationship autoloading during model construction. Factory batches, including `makeMany` and `createMany`, leave it disabled on the result. Counted `make` and `create` still use the model’s custom collection keys. To opt in for that batch, call `withRelationshipAutoloading()`. Later queries still use the model’s configured default.

```rust
use openrust::database::eloquent::{Collection, Model};

fn factory_relationship_loading_sample<M: Model>(models: Collection<M>) -> Collection<M> {
    models.withRelationshipAutoloading()
}
```

Use `forEachSequence(states)` to apply one attribute map per model and set the batch count to the number of maps. `crossJoinSequence(sequences)` combines the maps from each input sequence in order, with the last sequence changing fastest. Later maps override earlier maps on matching keys. Set the count separately to select how many combinations to use. The sequence repeats if the count exceeds the number of combinations. An empty input sequence returns an error.

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

Use `sequenceWithState` to read the zero-based index and the attributes assembled so far. `sequenceWithParent` also receives an optional reference to a typed parent supplied by the enclosing relationship or an explicit parent entry point. Both run at their declaration position, after earlier states and before deferred attributes and late overrides. They keep the configured batch size. Each sequence advances across operations on the same factory. An explicit parent supplies context. Set the foreign key separately when needed.

```rust
use openrust::database::{
    Connection, DatabaseResult, Value,
    eloquent::{Attributes, Model, factories::Factory},
};

async fn contextual_sequence<M: Model, P: Model>(
    connection: &Connection,
    parent: &P,
) -> DatabaseResult<()> {
    let batch = Factory::<M>::new(Attributes::new)
        .set("label", "earlier")
        .count(2)
        .sequenceWithState(|index, attributes| {
            assert_eq!(attributes["label"], Value::from("earlier"));
            Attributes::from([("label".into(), format!("item-{index}").into())])
        })
        .sequenceWithParent::<P>(|_, attributes, parent| {
            let suffix = if parent.is_some() { "parent" } else { "standalone" };
            let Value::Text(label) = &attributes["label"] else {
                panic!("earlier sequence must set a label");
            };
            Attributes::from([("label".into(), format!("{label}-{suffix}").into())])
        });

    let models = batch
        .createWithParentOn(connection, parent, Attributes::new())
        .await?;
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].getAttribute("label"), Some(&"item-0-parent".into()));
    assert_eq!(models[1].getAttribute("label"), Some(&"item-1-parent".into()));
    Ok(())
}
```

Use `insert(attributes)` on a single or counted factory to bulk-insert the generated rows, or `insertOn(connection, attributes)` to select the connection explicitly. The factory runs `afterMaking` and resolves any required parent keys. The bulk write skips target model persistence events, child creation and `afterCreating`. Before writing, the factory projects attributes through read accessors and serialization casts, includes hidden attributes subject to the visible allowlist, and skips appended attributes. The insert builder then reapplies setters and write casts. Rust keeps typed database values during this projection, including binary data.

```rust
use openrust::database::{DatabaseResult, eloquent::{Attributes, Model, factories::Factory}};

async fn insert_projected<M: Model>(factory: Factory<M>) -> DatabaseResult<()> {
    factory.count(2).insert(Attributes::new()).await
}
```

Use `makeWithParent(parent, attributes)` and `insertWithParent(parent, attributes)` to supply the parent to matching `stateWithParent` callbacks on a single or counted factory. The `On` variants accept an explicit connection. The parent is state context. Set the foreign key in the state or call attributes when the row should reference it.

Use `createWithParent(parent, attributes)`, `createQuietlyWithParent(parent, attributes)`, or `lazyWithParent(parent, attributes)` on a single or counted factory. The borrowed parent is available to matching `stateWithParent` and `afterCreatingWithParent` callbacks. Use the `On` variants for immediate creation on a particular connection. The lazy callback uses the connection resolved when awaited. The parent alone adds no foreign key.

Use `makeWithParentState`, `createWithParentState`, or `createQuietlyWithParentState` when the final state needs both the accumulated attributes and the borrowed parent. These methods support single and counted factories, with `On` forms for an explicit connection. The callback runs before deferred attributes and owner keys are resolved.

```rust
use openrust::database::{DatabaseResult, Value, eloquent::{Attributes, Model, factories::Factory}};

async fn seed_with_parent_state<P: Model, C: Model>(parent: &P) -> DatabaseResult<()> {
    let children = Factory::<C>::new(|| {
        Attributes::from([("name".into(), "Flight".into())])
    }).count(2);
    let named = |attributes: &Attributes, parent: &P| {
        let (Some(Value::Text(prefix)), Some(Value::Text(name))) =
            (parent.getAttribute("name"), attributes.get("name"))
        else {
            return Attributes::new();
        };
        Attributes::from([("name".into(), format!("{prefix} {name}").into())])
    };

    let drafts = children.makeWithParentState(parent, named).await?;
    assert!(drafts.iter().all(|model| !model.state().exists()));
    let saved = children.createWithParentState(parent, named).await?;
    assert_eq!(saved.len(), 2);
    Ok(())
}
```

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

Use `withoutAfterMaking()` or `withoutAfterCreating()` on a single or counted factory to clear the corresponding callbacks already registered on that factory. You can register new callbacks afterward. The other callback phase and Eloquent model events still run. This includes callbacks added in the named factory’s `configure` hook.

Use `connection(Some("archive"))` to select a named connection on a factory or counted batch. `getConnectionName()` returns the configured override, and `connection(None)` clears it. Operations resolve the override first, then the model connection, then the current application or Seeder default. The `On` methods use the connection supplied for the target factory. Nested factories use their own override or model connection before inheriting the enclosing connection. Relationship connection checks still apply.

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

### Deferred Factory Attributes {#deferred-factory-attributes}

Use `attribute` to compute one value after factory states and explicit overrides have been applied. Register dependent attributes in evaluation order. Use `parentAttributeWith` to build a related factory from the current values. An explicit value for the same key takes precedence, including a null foreign key. Parent exclusion also applies to `parentAttributeWith`.

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

Use `modelAttribute("owner_id", &owner)` to capture an existing model’s primary key for a factory attribute. A model without a key yields null. Explicit attributes take precedence. During creation, after saving a model and before creating its children, the factory clears cached empty relation collections. Nonempty collections and single relations, including cached nulls, remain available.

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn from_owner<M: Model, R: Model>(owner: &R) -> Factory<M> {
    Factory::new(Attributes::new).modelAttribute("owner_id", owner)
}
```

## Factory Relationships

Counted factories can still configure states, sequences, callbacks, and relationships. `count(n)` replaces the pending size, and `forEachSequence(states)` sets it to the sequence length. Other configuration methods preserve the pending size. Pass a factory directly to `has` or `hasNamed` to create one child per parent. A counted child factory uses its explicit count, including zero. The single-child path uses the model’s construction callbacks and skips the batch collection hook.

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


### Parent Context for States and Callbacks {#factory-parent-context}

Use `stateWithParent::<Parent>` and `afterCreatingWithParent::<Parent>` when a child factory needs to read the model that creates it through `has` or `hasAttached`. The callback receives `Some(&Parent)` for the matching type and `None` for standalone creation or a different parent type. Construction callbacks registered with `afterMaking` still receive the child only.

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

Use `has` with `HasOne`, `HasMany`, `MorphOne` or `MorphMany`. Use `r#for` to generate a `BelongsTo` or `MorphTo` owner, or `forExisting` to associate an existing model. Each `for` declaration resolves its required owner once per pair of child and owner database connections. Repeated calls on the same builder reuse that owner. A fresh builder resolves its own owner. A `MorphTo` descriptor must include the owner target and its mapped alias. `hasAttached` accepts a `BelongsToMany` or `MorphToMany` descriptor and writes the pivot attributes. Writes are sequential. If a later relation fails, earlier writes can remain.

For `BelongsTo` and `MorphTo`, the last `r#for` or `forExisting` targeting a foreign key replaces the definition of that key. A state or explicit operation attribute can then replace it, including with `Value::Null`. In that case the owner is neither created nor associated. A `MorphTo` declaration still supplies its type as a separate default, which a state can also replace.

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

Child factories apply relation query defaults after the definition and before explicit states. Direct and polymorphic child relations then supply the foreign key and discriminator before `afterMaking`. Attached model factories also see the relation query defaults before their explicit states.

Use `hasNamed`, `forNamed`, or `hasAttachedNamed` to resolve a relationship by name. The factory uses a descriptor registered through `Model::namedRelations` or the application’s `resolveRelationUsing`. Pass the Rust descriptor type explicitly, for example `forNamed::<_, MorphTo<Comment>>("subject", posts)`. Unknown names and incompatible descriptor types return errors. Use `forExistingNamed` to associate an existing owner through a registered relationship.

Use `hasInferred` when a relationship follows the related Rust model’s name. It tries the plural camel-case name first and falls back to the singular when the plural is absent from the model’s registered relationships. `hasAttachedInferred` uses the plural name. `forInferred` and `forExistingInferred` use the owner’s camel-case name. The descriptor type is explicit. For a custom relationship name, use the corresponding named method.

Pass a child factory configured with `forEachSequence(child_states)` to `hasInferred` to apply one state per child. The sequence length determines how many children each parent creates, in the order supplied.

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, factories::Factory, relations::HasMany},
    seeder::SeederContext,
};

async fn seed_inferred_children<P: Model, C: Model>(
    context: &SeederContext<'_>,
    parents: Factory<P>,
    children: Factory<C>,
    child_states: Vec<Attributes>,
) -> DatabaseResult<()> {
    parents
        .count(2)
        .hasInferred::<_, HasMany<P, C>>(children.forEachSequence(child_states))
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

Use `parentAttribute("owner_id", owners)` to declare a foreign-key attribute backed by another factory. It expands to a saved model key and can reuse a matching recycled model. An explicit value from the definition, a state, or the operation takes precedence. `withoutParents(vec![])` disables all factory-valued attributes. Pass `FactoryParentSelector::attribute("owner_id")` or `FactoryParentSelector::model::<Owner>()` to disable specific ones. Disabled attributes expand to null. Explicit `for` and `forExisting` relationships still resolve.

The process-wide default starts enabled. `Factory::<Owner>::dontExpandRelationshipsByDefault()` disables expansion for factories built afterward, across model types. `expandRelationshipsByDefault()` or `flushState()` restores the default. Existing factories retain the setting they were built with. A later selective `withoutParents` call keeps full suppression in effect.

```rust
use openrust::database::eloquent::{Attributes, Model, factories::Factory};

fn factory_without_parent_expansion<Owner: Model>() -> Factory<Owner> {
    Factory::<Owner>::dontExpandRelationshipsByDefault();
    let factory = Factory::<Owner>::new(Attributes::new);
    Factory::<Owner>::expandRelationshipsByDefault();
    factory
}
```

After a successful `hasAttachedNamed` write, the factory reloads the named relationship if it was already loaded or if the pivot change touches the related models. The refreshed collection is available to `afterCreating` and includes custom Pivot models when configured.

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

### Recycling Existing Models {#recycling-existing-models}

Call `recycle` with a saved model or `recycleMany` with an iterable of saved models of one type. Call either method before or after `count`. The pool follows nested factory relationships. For each generated owner, the factory randomly selects a matching model of the requested type on the same connection, or creates one if the pool has no match.

Repeated calls can add models of different types to the same builder. An empty input adds no candidates. A declared `has` child still creates a new model and leaves any existing child in the pool with its original owner.

Use `getRandomRecycledModel::<Owner>()` on a single or batch builder to inspect its pool by model type. It returns a borrowed model or `None` without querying the database. Relation resolution still filters candidates by the owner connection.

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

Pass a vector of attribute maps to `hasAttached` or `hasAttachedNamed` to apply pivot attributes in related-model order. The sequence cycles when the related factory count exceeds its length. Use `FactoryPivot::resolve` to compute attributes for each attachment from the saved parent model. An attribute map applies the same values to all attachments. The pivot sequence also continues across parents and repeated calls on the same builder.

With a nonempty pivot sequence and an uncounted related factory, the sequence length determines the number of related models. An explicit `count(n)` takes precedence, including zero. Shared attributes and empty sequences default to one generated related model.

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

Pivot maps can include structured values. For JSON columns, pass `Value::Json` in the shared map or in the maps for individual attachments. To read the column through a custom Pivot model, include it in `withPivot` and declare its JSON cast.

```rust
use openrust::database::{
    Value,
    eloquent::{Attributes, Model, factories::Factory, relations::BelongsToMany},
};
use serde_json::json;

fn attach_json_pivots<P: Model, R: Model>(
    owners: Factory<P>,
    members: Factory<R>,
    relation: BelongsToMany<P, R>,
) -> Factory<P> {
    owners.hasAttached(
        relation.withPivot(["meta"]),
        members,
        vec![
            Attributes::from([("meta".into(), Value::Null)]),
            Attributes::from([("meta".into(), Value::Json(json!({"roles": ["editor"]})))]),
        ],
    )
}
```

Pass an existing model, a vector of models, or an Eloquent collection to attach them without creating new related rows. The models must be persisted, have the related key, and belong to the connection used by the relationship. Invalid targets fail before any pivot row for that attachment is written.
