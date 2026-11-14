# Eloquent: Factories

## Defining Model Factories

Use `Factory::new` to define the model’s default attributes. Implement `HasFactory` on the model if you want to call `Flight::factory()`. Generate a factory under `database/factories` with the command below, then declare its Rust module and supply the definition.

```sh
./artisan make:factory FlightFactory --model=Flight
```

### Localized Fake Data {#localized-fake-data}

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


Use `faker.unique(false)` to avoid duplicates per formatter. `unique(true)` clears that generator’s history. With `Factory::tryWithFaker`, a batch shares one generator, and an exhausted formatter returns an error before the batch is saved. Separate factory calls start with fresh history. For example, use `faker.unique(false).safeEmail()?` inside the definition.

Other built-in formatters include `firstName`, `lastName`, `freeEmail`, `userName`, `streetName`, `postcode`, `phoneNumber`, `company`, and `word`. The same methods are available through `unique`. The selected locale supplies sample data where fake-rs has it.

Use `numberBetween(first, second)` for an inclusive integer range in either order. `randomDigit()` returns 0 through 9, and `randomDigitNotNull()` returns 1 through 9. `dateTimeBetween(start, end)` takes explicit UTC date-times, samples at whole-second precision including both endpoints, and returns an error when the start second follows the end second. All four formatters support `unique`.

`randomDigitNot(except)` excludes the supplied digit. `randomNumber(digits, strict)` uses a random width from one to nine when given `None`. Strict mode requires exactly the selected width. Non-strict mode also accepts zero digits and returns zero. Explicit widths above nine and strict zero digits return errors. Both methods support `unique`.

`randomFloat(decimals, min, max)` samples between the bounds and then rounds to the requested decimal place, with ties away from zero. Use `None` to choose a precision from zero to nine or to generate an upper bound. Reversed bounds are accepted. Rounding can move the result outside the original interval. Non-finite bounds return an error. Like PHP 8.5, the formatter keeps the sampled value when positive precision exceeds 308 or rescaling would overflow. Precision below −308 yields zero. `unique` treats positive and negative zero as the same result.

Use `unixTime(max)` to sample Unix seconds between the epoch and the specified endpoint, or `dateTime(max)` to return a UTC date-time. With `None`, the endpoint is the current time. `date(format, max)` and `time(format, max)` accept Chrono format strings, default to `%Y-%m-%d` and `%H:%M:%S`, and return an error for invalid formats. `iso8601(max)` includes the effective time-zone offset. All five methods support `unique`.

Use `dateTimeInInterval(date, interval)` to sample whole seconds between a UTC instant and that instant plus a signed Chrono `TimeDelta`. A negative duration samples backward. `None` defaults to 30 years ago for the instant and five days forward for the duration. Use `dateTimeThisCentury(max)`, `dateTimeThisDecade(max)`, or `dateTimeThisMonth(max)` to start 100 years, 10 years, or one month before now. `dateTimeThisYear(max)` starts at midnight on January 1 in the application time zone. The other calendar lookbacks and the omitted start of `dateTimeInInterval` also use that time zone. When both interval inputs are omitted, the default five days are local calendar days. An explicit `TimeDelta` is elapsed time. All four use now when `max` is `None`, reject an end before the start, and normalize calendar overflow as FakerPHP does. All five methods support `unique`. Supply typed dates and durations in Rust. For an application-defined calendar interval, compute its endpoint and use `dateTimeBetween`.

Use `amPm(max)`, `dayOfMonth(max)`, `dayOfWeek(max)`, `month(max)`, `monthName(max)`, and `year(max)` to format parts of a sampled date in the effective time zone. The day and month numbers are zero-padded strings. Weekday and month names are English, matching FakerPHP’s date format. `dateTimeAD(max)` samples from FakerPHP’s 64-bit AD lower bound. These methods use the current time when `max` is `None`. `century()` samples Roman numerals from I through XXI. All support `unique`.

Use `timezone(countryCode)` to sample an IANA time-zone identifier. Pass `None` for the full list, or a country code such as `Some("TW")` to restrict the candidates. Country codes are case-sensitive. Unknown codes return `None`. The data is bundled with the framework and matches the IANA version used by `chrono-tz`. The formatter also supports `unique`. You can parse the returned identifier and use it to convert a UTC date.

Date strings use the generator’s `setDefaultTimezone(Some(zone))` override, then `app.timezone` from the application or factory connection, then UTC. Pass `None` to clear the override and use `getDefaultTimezone()` to inspect it. Use `dateTimeInTimezone(max, timezone)` for a typed zoned result, with `Some(zone)` taking precedence for that call. The date-time methods continue to return UTC instants. The generator override controls output formatting and leaves the application’s calendar boundaries in effect.

Use `dateTimeBetweenExpressions(start, end)` for PHP-style date expressions, such as `"-30 years"` and `"now"`. Both bounds use one reference time and the application time zone. An explicit zone in an expression takes precedence. A numeric end string is a Unix timestamp, and `"@-1"` is an explicit timestamp expression. When an endpoint fails to parse, this method uses Unix second zero, matching FakerPHP’s numeric use of PHP’s `false`. A start later than the end still returns an error. This method shares `unique` history with `dateTimeBetween`.

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

### Text Generation {#text-generation}

Use `words(count)`, `sentences(count)`, and `paragraphs(count)` for vectors of exactly that size. Their `AsText` variants join words or sentences with spaces and paragraphs with blank lines. Use `sentenceWithWordCount(count, variable)` and `paragraph(count, variable)` for a fixed count or the same count variation rule as FakerPHP. The range-based `sentence(words)` remains available. `text(max)` uses a UTF-8 byte limit. Values below five or a limit that cannot fit a generated unit return an error. These methods support `unique`. Vocabulary comes from fake-rs and varies by locale.

### Pattern Formatters {#pattern-formatters}

Use `numerify(Some(pattern))` to replace `#` with digits and `%` with nonzero digits. `lexify` replaces `?` with lowercase ASCII letters. `bothify` expands all three and replaces each `*` with a digit or lowercase letter, with equal probability for the two classes. `asciify` replaces `*` with printable ASCII from `!` through `~`. Other characters are preserved. Pass `None` to use the FakerPHP default pattern. `randomLetter()` and `randomAscii()` return individual Rust characters. All six methods support `unique`.

Use `regexify(pattern)?` to generate a sample from a basic regular expression. It accepts optional slash delimiters and outer anchors, literals, ASCII classes and ranges, `\d`, `\w`, dots, ordinary groups, alternatives and bounded repeats. As in FakerPHP, `*` and `+` choose a per-pattern upper bound from one through nine. Invalid syntax, unsupported features such as lookarounds and backreferences, and oversized generation return errors. `unique(false).regexify(pattern)?` deduplicates generated strings.

### Collection Sampling {#collection-sampling}

Use `randomElement(values)` to borrow one item or get `None` from an empty slice. `randomKey(values)` returns its zero-based position. To choose from map keys or enum variants, pass a slice of the keys or variants to `randomElement`. With `randomElements(values, count, allowDuplicates)?`, use `Some(n)` for a fixed count or `None` for a random count from one through the slice length. With replacement disabled, each position is selected at most once. Equal values at different positions can still appear together. A zero count returns an empty vector. A nonzero sample requires a nonempty slice, and sampling without replacement rejects counts above its length.

Use `shuffleArray(values)` to return a shuffled clone and `shuffleString(value)` to shuffle Unicode scalar values. `boolean(None)` uses a 50% chance of true. Pass `Some(75)` for a 75% chance. These methods support `unique`, which compares JSON-serialized values for generic samples and shuffled arrays.

### Locale, Country and Currency Samples {#locale-and-code-samples}

Use `languageCode()`, `countryCode()`, `countryISOAlpha3()`, `currencyCode()`, and `emoji()` for fixture values from the pinned FakerPHP catalog. `randomLocale()` samples a locale identifier such as `en_GB` or `zh_TW`. It corresponds to FakerPHP’s `locale()` formatter. OpenRust’s existing `locale()` reads the configured generator locale. The sample catalog includes historical identifiers. All six formatters support `unique`.

### Digest Samples {#digest-samples}

Use `md5()`, `sha1()`, and `sha256()` to generate lowercase hexadecimal fixture values of 32, 40, and 64 characters. Like FakerPHP, each formatter hashes the decimal text of a random integer from 0 through 2,147,483,647. All three support `unique` and are intended for test and seed data.

### Network Address Samples {#network-address-samples}

Use `ipv4()`, `localIpv4()`, `ipv6()`, and `macAddress()` for network fixtures. `ipv4()` samples the ranges `1.0.0.0`–`127.255.255.255` and `128.0.0.0`–`255.255.255.254` with equal probability for each range. The result can include private or special-use addresses. `localIpv4()` chooses among `10/8`, `172.16/12`, and `192.168/16` with equal probability. IPv6 uses eight uncompressed lowercase hexadecimal groups without leading zeros. MAC addresses use six uppercase two-digit octets separated by colons. All four support `unique`.

### Domains, URLs and Slugs {#domain-url-slug-samples}

Use `domainWord()?` to transliterate a generated surname to lowercase ASCII, and `tld()` to sample a suffix using the FakerPHP weights for supported locales. `domainName()?` joins them. `url()?` uses the upstream HTTP and HTTPS templates with optional `www`, slug and `.html`. Names use OpenRust’s locale data and transliteration, which can differ from FakerPHP’s ICU or fallback mappings. If the surname yields no usable characters, the domain and URL formatters return an error. `slug(count, variable)` joins generated words with hyphens. Pass `None` for six base words, `Some(0)` for an empty result, or `false` for an exact count. All five formatters support `unique`.

### Email and Password Samples {#email-and-password-samples}

Use `email()?` for a general address or `companyEmail()?` for an address on a generated domain. The simplified Chinese locale uses its free-provider list for general email. `safeEmail()` uses reserved example domains and `freeEmail()` uses localized provider domains. Use `safeEmailDomain()` and `freeEmailDomain()` to sample only the domain. Usernames use localized names or words with ASCII transliteration. If the result contains no usable characters, OpenRust generates an ASCII letter to preserve the infallible username and email methods. `password(min, max)?` generates printable ASCII from `!` through `~` with inclusive bounds in either order. The defaults are 6 and 20. These formatters support `unique`.

### Phone and IMEI Samples {#phone-and-imei-samples}

Use `phoneNumber()` for localized display numbers. Simplified and traditional Chinese use the pinned FakerPHP templates. Other locales retain the existing fake-rs formats. `e164PhoneNumber()` uses the pinned international templates and available regional overrides. It returns a plus sign followed by digits for test data. `imei()` returns 15 digits, including the Luhn check digit. All three support `unique`.

### Postal Addresses and Titles {#address-and-title-samples}

Use `streetAddress()` for a street line and `address()` for a complete address sample. The US, French, simplified Chinese and traditional Chinese forms follow their respective FakerPHP layouts. City and street names and some address components still use fake-rs data. Use `buildingNumber()`, `secondaryAddress()`, `postcode()`, `state()`, and `stateAbbr()` for individual fields. `title(Some("male"))` and `title(Some("female"))` select an honorific group. Use `title(None)` to sample either group. `titleMale()` and `titleFemale()` select directly. These methods support `unique`.

### Given Names by Gender {#given-names-by-gender}

Use `firstNameMale()` or `firstNameFemale()` to draw from the corresponding FakerPHP catalog. `firstNameWithGender(Some("male"))` and `nameWithGender(Some("female"))` select a catalog explicitly. Pass `None` to sample both groups. Unrecognized group strings follow the same rule. Full names use the regional templates and existing surname data. Traditional Chinese given names combine one or two sampled characters. Missing region catalogs use English given names and templates. The existing no-argument `name()` and `firstName()` keep their data sources. Their gender-selecting variants share the corresponding `unique` history.

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

## Creating Models Using Factories

Apply `state` in declaration order. A `sequence` receives the zero-based index for the current batch. `make` builds the model and runs `afterMaking`. `create` saves it, creates its declared children, then runs `afterCreating`. Calling `count` switches the result to an Eloquent collection. Factory attributes bypass mass-assignment filtering and still pass through the model’s setters and casts.

Use `prependState` to run a transformation before all currently registered states. Each transformation sees the attributes produced so far. Use `set(name, value)` to append a single attribute override. Both work before or after `count`. Attributes passed directly to a creation method are applied after the registered states.

For a counted batch, all models are constructed before `afterMaking` runs. `create` then saves each model and its declared relations. The batch runs `afterCreating` after those writes finish. If `afterMaking` fails, the batch itself has yet to be saved. Owner factories resolved during construction may already have written records.

Use `raw()` or `rawOn(connection)` to get expanded attributes before model setters and casts. A counted factory returns a vector of attribute maps. Raw expansion skips the target factory’s callbacks and child relationships. It can still persist a declared parent to resolve its key. Use `createQuietly()` or `createQuietlyOn(connection)` to persist models and relationships while suppressing Eloquent model events. Factory callbacks still run.

Use `createOne(attributes)` to create one model even on a counted factory. Use `createMany(records)` to create records one at a time, in order. Pass `FactoryRecords::Default` for the configured count (one on an uncounted factory), `FactoryRecords::Count(n)` for an explicit count, or `FactoryRecords::Attributes(vec![…])` for per-record overrides. The overrides apply after factory states. Each record runs both factory callbacks before the next one starts. If a later record fails, earlier writes can remain. Use `createOneQuietly` and `createManyQuietly` to suppress model events, or the corresponding `On` methods to select a connection.

Use `makeOne(attributes)` to build one unsaved model, or `makeMany(records)` to build unsaved models in order. Both run `afterMaking` and accept explicit attributes. Use the same `FactoryRecords` variants with `makeMany`. It retains the configured count for later calls, and a zero count yields an empty collection. Neither method saves the returned models. Resolving a declared parent can still write that parent. The `On` variants take a connection. `lazy(attributes)` returns an async callback. Await a call to create the model or counted batch using the connection resolved at that time. Calling it again creates a fresh result.

Use the callback returned by `lazy` as shown below. Creating the callback leaves the target table untouched. Each awaited invocation writes a new batch.

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

Use `forEachSequence(states)` to apply one attribute map per model and set the batch count to the number of maps. `crossJoinSequence(sequences)` combines the maps from each input sequence in order, with the last sequence changing fastest. Later maps override earlier maps on matching keys. Set the count separately to select how many combinations to use. The sequence repeats if the count exceeds the number of combinations. An empty input sequence returns an error.

Use `insert(attributes)` on a single or counted factory to bulk-insert the generated rows, or `insertOn(connection, attributes)` to select the connection explicitly. The factory runs `afterMaking` and resolves any required parent keys. The bulk write skips target model persistence events, child creation and `afterCreating`. It writes storage attributes, including hidden columns, without adding appended values or applying output accessors.

Use `withoutAfterMaking()` or `withoutAfterCreating()` on a single or counted factory to clear the corresponding callbacks already registered on that factory. You can register new callbacks afterward. The other callback phase and Eloquent model events still run.

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

## Factory Relationships

Use `has` with `HasOne`, `HasMany`, `MorphOne` or `MorphMany`. Use `r#for` to generate a `BelongsTo` or `MorphTo` owner, or `forExisting` to associate an existing model. A batch resolves each generated owner once, including when using `make`. A new invocation resolves a new owner. A `MorphTo` descriptor must include the owner target and its mapped alias. `hasAttached` accepts a `BelongsToMany` or `MorphToMany` descriptor and writes the pivot attributes. Writes are sequential. If a later relation fails, earlier writes can remain.

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

Use `hasNamed`, `forNamed`, or `hasAttachedNamed` to resolve a relationship by name. The factory uses a descriptor registered through `Model::namedRelations` or the application’s `resolveRelationUsing`. Pass the Rust descriptor type explicitly, for example `forNamed::<_, MorphTo<Comment>>("subject", posts)`. Unknown names and incompatible descriptor types return errors.

Use `parentAttribute("owner_id", owners)` to declare a foreign-key attribute backed by another factory. It expands to a saved model key and can reuse a matching recycled model. An explicit value from the definition, a state, or the operation takes precedence. `withoutParents(vec![])` disables all factory-valued attributes. Pass `FactoryParentSelector::attribute("owner_id")` or `FactoryParentSelector::model::<Owner>()` to disable specific ones. Disabled attributes expand to null. Explicit `for` and `forExisting` relationships still resolve.

### Recycling Existing Models {#recycling-existing-models}

Call `recycle` with a saved model or `recycleMany` with an iterable of saved models of one type. Call either method before or after `count`. The pool follows nested factory relationships. For each generated owner, the factory randomly selects a matching model of the requested type on the same connection, or creates one if the pool has no match. A batch reuses its selected owner.

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

Pass a vector of attribute maps to `hasAttached` or `hasAttachedNamed` to apply pivot attributes in related-model order. The sequence cycles when the related factory count exceeds its length. Use `FactoryPivot::resolve` to compute attributes for each attachment from the saved parent model. An attribute map applies the same values to all attachments.

Pass an existing model, a vector of models, or an Eloquent collection to attach them without creating new related rows. The models must be persisted, have the related key, and belong to the connection used by the relationship. Invalid targets fail before any pivot row for that attachment is written.
