# Localization

## Configuring the Locale

Configure `locale` and `fallback_locale` in `config/app.rs` through the root `.env`:

```dotenv
APP_LOCALE=en
APP_FALLBACK_LOCALE=en
```

Following the Laravel tutorial, use `App::setLocale` to change the locale for an HTTP request, `App::currentLocale` to read it, and `App::isLocale("en")?` to check it. For example:

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, abort},
    routing::RoutingError,
    support::facades::{App, Route},
};

pub fn register() -> Result<(), RoutingError> {
    Route::get("/greeting/{locale}", greeting)
}

fn greeting(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let locale = request.route("locale").unwrap_or("en").to_owned();
    Box::pin(async move {
        if !["en", "zh-Hans", "zh-Hant"].contains(&locale.as_str()) {
            return Err(abort(StatusCode::BAD_REQUEST, "", Default::default()));
        }
        App::setLocale(&locale).await.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        let current = App::currentLocale().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::from(current))
    })
}
```

`App::getLocale` is an alias for `currentLocale`; `getFallbackLocale` and `setFallbackLocale` read and update the fallback locale. `setLocale` updates both `app.locale` and the translator, then dispatches `foundation::events::LocaleUpdated` with `locale` and `previousLocale`. Rust event listeners may be asynchronous, so this method requires `.await` and returns failures through `Result`. Request and queue-task changes are isolated; changes during startup become that application's defaults.

Service failures propagate at the step where they occur: configuration is written before the translator is updated, followed by event dispatch. On first resolution, the long-lived translator reads application defaults separately from the current scope configuration, so the first request cannot become the application default. A listener failure keeps the updated configuration and translator until the current lifecycle ends; the error retains its original source. Setting the same locale again still dispatches the event, while changing the fallback locale does not.

Language identifiers use BCP 47 throughout configuration, APIs, resources, documentation URLs, HTML and HTTP. `setLocale("zh-CN")` keeps `zh-CN`; `isLocale("ZH-cn")` is true and `isLocale("zh-Hans")` is false. Normalization preserves script, region, variants and supported extensions; choosing a translation resource is a separate operation. Old underscore input returns an error.

The `App` facade currently exposes these locale methods; other Laravel App methods are not exposed through this facade. Templates use the translation helpers below. `Lang::withLocale` provides a temporary translator-only locale scope without changing application locale configuration.

### Browser Language {#welcome-page-language}

OpenRust includes `openrust::http::middleware::SetLocale` in its default global middleware stack. Applications do not need a locale middleware file or manual registration in `bootstrap/app.rs`.

The defaults are `en`, `zh-Hans`, and `zh-Hant`. The middleware reuses `Request::getLanguages()` for parsing and priority order, respects quality weights, skips `q=0`, and keeps browser order for equal weights. Without a match, it uses a supported `APP_LOCALE`, then the first candidate (English by default). Controllers, validation, and Blade share the current request locale.

`zh-Hans`, `zh-CN`, `zh-SG`, and bare `zh` select Simplified Chinese; `zh-Hant`, `zh-TW`, `zh-HK`, and `zh-MO` select Traditional Chinese. An explicit script takes precedence over region; unknown scripts are not inferred from Chinese regions. Numeric variants are not scripts. Other languages match an exact tag or base language. This selection policy does not change public locale canonicalization.

To support other languages, override candidates inside `withMiddleware` in `bootstrap/app.rs`, without adding configuration keys:

```rust
.withMiddleware(|middleware| {
    middleware.setLocale(&["en", "fr", "de"]).expect("Valid locale tags");
})
```

Candidates are canonicalized as BCP 47. Invalid tags return `TranslationError` and preserve the previous setting; repeated calls use the last successful setting. Pass an empty list with `middleware.setLocale(&[])` to disable detection. The middleware then leaves the locale and response headers untouched and emits no locale update event; the application retains `app.locale` and manual `App::setLocale` calls. For specific routes only, disable global detection and attach `SetLocale::new(&["en", "fr"])?` through existing route or group middleware.

Before dispatching downstream, the middleware calls `App::setLocale`, updating configuration, the translator, and the `LocaleUpdated` event. Listener errors use the existing exception handler and stop downstream dispatch. After downstream processing, including rendered exception responses, it reads the final locale, adds `Content-Language` only when absent, and merges `Accept-Language` into `Vary`, preserving existing values and wildcards. It does not write cookies or change other requests, queue jobs, or application defaults. It runs after input transformations and before application-appended global middleware and routing. Earlier responses from CORS preflight, maintenance mode, or request size checks do not pass through locale selection.

Remove the previous application `SetLocale` file and its `append` registration to avoid duplicate execution. Use the empty-list setting above to disable detection.

The welcome page uses the current application locale for HTML `lang` and `Content-Language`. Display text uses the configured translation fallback. Repository links use the framework package metadata, and the template builds documentation paths with the current application locale. The default `en`, `zh-Hans`, and `zh-Hant` locales have matching documentation directories. If detection is disabled and `APP_LOCALE=fr`, text may fall back to English, but documentation links still point to `docs/fr`, which is not included. For another application locale, provide the corresponding documentation or customize the welcome template.

### URL Language Selection {#locale-query}

The default middleware reads `?lang=en`, `?lang=zh-Hans`, or `?lang=zh-Hant` before negotiating `Accept-Language`. A `localeUsing` callback returning a language takes priority over the URL; `None` continues with the URL, then browser negotiation. Explicit URL values require an exact canonical candidate: `ZH-hANT` selects `zh-Hant`, but `zh-CN` is not an alias here. Empty, invalid, array and unsupported values return HTTP 400. Repeated scalar keys follow the normal query parser: the last value wins. Request body fields do not select a language.

Configure the parameter in the language section of `config/app.rs`:

```rust
"locale_query_parameter": "lang",
```

Change it to `"language"` to read only `?language=...`, or set it to JSON `null` to disable URL selection while retaining callbacks and browser negotiation. A missing key uses `lang`, including older configuration caches. Empty names, whitespace, control characters, dots, brackets and non-string values are configuration errors; use a nonempty top-level query key. Configuration caches retain their existing values until rebuilt. There is no additional environment variable or middleware registration.

The welcome page provides normal GET links for the available `en`, `zh-Hans` and `zh-Hant` candidates plus “Follow browser”. Links preserve other query parameters, and the automatic option removes only the configured language key. The current language is underlined; a border marks the explicit selection or automatic mode. Refreshing or sharing a URL retains its choice. Visiting the home page without that parameter resumes browser negotiation; no preference is written to Cookie, Session, a database or localStorage.

The switcher is hidden when URL selection is disabled, the middleware is disabled, no welcome-page language is a candidate, or a callback has already selected a language that URL links cannot override. Custom candidates do not add untranslated menu entries. The middleware places `LocaleSelection` in the native request extensions, carrying the active query parameter, candidates and `from_query` flag; the controller reads it only to build controls, without selecting a language again. URL selection retains `Content-Language`, `Vary: Accept-Language` and downstream header behavior. Private callback-based content still requires an application cache policy.

### Custom Locale Selection {#custom-locale-selection}

`localeUsing` is an OpenRust extension. Use it to configure a synchronous callback on the current application's middleware instance. For example, prefer an explicit request header and continue with URL selection and browser negotiation when it is absent:

```rust
.withMiddleware(|middleware| {
    middleware.setLocale(&["en", "zh-Hans", "zh-Hant"]).expect("语言标签有效");
    middleware.localeUsing(|request| {
        Ok(request.header("X-Locale")?.map(str::to_owned))
    });
})
```

The callback returns `Result<Option<String>, HttpError>`:

- `Ok(Some(locale))` canonicalizes the BCP 47 tag and requires an exact match in the canonical candidate list. It does not apply browser aliases, base-language matching, or translation fallback. With default candidates, `ZH-hANT` selects `zh-Hant`; `zh-CN`, `en-US`, empty strings, and invalid tags are errors. Include a valid region or extension tag explicitly to select it unchanged.
- `Ok(None)` tries the configured URL parameter first, then the existing browser negotiation and supported application default or first candidate.
- `Err(error)` passes the original error to the existing exception handler and stops downstream execution. Invalid or unsupported selections are internal errors; validate untrusted user input in the callback and return an appropriate HTTP error if needed. Selection failures do not switch locale or emit `LocaleUpdated`.

Without a callback, behavior stays unchanged. Candidate and callback configuration are independent and can be called in either order. Invalid candidate updates preserve both previous settings; the last callback replaces the previous callback. Empty candidates disable the entire instance, including callback execution, events and header handling. The callback remains stored for re-enabling candidates. Route instances expose the same `setLocale` and `localeUsing` methods; callbacks and candidate changes belong to that instance, never a process-wide registry. The callback itself must also avoid shared mutable user state.

The default global position precedes routing, Session and authentication. A callback can read headers and information already prepared at that point; it cannot access dependencies provided later. For Session rules, first call `middleware.setLocale(&[]).expect("语言标签有效")` in `withMiddleware`. Then register the same middleware in `routes/web.rs`, after the default `web` group has loaded Session:

```rust
use std::sync::Arc;
use openrust::{
    contracts::session::Session,
    http::{Response, middleware::SetLocale},
    routing::{RouteGroup, RoutingError},
    support::facades::Route,
};

pub fn register() -> Result<(), RoutingError> {
    let mut locale = SetLocale::new(&["en", "zh-Hans", "zh-Hant"])
        .expect("语言标签有效");
    locale.localeUsing(|request| {
        Ok(request.session()?.get("locale")
            .and_then(|value| value.as_str().map(str::to_owned)))
    });
    Route::group(RouteGroup::new().middleware(Arc::new(locale)), |router| {
        router.get("/account", || Response::from("Account"))?;
        Ok(())
    })
}
```

This example requires a configured Session driver and application encryption key. It reads an existing `locale` Session value and does not persist a new preference. If no value exists, URL selection and then browser negotiation apply. Authentication-dependent rules likewise belong after authentication; a rule affecting localized route bindings must run before binding substitution. For that placement, use the existing `Middleware::group` / `MiddlewareEntry` configuration to explicitly order the group. Merely adding a callback does not reorder middleware. Perform asynchronous lookups in an earlier middleware and pass prepared data through the request; this callback is synchronous.

The same `SetLocale` applies `App::setLocale`, events and response headers for these sources. Downstream locale changes determine a missing `Content-Language`; existing values remain intact. `Accept-Language` is still merged into `Vary`, since the callback can fall back to browser negotiation. This does **not** fully vary a response selected by `X-Locale`, Cookie, Session or user data. For the header example, the response must also vary by `X-Locale`; for private content, apply `Cache-Control: private, no-store` or an appropriate application cache policy. Add other `Vary` fields when relevant. The middleware preserves those headers and does not infer cache keys, persist cookies, or make personalized responses safe for shared caches.

### Translating in Blade {#blade-translations}

Read translations directly in templates. The controller only supplies page data:

```blade
<h1>{{ __('welcome.heading') }}</h1>
<p>{{ trans('messages.greeting', ['name' => $name]) }}</p>
<p>{{ trans_choice('messages.items', $count) }}</p>
```

`__` and `trans` accept a string key, an optional replacement array, and an optional locale. `trans_choice` adds a numeric or array count after the key. Omitting the locale or passing `null` or an empty string uses the current request language; an explicit locale affects only that call. Replacement values may be strings, numbers, booleans, or `null`. Missing keys and fallback languages follow the same rules as `Lang`.

<code v-pre>{{ ... }}</code> escapes HTML. `@lang('messages.title')` and `@choice('messages.items', $count)` output raw text, as does `{!! __('messages.title') !!}`; use these only with trusted HTML and replacement values. Includes and components inherit the rendering language, and compiled templates resolve translations at render time. The helpers require an active application translation service. Helpers return text or a translation group/list. Use groups and lists in `@foreach` or read an entry with an array offset; directly echoing an array returns an error. Rust callers can use `Lang::array`.

`@lang ... @endlang` renders a translation block, trims surrounding whitespace, and translates the resulting key. Use `@lang(['name' => $name]) ... @endlang` for replacements; output is raw text. Nested blocks share replacement state: starting an inner block replaces the outer replacement array. `__()` and `__(null)` return `null`, which echoes as empty text. `@lang('messages.title', [], null, false)` disables group fallback for that call. For plural translations, `count => null` uses the actual count, while explicit zero and empty strings remain replacements. `trans()` / `trans(null)` would return a translator object, which the template value model does not expose; use `Lang` from Rust instead. Arbitrary PHP functions, facade method calls, and named arguments are not implemented. Argument counts and types are checked explicitly rather than relying on PHP coercion.

## Publishing Language Files {#publishing-the-language-files}

Run `./artisan lang:publish` to publish the English authentication, pagination, password reset, and validation JSON catalogs. The command writes to `lang/en` or to `resources/lang/en` when the application already uses `resources/lang`. It preserves existing files by default. Use `--force` to replace all four files, or `--existing` to replace only previously published files. When both flags are set, `--existing` determines the scope.

## Defining Translation Strings

Put grouped translations in files such as `lang/en/messages.json`. Retrieve nested entries with dot notation. Use `lang/en.json` for translations whose keys are complete phrases. When `resources/lang` exists, it is used as the application language directory.

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

### Registering Paths and Translation Lines {#registering-translation-sources}

Register directories through `Lang::addPath`, `Lang::addJsonPath`, or `Lang::addNamespace` in the application scope before the first lookup. Later paths override earlier paths. Grouped catalogs merge recursively, including list indexes. Literal JSON keys are replaced as a whole. Application roots are applied after JSON-only roots.

Use `Lang::addLines` with dotted group keys. A newly registered group starts with the supplied lines. To extend a file-backed group, call `Lang::load` first. Loaded groups are cached for the application, so later path registration applies to future loads.

```rust
use std::path::PathBuf;

use openrust::{
    support::facades::Lang,
    translation::{Line, TranslationResult},
};

pub fn register_translations(directory: PathBuf) -> TranslationResult<()> {
    Lang::addPath(directory)?;
    Lang::load(None, "messages", "en")?;
    Lang::addLines(
        [("messages.runtime".into(), Line::Text("Registered :name".into()))],
        "en",
        None,
    )
}
```

### Loading Groups From a Custom Source {#loading-custom-translation-sources}

Pass a `Loader` to `Lang::loadFrom` at an async application boundary. Each resource identifies one locale, group, and optional namespace. The returned lines replace the selected groups together. A loader error or a concurrent runtime write to one of those groups leaves the batch unpublished. Subsequent lookups stay synchronous.

```rust
use openrust::{
    contracts::translation::Loader,
    support::facades::Lang,
    translation::{TranslationResource, TranslationResult},
};

pub async fn load_notice(source: &dyn Loader) -> TranslationResult<()> {
    Lang::loadFrom(
        source,
        &[TranslationResource {
            locale: "en",
            group: "notices",
            namespace: None,
        }],
    )
    .await
}
```

### Database Translation Source (Extension) {#database-translation-loader}

Add `openrust-translation-database` to the application's Cargo dependencies using `path = "vendor/openrust/translation-database"`. `DatabaseLoader` accepts an explicit framework `Connection`; obtain it from the application's database manager or `DB::connection`. It reads translations from PostgreSQL 18, MySQL 8.4, and SQLite. Other database targets require separate behavior validation.

Create the table through an application migration before loading translations. This minimal schema is supported:

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_translations(connection: &Connection) -> DatabaseResult<()> {
    connection.getSchemaBuilder().create("language_lines", |table| {
        table.id();
        table.string("group", None).index();
        table.string("key", None);
        table.json("text");
        table.timestamps();
    }).await
}
```

The schema follows Spatie’s Laravel Translation Loader: one row per group/key; `text` is a JSON object such as `{"en":"Hello :name","fr":"Bonjour :name"}`. Use canonical locale keys (`en-US`, `zh-Hans`). A missing or null locale leaves the file value available. Translation values must be strings; empty strings are valid. Group `*` stores literal keys, including dots. Other groups expand dotted keys such as `nested.title` or `items.0`. Package namespaces use only the file source, matching Spatie. Duplicate or overlapping keys, invalid JSON, empty key segments, and keys deeper than 128 segments fail the load.

Call this function inside an asynchronous application scope, such as a provider's `boot`, after migrations are available:

```rust
use std::{path::PathBuf, sync::Arc};
use openrust::{
    database::Connection,
    support::facades::Lang,
    translation::{FileLoader, TranslationResource, TranslationResult},
};
use openrust_translation_database::DatabaseLoader;

pub async fn load_database_translations(
    connection: Connection,
    language_directory: PathBuf,
) -> TranslationResult<()> {
    let source = DatabaseLoader::new(connection)
        .withTable("language_lines")?
        .withMaxRows(10_000)?
        .withFallback(Arc::new(FileLoader::new(language_directory)));
    Lang::loadFrom(
        &source,
        &[
            TranslationResource { locale: "fr", group: "messages", namespace: None },
            TranslationResource { locale: "en", group: "messages", namespace: None },
        ],
    ).await
}
```

The selected connection's table prefix applies. The table name is an unqualified identifier. The default table is `language_lines`, and the default maximum is 10,000 rows per group. Exceeding the configured limit fails the load without truncating the result.

`withFallback` loads the same locale/group/namespace from the file source and overlays database entries. Groups and list indexes merge recursively; literal JSON values are replaced as a whole. Empty database strings remain valid overrides. After deleting a row, explicitly reload its resource to restore the file value. The translator retains its configured language fallback; load database resources for both the selected and fallback languages when needed.

`Lang::loadFrom` publishes all requested groups together. Database, file, or parsing failures leave the previous published catalog available and return an error; missing rows produce an empty database overlay. Reads occur per group; publication atomicity does not provide a transaction spanning all database reads. Reloads are explicit. Applications manage writes, migrations, and refresh timing; the package does not automatically select a source or start background tasks.

Unlike Spatie’s PHP model, this source does not perform locale fallback while reading each row: OpenRust’s translator applies its configured fallback after loading exact locales. This also preserves file translations for the selected locale. A missing table returns an error so a failed explicit reload cannot silently replace a published database catalog with files.

## Retrieving Translation Strings

Use `__` to retrieve text. Placeholders support `:name`, `:Name` and `:NAME`. Use `Lang::array` for a group or list. A missing key is returned as text. Literal JSON keys are looked up in the selected locale. Grouped keys can fall back to the configured language.

### Fallback and Missing Keys {#translation-lookup-hooks}

Use `Lang::has(key, locale, true)` to check fallback translations and `Lang::hasForLocale(key, locale)` for an exact locale. Existence checks skip the missing-key handler. Register lookup hooks inside the application scope. The locale callback can reorder grouped lookups. A literal JSON match takes precedence. The missing-key handler can return replacement text, which then receives normal placeholder replacements. Returning `None` keeps the original key.

```rust
use openrust::{support::facades::Lang, translation::TranslationResult};

pub fn register_lookup_hooks() -> TranslationResult<()> {
    Lang::determineLocalesUsing(|locales| {
        let mut locales = locales.to_vec();
        if locales.first().is_some_and(|locale| locale == "fr-CA") {
            locales.insert(1, "fr".into());
        }
        locales
    })?;

    Lang::handleMissingKeysUsing(|missing| {
        (missing.key == "messages.welcome").then(|| "Hello, :name".into())
    })
}
```

### Replacing Parameters in Translation Strings {#replacing-parameters-in-translation-strings}

Use `Lang::getWith` to pass an ordered list of text, tags, and objects. A callback receives the contents of each matching `<name>…</name>` on the same line. Tag callbacks run in argument order before text placeholders are substituted. Use `Lang::arrayWith` to replace values within a group or list. `Lang::choiceWith` picks the plural form before applying replacements.

#### Object Replacement Formatting {#object-replacement-formatting}

Register a formatter for the concrete Rust type with `Lang::stringable` in the application scope, typically in `AppServiceProvider::boot`. Use `Replacement::object` when registration is required, or `Replacement::display` to use the type’s `Display` implementation when no formatter is registered. To use a Rust enum, pass the desired variant name or value explicitly, or register a formatter for the enum.

For example, define `payments.notice` in `lang/en/payments.json`.

```json
{"notice": "<mark>Paid</mark> :amount"}
```

```rust
use openrust::{
    support::facades::Lang,
    translation::{Replacement, TranslationResult},
};

pub struct Amount(pub u32);

pub fn register_formatters() -> TranslationResult<()> {
    Lang::stringable(|amount: &Amount| format!("{} USD", amount.0))
}

pub fn payment_notice(amount: &Amount) -> TranslationResult<String> {
    Lang::getWith(
        "payments.notice",
        &[
            ("amount", Replacement::object(amount)),
            ("mark", Replacement::tag(|text| format!("[{text}]"))),
        ],
        None,
    )
}
```

## Pluralization

Use `trans_choice` to select a form. Explicit conditions such as `{0}` or `[2,*]` are tried first. Otherwise, the selector uses the matching locale’s plural rules. It provides `:count` automatically.

```rust
use std::collections::BTreeMap;

use openrust::{
    support::facades::Lang,
    translation::{TranslationResult, __, trans_choice},
};

pub async fn summary(locale: &str) -> TranslationResult<(String, String)> {
    Lang::withLocale(locale, async {
        let replacements = BTreeMap::from([("name".into(), "ada".into())]);
        Ok((
            __("messages.welcome", &replacements)?,
            trans_choice("messages.items", 2_i64, &replacements)?,
        ))
    })
    .await?
}
```

Use BCP 47 tags with hyphens: `en`, `en-GB`, `fr-CA`, `es-419`, `sr-Latn-RS`, `zh-Hans`, `zh-Hant`, `zh-Hant-HK`, `en-US-u-nu-latn` and `en-x-demo`. The existing ICU4X 2.4 parser and CLDR canonicalizer normalize casing and deprecated aliases, such as `iw-IL` to `he-IL`, without likely-subtag expansion. This validates the supported Unicode locale syntax, not every IANA registration. Unsupported forms such as grandfathered `i-klingon`, standalone private-use `x-demo` and extlang forms return an explicit error; supported extensions are never silently removed. Translation lookup uses the complete canonical tag as an exact catalog name and then the configured fallback for grouped messages; literal JSON keys keep their existing lookup semantics. No implicit region-to-script lookup is added. Number consumes Unicode extensions; Faker adapts its private provider keys without changing the public locale.

## Overriding Package Language Files {#overriding-package-language-files}

To override a package translation such as `shop::cart.summary.heading`, define the same key in `lang/vendor/shop/en/cart.json`. Use the package namespace, locale, and group name from the original translation. Only the entries provided by the application are overridden. Other entries still come from the package.

## Artisan translations {#artisan-translations}

Artisan uses `APP_LOCALE` for list headings, help and argument errors. Built-in text is available in English, Simplified Chinese and Traditional Chinese. An application can override text in `lang/<locale>/console.json`. Lookup uses the active locale and then `APP_FALLBACK_LOCALE` before built-in text. A command without a translated description uses its registered description.

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh-Hans ./artisan list make
APP_LOCALE=zh-Hant ./artisan help list
```

To customize help, use keys such as `options`, `commands.list.description` and `commands.list.arguments.namespace` inside the console group. See [Artisan output formats](./artisan.md#command-help-and-output-formats).

Production confirmations and cancellation messages use the same translation group. Override the message with `confirm.production` or `confirm.cancelled` in `console.json`. Answer with `y` or `yes` to proceed. An empty answer declines.

## Locale contract methods

The Translator contract, concrete translator, and Lang facade expose `getLocale`, `setLocale`, `getFallback`, and `setFallback`. Boot-time changes configure application defaults; request/queue changes stay in the current scope. Nested `withLocale` restores its caller’s locale; fallback is set separately. Migrate old snake_case contract calls to these names.

## Locale Ownership

Locale and fallback getters return owned `String` snapshots. Setters take `&self` and return `TranslationResult<()>`; invalid tags return an error. Normalization keeps `zh-CN` distinct from `zh-Hans`. Boot-time values form the defaults; request and queue-task changes stay in that container scope. `withLocale` temporarily overrides only the active task and restores the enclosing locale after completion.

## Upgrading Locale Identifiers {#bcp47-upgrade}

Pause new jobs and drain old queued mail/notification jobs before stopping the old web and queue processes. If draining is impossible, retain those jobs for application-specific migration. Back up configuration, catalogs and affected data before changing resources or stored locale values, and do not mix old and new processes. Update `APP_LOCALE`, `APP_FALLBACK_LOCALE` and `APP_FAKER_LOCALE` explicitly; the application never edits your `.env` automatically.

| Existing project value/path | Replacement |
| --- | --- |
| `zh_CN`, `lang/zh_CN`, `docs/zh_CN` | `zh-Hans`, `lang/zh-Hans`, `docs/zh-Hans` |
| `zh_TW`, `lang/zh_TW`, `docs/zh_TW` | `zh-Hant`, `lang/zh-Hant`, `docs/zh-Hant` |
| Regional `en_US`, `en_GB`, `zh_HK`, `zh_Hant_HK` | `en-US`, `en-GB`, `zh-HK`, `zh-Hant-HK` |

The first two mappings apply only to this project's default catalogs and confirmed data. Valid `zh-CN` and `zh-TW` remain regional tags. Rename sentence-key JSON files and package overrides as well as grouped directories. Detect duplicate destination files or JSON keys before moving anything; preserve both versions for manual resolution. No old URL redirects or underscore aliases are installed by default; configure any transitional redirects at your hosting layer.

Clear old configuration and view caches with the old binary (`./main artisan config:clear`, `./main artisan view:clear`) before replacing it, or use the existing source `./artisan` entrypoint. Deploy the new code and resources together, rebuild with `config:cache` and `view:cache` using the new configuration, and restart all long-lived processes. Translation directory caches are process-local and disappear on restart. Drain old queued mail/notification jobs before upgrading, or inspect their serialized `locale` fields with application-specific tooling; encrypted payloads require the old application's encryption keys. A generic text replacement in queue, cache or settings blobs is unsafe.

For database translations, inventory `language_lines.text` JSON keys and any application-owned settings or locale columns first. The templates in `vendor/openrust/translation-database/database/upgrade-bcp47/` include separate read-only inventory queries and PostgreSQL, MySQL and SQLite transactions. They map the two default catalogs plus `en_US`/`en_GB`; customize table names and any other legacy locale mappings after review. Unknown underscore keys or any old/new collision abort the transaction, including equal or null values; no content is overwritten. The final statement is **ROLLBACK** by default. Inspect the result and backup before explicitly changing it to **COMMIT**. On error roll back and close that dedicated database session. Re-running a successful migration is a no-op; rollback after a committed migration requires restoring the backup, since existing new-format keys cannot safely be distinguished from migrated ones.

Faker keeps native provider keys private; `locale()` and `randomLocale()` return BCP 47. Script-only `zh-Hans`/`zh-Hant` use the corresponding existing Chinese providers, while the public label retains its original region and extensions. Third-party protocols such as WeChat's native `lang=zh_CN` are unchanged at that boundary.
