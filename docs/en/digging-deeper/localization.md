# Localization

Rust applications import `Lang`, `__`, and `trans_choice` from `openrust::localization`. The concrete service and its contract are both named `Localizer`, under `openrust::localization` and `openrust::contracts::localization` respectively. When upgrading older Rust imports, replace `Translator` with `Localizer` and change the container keys `translator` and `translation.loader` to `lang` and `lang.loader`. Resolve the concrete `FileLoader` by type when registering file paths. Application `lang/` directories and template helpers keep their existing syntax.

For translating user-provided text, see [Text Translation](../packages/translate.md).

## Configuring the Locale

Configure `locale` and `fallback_locale` in `config/app.rs` through the root `.env`:

```dotenv
APP_LOCALE=en
APP_FALLBACK_LOCALE=en
```

Following the framework tutorial, use `App::set_locale` to change the locale for an HTTP request, `App::current_locale` to read it, and `App::is_locale("en")?` to check it. For example:

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
        App::set_locale(&locale).await.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        let current = App::current_locale().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::from(current))
    })
}
```

`App::get_locale` is an alias for `current_locale`; `get_fallback_locale` and `set_fallback_locale` read and update the fallback locale. `set_locale` updates both `app.locale` and the localizer, then dispatches `foundation::events::LocaleUpdated` with `locale` and `previous_locale`. Rust event listeners may be asynchronous, so this method requires `.await` and returns failures through `Result`. Request and queue-task changes are isolated; changes during startup become that application's defaults.

Service failures propagate at the step where they occur: configuration is written before the localizer is updated, followed by event dispatch. On first resolution, the long-lived localizer reads application defaults separately from the current scope configuration, so the first request cannot become the application default. A listener failure keeps the updated configuration and localizer until the current lifecycle ends; the error retains its original source. Setting the same locale again still dispatches the event, while changing the fallback locale does not.

Language identifiers use BCP 47 throughout configuration, APIs, resources, documentation URLs, HTML and HTTP. `set_locale("zh-CN")` keeps `zh-CN`; `is_locale("ZH-cn")` is true and `is_locale("zh-Hans")` is false. Normalization preserves script, region, variants and supported extensions; choosing a translation resource is a separate operation. Old underscore input returns an error.

The `App` facade currently exposes these locale methods; other the framework App methods are not exposed through this facade. Templates use the translation helpers below. `Lang::with_locale` provides a temporary localizer-only locale scope without changing application locale configuration.

### Browser Language {#welcome-page-language}

The application automatically installs `openrust::http::middleware::SetLocale`. Applications do not need a locale middleware file or manual registration in `bootstrap/app.rs`.

Default candidates are discovered once at HTTP startup from framework and application translation resources; the framework provides all 18 bundled locales. The middleware shares the request language parser for its configured negotiation header, respects quality weights, skips `q=0`, and keeps browser order for equal weights. Without a match, it uses a supported `APP_LOCALE`, then the first candidate in stable tag order. Controllers, validation, and Blade share the current request locale.

`zh-Hans`, `zh-CN`, `zh-SG`, and bare `zh` select Simplified Chinese; `zh-Hant`, `zh-TW`, `zh-HK`, and `zh-MO` select Traditional Chinese. An explicit script takes precedence over region; unknown scripts are not inferred from Chinese regions. Numeric variants are not scripts. Other languages match an exact tag or base language. This selection policy does not change public locale canonicalization.

Use [`app.supported_locales`](#available-locales) to restrict or order languages and enable non-file sources through configuration. To override that list in code, use `with_middleware` in `bootstrap/app.rs`:

```rust
.with_middleware(|middleware| {
    middleware.set_locale(&["en", "fr", "de"]).expect("Valid locale tags");
})
```

Candidates are canonicalized as BCP 47. Invalid tags return `TranslationError` and preserve the previous setting; repeated calls use the last successful setting. Pass an empty list with `middleware.set_locale(&[])` to disable detection. The middleware then leaves the locale and response headers untouched and emits no locale update event; the application retains `app.locale` and manual `App::set_locale` calls. For specific routes only, disable global detection and attach `SetLocale::new(&["en", "fr"])?` through existing route or group middleware.

Before dispatching downstream, the middleware calls `App::set_locale`, updating configuration, the localizer, and the `LocaleUpdated` event. Listener errors use the existing exception handler and stop downstream dispatch. After downstream processing, including rendered exception responses, it reads the final locale, adds `Content-Language` only when absent, and merges the enabled configured language header (default `Accept-Language`) into `Vary`, preserving existing values and wildcards. Automatic selection never writes a preference or changes other requests, queue jobs, or application defaults. The default route placement follows input transformations and protected Cookie setup; see [Automatic and fixed language](#locale-preference). Earlier responses from CORS preflight, maintenance mode, or request size checks do not pass through locale selection.

Remove the previous application `SetLocale` file and its `append` registration to avoid duplicate execution. Use the empty-list setting above to disable detection.

The welcome page uses the current application locale for HTML `lang` and `Content-Language`. Text follows the configured translation fallback. Documentation links use the corresponding `en`, `zh-Hans` or `zh-Hant` directory; other runtime languages link to English documentation. This mapping only selects documentation links and does not change the application locale or message fallback.

### Framework defaults and application overrides {#default-translations}

The 18 default locales live in `vendor/openrust/framework/src/localization/lang/` and are embedded in the binary. The default application keeps editable `en`, `zh-Hans` and `zh-Hant` welcome.json files under `lang/`; other built-in catalogs do not need application copies. Create only the keys you want to override, for example `lang/fr/validation.json`:

```json
{"required": "Custom message for :attribute", "min": {"string": "At least :min characters"}}
```

Grouped resources are merged recursively: these keys win while other French validation messages remain available from the framework. A sentence catalog such as `lang/fr.json` overrides matching top-level sentence keys. Missing keys then follow the configured fallback language. Invalid application JSON remains an error; defaults do not hide broken files. Changing an override requires refreshing an already-loaded localizer by restarting persistent processes.

Framework defaults require neither runtime source files nor publishing. `lang:publish` optionally publishes all bundled groups and sentence catalogs for customization; existing files remain protected unless overwrite is requested. Namespaced package translation rules stay separate.

### Available Languages {#available-locales}

For ordinary applications, configure `app.supported_locales` through `.env`; no middleware callback is required:

```dotenv
APP_SUPPORTED_LOCALES=en,zh-Hans,zh-Hant,ja
```

An unset variable defaults to `*`, discovering all available framework and application resource languages. `APP_SUPPORTED_LOCALES=` produces an empty list and disables locale selection and the welcome menu. A comma-separated list enables those languages in order, including database-only languages. Tags are canonicalized and deduplicated in their first occurrence order. Explicit `null` and `(null)` (case-insensitive), malformed tags and empty list entries fail at startup. Direct configuration accepts only `"*"` or a JSON array; JSON `null` and other types or strings are rejected. A wildcard is not a list entry and does not invent translations for every valid BCP 47 tag.

Candidate precedence is explicit `middleware.set_locale(...)` → `app.supported_locales` → resource discovery. An explicit empty list also takes precedence. Configured and explicit lists skip scanning. The default middleware captures candidates once at HTTP startup; changes require restarting the application and rebuilding configuration cache when used. This setting does not alter `app.locale`, `app.fallback_locale` or request-source priority.


The Provider loader includes embedded framework locales. `FileLoader::new(path)` alone remains a loader for explicitly supplied files. `FileLoader::available_locales()` discovers `<locale>.json` files and locale directories containing grouped JSON resources, including nested groups. It uses the same selected application root as translation loading (`resources/lang` when present, otherwise `lang`), plus application paths registered with `add_path` and top-level JSON files registered with `add_json_path`. Package namespaces, `vendor`, hidden entries, symbolic links and empty locale directories do not enable languages. Names must already match their canonical BCP 47 tag; for example, use `zh-Hans`, not `zh_CN` or `ZH-hans`. Results are deduplicated and sorted by tag. Discovery does not parse message contents or assess translation completeness.

The default middleware captures this list after application Providers finish booting and before serving requests. It does not scan on each request; restart the application after adding or removing language files. This list is independent of the configuration cache. Explicit `set_locale` candidates override configuration; otherwise an `app.supported_locales` array skips discovery. An empty discovered list leaves the configured application locale in effect and hides the welcome switcher. Database-only languages require explicit candidates; discovery never queries the database or enables every package language.

For Japanese, add `lang/ja/welcome.json` and `lang/ja/languages.json` containing `{"name":"日本語"}`; no controller or menu changes are needed. Missing messages retain the normal translation fallback. Templates can use `Lang::has_for_locale('languages.name', $choice)` to test exact-locale presence. For right-to-left pages, set `"direction":"rtl"` in that locale's `languages.json`; the welcome page sets HTML `dir="rtl"`, and its scrollable menu supports a longer language list. Missing direction uses the document's default left-to-right layout. New UI translations do not create documentation directories; keep the welcome documentation links consistent with the actual documentation available.

### URL Language Selection {#locale-query}

`?locale=en`, `?locale=zh-Hans` and `?locale=zh-Hant` override the locale for one request. They never save or replace a fixed preference. The order is callback → URL → protected preference Cookie → configured header → application default or first supported candidate. A callback returning `None` continues to the next source. Explicit URL values must canonicalize to an exact candidate; invalid, empty, array or unsupported values return HTTP 400. Repeated scalar keys use the normal query parser's last value. Request body fields do not select the current request's language.

In the language section of `config/app.rs`:

```rust
"locale_query_parameter": get(env, "APP_LOCALE_QUERY_PARAMETER", "locale")?,
```

Set `APP_LOCALE_QUERY_PARAMETER=language` to read only `?language=...`. A missing configuration key defaults to `locale`; JSON `null` disables only URL selection. Names must be nonempty top-level query keys without whitespace, control characters, dots or brackets; incorrect configuration types are errors. Environment values remain strings. Rebuild the configuration cache after changes; requests use the loaded repository without rereading `.env`.

The default query and Cookie names are now `locale` (`?locale=zh-Hans`, `locale=zh-Hans` before Cookie encryption). Old `lang` Cookies are no longer read by default: select a language again and update old bookmarks. To retain the previous names, explicitly set `APP_LOCALE_COOKIE=lang` and/or `APP_LOCALE_QUERY_PARAMETER=lang`. Rebuild the configuration cache after upgrading; cached explicit names remain authoritative. The save endpoint remains `POST /locale` with default form field `locale`.

### Automatic and Fixed Language {#locale-preference}

With no manual preference, Web pages negotiate the language on every request. The welcome-page language menu lists the same active candidates as the middleware and offers Follow browser. A POST form protected by the existing Web CSRF middleware saves the selected candidate; Follow browser deletes it. Automatic detection and GET requests never save a preference. The underlined item is the current request language; the check mark identifies the saved choice or automatic mode, including during a temporary URL override. Native names come from each locale's `languages.json`; a missing exact-locale name displays the locale code, without falling back to another language's name. Other query parameters survive the POST/303 redirect to the local page specified by the form; the temporary language parameter is removed. Disabling URL selection does not disable these forms.

The language preference setting `app.locale_cookie` defaults to `"locale"`, including when absent; the application preserves `APP_LOCALE_COOKIE` to override the name. JSON `null` disables preference reads and writes (the environment string `null` is a name). Choose a nonempty top-level Cookie name that does not conflict with Session or `XSRF-TOKEN`. Invalid configuration is an error. The Cookie must pass through `EncryptCookies` and must not be excluded from encryption. The default instance runs after Web Cookie decryption and queue setup, before the ordinary Web Session middleware. Stateless API routes do not read raw preference Cookies or automatically start Session. Without a protected Cookie queue, the welcome page retains temporary GET links when URL selection is enabled.

The preference uses `Cookie::forever` (400 days), encryption, HttpOnly, and the existing CookieJar defaults from `session.path`, `domain`, `secure` and `same_site` (normally `/`, current host, HTTPS-dependent Secure, and Lax). Deletion uses the same path and domain. Set `session.secure=true` on HTTPS-only deployments. Expiration, clearing Cookies, or changing browsers loses the local preference; it is not permanent or synchronized across devices. Invalid, damaged or no-longer-supported saved values are ignored and automatic selection resumes. Explicit invalid submissions return HTTP 400. Browser expiry is enforced by the client; a replayed Cookie header carries no expiry metadata.

For a protected Web write endpoint, the language preference helper reads only the specified top-level body field and leaves the response to the controller:

The welcome form field is configured separately from the Cookie and query parameter names in `config/app.rs`:

```rust
"locale_form_field": get(env, "APP_LOCALE_FORM_FIELD", "locale")?,
```

Set `APP_LOCALE_FORM_FIELD=preferred_locale` to change the welcome form and save handler together. The template reads `config('app.locale_form_field')`; the save handler strictly reads `Config::string("app.locale_form_field")`. Neither supplies a fallback; the application configuration defines the default `locale`. Missing keys, JSON `null` and non-string values cause the save handler to fail without writing a preference. Use a nonempty top-level name distinct from `redirect`, `_token` and `_method`. Rebuild the configuration cache after changes. This setting does not rename Cookie or URL inputs; the generic saver still accepts a caller-selected field.

```rust
let redirect = match request.post(Some("redirect")).await? {
    None => "/".to_owned(),
    Some(serde_json::Value::String(path)) => path,
    _ => return Err(HttpError::InvalidRequest),
};
let target = SetLocale::url_without_locale(request, &redirect)?;
SetLocale::store_preference_from_request(request, "locale").await?;
Ok(Redirect::to(&target, RedirectOptions {
    status: StatusCode::SEE_OTHER,
    ..Default::default()
})?.into())
```

The welcome form submits the current page path in the body field `redirect`. Another page can send `/account?tab=profile` to return there after saving. Missing, null or empty `redirect` falls back to `/`; other types and non-local targets return 400 before a preference is written. Query parameters do not supply this field. This is an application form convention, not a new middleware configuration.

A missing preference field, or a number, boolean, array or object as its value, returns HTTP 400. JSON `null` or an empty string clears the preference; a nonempty string uses the existing BCP 47 and candidate validation. Query parameters never supply the body field. It reads values after the existing input middleware; default trimming and empty-string conversion can turn whitespace into a clear operation. The method neither changes the current request language nor bypasses method, Cookie-pipeline or CSRF checks. The lower-level `store_preference(request, Some("zh-Hans"))` and `store_preference(request, None)` remain available for already validated values.

`url_without_locale(request, "/")` generates the application-specified return target, not the save endpoint. It removes the active language query key from the target and request, preserves other values and encoding, and uses the existing URL generator for trusted proxy subpaths. Request values override other target query values. Only local paths beginning with a single `/` are accepted; external or protocol-relative URLs and backslashes are rejected. It only returns a URL and does not store a preference or create a response. Import `SetLocale`, `Redirect`, `RedirectOptions` and `StatusCode` for the page example.

A controller using the same protected Cookie pipeline can instead accept a field named `language` and return 204 (or its own JSON response):

```rust
SetLocale::store_preference_from_request(request, "language").await?;
Ok(Response::empty(StatusCode::NO_CONTENT))
```

This does not enable persistence on an ordinary stateless API. Add the existing stateful Cookie pipeline and write protection explicitly; retain CSRF for browser requests. No route, Session or account storage is registered by these helpers. Both helpers manage language preferences.

Use one of these actions per write request, then redirect to an application-controlled local destination. The method rejects read requests and unavailable or unprotected preference storage; it queues a Cookie without changing the current locale or emitting a second event. It does not replace CSRF middleware. `LocaleSelection` exposes `preference`, `can_store`, supported candidates and URL selection data; read `App::current_locale()` for the actual language. A callback selecting a language hides controls that cannot override it. Empty candidates disable the entire middleware.

Protected preference pages use the existing cache-header merger to set `Cache-Control: private, no-store` and merge `Vary: Cookie`, even without a saved preference. The configured negotiation header is also merged when enabled; existing `Vary` entries and `*` remain intact. Route cache directives cannot publish these variants. Callbacks based on accounts or other private data still need an application-specific private-cache policy, especially without Cookie middleware.

The theme menu offers Light, Dark and Follow system. Automatic mode follows system changes; manual appearance is saved in localStorage (`appearance`), and Follow system removes it. Without JavaScript, language forms still work and appearance follows the system.

### API Client Preferences {#api-locale-preference}

Independent frontends and mobile clients store their own fixed selection. In automatic mode they send the current system/browser language on each request; in fixed mode they send the saved tag using the configured header (normally `Accept-Language`). Restoring automatic mode clears the client's saved selection. Sending that header does not create a server-side preference, Cookie, Session or account record. Only translated text changes. This repository supplies the server endpoint and consumer behavior, not a mobile client's persistence implementation.

A same-origin stateful browser API can use the same preference only when its route pipeline includes Cookie decryption and queuing before SetLocale. The built-in stateful API wrapper provides this for trusted frontend origins; ordinary API calls remain stateless. The same callback → URL → Cookie → header priority then applies. Keep CSRF protection on write endpoints. Returning JSON alone does not enable browser preferences.

For cross-device preferences, store a locale in your application's account settings and return that trusted, validated candidate from an instance `locale_using` callback placed after authentication. Return `None` when the account has no setting, allowing Cookie and browser fallback. No framework user-model field or account table is required or created. See [Custom locale selection](#custom-locale-selection) for disabling the default instance and placing the same middleware after its dependencies.

### Language Preference Header {#locale-header}

API clients should send the standard header by default:

```http
Accept-Language: zh-Hans
```

In `config/app.rs`, `"locale_header": get(env, "APP_LOCALE_HEADER", "Accept-Language")?` selects the application negotiation header. Set `APP_LOCALE_HEADER=X-Language` to read only that header, with case-insensitive HTTP header names; when absent, it does not retry `Accept-Language`. Values keep the same BCP 47 validation, weights, repeated-header merging and candidate matching. Missing configuration uses `Accept-Language`; JSON `null` disables only header negotiation. Environment values are strings: `APP_LOCALE_HEADER=null` names the header `null`, rather than disabling negotiation. To disable it, set JSON `null` in application configuration. Empty or invalid names and non-string values are configuration errors (HTTP 500), even when a callback or URL selects the language. An empty candidate list still disables the entire middleware.

Selection order is `locale_using` returning `Some` → configured URL parameter → protected preference Cookie → configured header → application default and candidate fallback. `None` continues to the next source. Configuration uses the existing repository and configuration cache; changing the source or environment does not override an already loaded cache. Rebuild the configuration cache after changing `APP_LOCALE_HEADER`. Both header negotiation and URL language selection are configurable.

`Request::get_languages` and `get_preferred_language` always describe `Accept-Language`, regardless of this application setting. Only text using translation resources changes with the locale. Enabled header negotiation adds the configured header name to `Vary`, preserving existing values and `*`; `null` adds no language-header entry and keeps URL switching, callbacks and `Content-Language`. Cookie or user-based callbacks still need an appropriate private-cache policy and any additional `Vary` fields.

Use `App::current_locale()?` in business code, controllers and pages to read the actual application locale; request preferences can differ after URL, stored-preference callbacks or downstream `App::set_locale` selection. For example, with `APP_LOCALE_HEADER=X-Language`, `Accept-Language: en` and `X-Language: zh-Hant`, Request reports English preferences while App and translated content use Traditional Chinese. With SetLocale disabled, this configuration alone performs no negotiation.

### Custom Locale Selection {#custom-locale-selection}

For a custom negotiation header, use the existing `APP_LOCALE_HEADER=X-Locale`; leave it unset to negotiate `Accept-Language`. The header is checked after URL and protected Cookie preferences. A configured custom header missing from a request falls back to the application default or first candidate, without retrying `Accept-Language`. This differs from a first-priority callback. Configure ordinary candidate lists above; reserve callbacks for business rules such as user or tenant preferences.

Use `locale_using` to configure a synchronous callback on the current application's middleware instance. For example, prefer an explicit request header and continue with URL selection, the protected preference Cookie and header negotiation when it is absent:

```rust
.with_middleware(|middleware| {
    middleware.locale_using(|request| {
        Ok(request.header("X-Locale")?.map(str::to_owned))
    });
})
```

The callback returns `Result<Option<String>, HttpError>`:

- `Ok(Some(locale))` canonicalizes the BCP 47 tag and requires an exact match in the canonical candidate list. It does not apply browser aliases, base-language matching, or translation fallback. With default candidates, `ZH-hANT` selects `zh-Hant`; `zh-CN`, `en-US`, empty strings, and invalid tags are errors. Include a valid region or extension tag explicitly to select it unchanged.
- `Ok(None)` tries the configured URL parameter first, then the protected preference Cookie and existing browser negotiation and supported application default or first candidate.
- `Err(error)` passes the original error to the existing exception handler and stops downstream execution. Invalid or unsupported selections are internal errors; validate untrusted user input in the callback and return an appropriate HTTP error if needed. Selection failures do not switch locale or emit `LocaleUpdated`.

Without a callback, behavior stays unchanged. Candidate and callback configuration are independent and can be called in either order. Invalid candidate updates preserve both previous settings; the last callback replaces the previous callback. Empty candidates disable the entire instance, including callback execution, events and header handling. The callback remains stored for re-enabling candidates. Route instances expose the same `set_locale` and `locale_using` methods; callbacks and candidate changes belong to that instance, never a process-wide registry. The callback itself must also avoid shared mutable user state.

The default instance runs in the resolved route pipeline, after direct Cookie decryption/queue middleware and before ordinary Web Session and authentication. With the stateful API wrapper it runs inside that wrapper, after its Cookie/Session protection. Routes without these dependencies remain stateless. Unmatched error pages and public static responses still use header/URL selection without Web storage. A callback can read headers and information already prepared at that point; it cannot access dependencies provided later. For Session rules, first call `middleware.set_locale(&[]).expect("语言标签有效")` in `with_middleware`. Then register the same middleware in `routes/web.rs`, after the default `web` group has loaded Session:

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
    locale.locale_using(|request| {
        Ok(request.session()?.get("locale")
            .and_then(|value| value.as_str().map(str::to_owned)))
    });
    Route::group(RouteGroup::new().middleware(Arc::new(locale)), |router| {
        router.get("/account", || Response::from("Account"))?;
        Ok(())
    })
}
```

This example requires a configured Session driver and application encryption key. It reads an existing `locale` Session value and does not persist a new preference. If no value exists, URL selection, the protected preference Cookie and then header negotiation apply. Authentication-dependent rules likewise belong after authentication; a rule affecting localized route bindings must run before binding substitution. For that placement, use the existing `Middleware::group` / `MiddlewareEntry` configuration to explicitly order the group. Merely adding a callback does not reorder middleware. Perform asynchronous lookups in an earlier middleware and pass prepared data through the request; this callback is synchronous.

The same `SetLocale` applies `App::set_locale`, events and response headers for these sources. Downstream locale changes determine a missing `Content-Language`; existing values remain intact. The configured language header is merged into `Vary` when enabled, since the callback can fall back to header negotiation. This does **not** fully vary a response selected by `X-Locale`, Cookie, Session or user data. For the header example, the response must also vary by `X-Locale`; for private content, apply `Cache-Control: private, no-store` or an appropriate application cache policy. Add other `Vary` fields when relevant. The middleware preserves those headers and does not infer cache keys, persist callback results, or make arbitrary personalized responses safe for shared caches. Protected Cookie preference pages additionally receive `private, no-store` and `Vary: Cookie`.

### Translating in Blade {#blade-translations}

Read translations directly in templates. The controller only supplies page data:

```blade
<h1>{{ __('welcome.heading') }}</h1>
<p>{{ trans('messages.greeting', ['name' => $name]) }}</p>
<p>{{ trans_choice('messages.items', $count) }}</p>
```

`__` and `trans` accept a string key, an optional replacement array, and an optional locale. `trans_choice` adds a numeric or array count after the key. Omitting the locale or passing `null` or an empty string uses the current request language; an explicit locale affects only that call. Replacement values may be strings, numbers, booleans, or `null`. Missing keys and fallback languages follow the same rules as `Lang`.

<code v-pre>{{ ... }}</code> escapes HTML. `@lang('messages.title')` and `@choice('messages.items', $count)` output raw text, as does `{!! __('messages.title') !!}`; use these only with trusted HTML and replacement values. Includes and components inherit the rendering language, and compiled templates resolve translations at render time. The helpers require an active application translation service. Helpers return text or a translation group/list. Use groups and lists in `@foreach` or read an entry with an array offset; directly echoing an array returns an error. Rust callers can use `Lang::array`.

`@lang ... @endlang` renders a translation block, trims surrounding whitespace, and translates the resulting key. Use `@lang(['name' => $name]) ... @endlang` for replacements; output is raw text. Nested blocks share replacement state: starting an inner block replaces the outer replacement array. `__()` and `__(null)` return `null`, which echoes as empty text. `@lang('messages.title', [], null, false)` disables group fallback for that call. For plural translations, `count => null` uses the actual count, while explicit zero and empty strings remain replacements. `trans()` / `trans(null)` would return a localizer object, which the template value model does not expose; use `Lang` from Rust instead. Arbitrary PHP functions, facade method calls, and named arguments are not implemented. Argument counts and types are checked explicitly rather than relying on PHP coercion.

## Publishing Language Files {#publishing-the-language-files}

Run `./artisan lang:publish` to publish all eight bundled groups and notification sentence catalogs for the 18 bundled locales. The command uses `lang/`, or `resources/lang/` when that directory already exists. Existing files are preserved by default. `--force` replaces the bundled files; `--existing` only replaces bundled files already present. When both flags are set, `--existing` determines the scope. Custom groups and other locales are preserved.

## Defining Translation Strings

Put grouped translations in files such as `lang/en/messages.json`. Retrieve nested entries with dot notation. Use `lang/en.json` for translations whose keys are complete phrases. When `resources/lang` exists, it is used as the application language directory.

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

### Registering Paths and Translation Lines {#registering-translation-sources}

Register directories through `Lang::add_path`, `Lang::add_json_path`, or `Lang::add_namespace` in the application scope before the first lookup. Later paths override earlier paths. Grouped catalogs merge recursively, including list indexes. Literal JSON keys are replaced as a whole. Application roots are applied after JSON-only roots.

Use `Lang::add_lines` with dotted group keys. A newly registered group starts with the supplied lines. To extend a file-backed group, call `Lang::load` first. Loaded groups are cached for the application, so later path registration applies to future loads.

```rust
use std::path::PathBuf;

use openrust::{
    support::facades::Lang,
    localization::{Line, TranslationResult},
};

pub fn register_translations(directory: PathBuf) -> TranslationResult<()> {
    Lang::add_path(directory)?;
    Lang::load(None, "messages", "en")?;
    Lang::add_lines(
        [("messages.runtime".into(), Line::Text("Registered :name".into()))],
        "en",
        None,
    )
}
```

### Loading Groups From a Custom Source {#loading-custom-translation-sources}

Pass a `Loader` to `Lang::load_from` at an async application boundary. Each resource identifies one locale, group, and optional namespace. The returned lines replace the selected groups together. A loader error or a concurrent runtime write to one of those groups leaves the batch unpublished. Subsequent lookups stay synchronous.

```rust
use openrust::{
    contracts::localization::Loader,
    support::facades::Lang,
    localization::{TranslationResource, TranslationResult},
};

pub async fn load_notice(source: &dyn Loader) -> TranslationResult<()> {
    Lang::load_from(
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

Add `openrust-localization-database` to the application's Cargo dependencies using `path = "vendor/openrust/localization-database"`. `DatabaseLoader` accepts an explicit framework `Connection`; obtain it from the application's database manager or `Db::connection`. It reads translations from PostgreSQL 18, MySQL 8.4, and SQLite. Other database targets require separate behavior validation.

Create the table through an application migration before loading translations. This minimal schema is supported:

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_translations(connection: &Connection) -> DatabaseResult<()> {
    connection.get_schema_builder().create("language_lines", |table| {
        table.id();
        table.string("group", None).index();
        table.string("key", None);
        table.json("text");
        table.timestamps();
    }).await
}
```

The schema follows Spatie’s the framework Translation Loader: one row per group/key; `text` is a JSON object such as `{"en":"Hello :name","fr":"Bonjour :name"}`. Use canonical locale keys (`en-US`, `zh-Hans`). A missing or null locale leaves the file value available. Translation values must be strings; empty strings are valid. Group `*` stores literal keys, including dots. Other groups expand dotted keys such as `nested.title` or `items.0`. Package namespaces use only the file source, matching Spatie. Duplicate or overlapping keys, invalid JSON, empty key segments, and keys deeper than 128 segments fail the load.

Call this function inside an asynchronous application scope, such as a provider's `boot`, after migrations are available:

```rust
use std::{path::PathBuf, sync::Arc};
use openrust::{
    database::Connection,
    support::facades::Lang,
    localization::{FileLoader, TranslationResource, TranslationResult},
};
use openrust_localization_database::DatabaseLoader;

pub async fn load_database_translations(
    connection: Connection,
    language_directory: PathBuf,
) -> TranslationResult<()> {
    let source = DatabaseLoader::new(connection)
        .with_table("language_lines")?
        .with_max_rows(10_000)?
        .with_fallback(Arc::new(FileLoader::new(language_directory)));
    Lang::load_from(
        &source,
        &[
            TranslationResource { locale: "fr", group: "messages", namespace: None },
            TranslationResource { locale: "en", group: "messages", namespace: None },
        ],
    ).await
}
```

The selected connection's table prefix applies. The table name is an unqualified identifier. The default table is `language_lines`, and the default maximum is 10,000 rows per group. Exceeding the configured limit fails the load without truncating the result.

`with_fallback` loads the same locale/group/namespace from the file source and overlays database entries. Groups and list indexes merge recursively; literal JSON values are replaced as a whole. Empty database strings remain valid overrides. After deleting a row, explicitly reload its resource to restore the file value. The localizer retains its configured language fallback; load database resources for both the selected and fallback languages when needed.

`Lang::load_from` publishes all requested groups together. Database, file, or parsing failures leave the previous published catalog available and return an error; missing rows produce an empty database overlay. Reads occur per group; publication atomicity does not provide a transaction spanning all database reads. Reloads are explicit. Applications manage writes, migrations, and refresh timing; the package does not automatically select a source or start background tasks.

Unlike Spatie’s PHP model, this source does not perform locale fallback while reading each row: the localizer applies its configured fallback after loading exact locales. This also preserves file translations for the selected locale. A missing table returns an error so a failed explicit reload cannot silently replace a published database catalog with files.

### Default database source

For a new application, publish and run the migration while file mode is active. Applications with an existing table keep their original migration and data.

```sh
./artisan vendor:publish --tag=localization-database-migrations
./artisan migrate
```

Bind the default source in an application provider’s `register` method. The example uses the default database connection and the same `FileLoader` used by application and package language paths.

```rust
use std::sync::Arc;
use openrust::{
    contracts::{container::{BindingResolutionError, Container as _}, foundation::Application as _, localization::Loader},
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    localization::FileLoader,
    support::{ProviderResult, ServiceProvider},
};
use openrust_localization_database::DatabaseLoader;

struct LanguageSource;

impl ServiceProvider for LanguageSource {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().singleton_named::<dyn Loader, _>("lang.loader", |container| {
                let files = container.make::<FileLoader>()?;
                let connection = container.make::<DatabaseManager>()?.connection(None).map_err(|source| {
                    BindingResolutionError::FactoryFailed { service: "lang.loader", source: Box::new(source) }
                })?;
                Ok(Arc::new(DatabaseLoader::new(connection).with_fallback(files)))
            })?;
            Ok(())
        })
    }
}
```

Add a `LanguageSource` instance to the application’s provider list. Insert the `resources` value under the `lang` key in the configuration loader’s returned map.

```rust
use serde_json::json;

let resources = json!({"resources":[
    {"locale":"fr","group":"messages"},
    {"locale":"en","group":"messages"},
    {"locale":"fr","group":"*"}
]});
```

The application loads declared groups after providers boot. `Lang` queries remain synchronous. Database updates become visible after `Lang::refresh_default_source().await` or a restart. A failed refresh retains the prior snapshot. Runtime entries added with `add_lines` and `load_from` take precedence. Set `lang.resources` to an empty array for file mode. If a cached declaration prevents startup during an outage, deploy the empty list and point `APP_CONFIG_CACHE` at an unused absolute path before restarting. Rebuild the intended cache after recovery. The cache stores declarations. Each process loads the terms at startup.

## Retrieving Translation Strings

Use `__` to retrieve text. Placeholders support `:name`, `:Name` and `:NAME`. Use `Lang::array` for a group or list. A missing key is returned as text. Literal JSON keys are looked up in the selected locale. Grouped keys can fall back to the configured language.

### Fallback and Missing Keys {#translation-lookup-hooks}

Use `Lang::has(key, locale, true)` to check fallback translations and `Lang::has_for_locale(key, locale)` for an exact locale. Existence checks skip the missing-key handler. Register lookup hooks inside the application scope. The locale callback can reorder grouped lookups. A literal JSON match takes precedence. The missing-key handler can return replacement text, which then receives normal placeholder replacements. Returning `None` keeps the original key.

```rust
use openrust::{support::facades::Lang, localization::TranslationResult};

pub fn register_lookup_hooks() -> TranslationResult<()> {
    Lang::determine_locales_using(|locales| {
        let mut locales = locales.to_vec();
        if locales.first().is_some_and(|locale| locale == "fr-CA") {
            locales.insert(1, "fr".into());
        }
        locales
    })?;

    Lang::handle_missing_keys_using(|missing| {
        (missing.key == "messages.welcome").then(|| "Hello, :name".into())
    })
}
```

### Replacing Parameters in Translation Strings {#replacing-parameters-in-translation-strings}

Use `Lang::get_with` to pass an ordered list of text, tags, and objects. A callback receives the contents of each matching `<name>…</name>` on the same line. Tag callbacks run in argument order before text placeholders are substituted. Use `Lang::array_with` to replace values within a group or list. `Lang::choice_with` picks the plural form before applying replacements.

#### Object Replacement Formatting {#object-replacement-formatting}

Register a formatter for the concrete Rust type with `Lang::stringable` in the application scope, typically in `AppServiceProvider::boot`. Use `Replacement::object` when registration is required, or `Replacement::display` to use the type’s `Display` implementation when no formatter is registered. To use a Rust enum, pass the desired variant name or value explicitly, or register a formatter for the enum.

For example, define `payments.notice` in `lang/en/payments.json`.

```json
{"notice": "<mark>Paid</mark> :amount"}
```

```rust
use openrust::{
    support::facades::Lang,
    localization::{Replacement, TranslationResult},
};

pub struct Amount(pub u32);

pub fn register_formatters() -> TranslationResult<()> {
    Lang::stringable(|amount: &Amount| format!("{} USD", amount.0))
}

pub fn payment_notice(amount: &Amount) -> TranslationResult<String> {
    Lang::get_with(
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
    localization::{TranslationResult, __, trans_choice},
};

pub async fn summary(locale: &str) -> TranslationResult<(String, String)> {
    Lang::with_locale(locale, async {
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

Artisan uses `APP_LOCALE` for list headings, help and argument errors. The application ships console catalogs for the 18 locales listed below. Override text in `lang/<locale>/console.json`. Lookup uses the active locale and then `APP_FALLBACK_LOCALE` before built-in text. A command without a translated description uses its registered description.

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh-Hans ./artisan list make
APP_LOCALE=zh-Hant ./artisan help list
```

To customize help, use keys such as `options`, `commands.list.description` and `commands.list.arguments.namespace` inside the console group. See [Artisan output formats](./artisan.md#command-help-and-output-formats).

Production confirmations and cancellation messages use the same translation group. Override the message with `confirm.production` or `confirm.cancelled` in `console.json`. Answer with `y` or `yes` to proceed. An empty answer declines.

## Locale contract methods

The Localizer contract, concrete localizer, and Lang facade expose `get_locale`, `set_locale`, `get_fallback`, and `set_fallback`. Boot-time changes configure application defaults; request/queue changes stay in the current scope. Nested `with_locale` restores its caller’s locale; fallback is set separately. Rust methods use native snake_case names; update older camelCase Rust callers to these methods. Existing template expressions keep their own names.

## Locale Ownership

Locale and fallback getters return owned `String` snapshots. Setters take `&self` and return `TranslationResult<()>`; invalid tags return an error. Normalization keeps `zh-CN` distinct from `zh-Hans`. Boot-time values form the defaults; request and queue-task changes stay in that container scope. `with_locale` temporarily overrides only the active task and restores the enclosing locale after completion.

## Upgrading Locale Identifiers {#bcp47-upgrade}

Pause new jobs and drain old queued mail/notification jobs before stopping the old web and queue processes. If draining is impossible, retain those jobs for application-specific migration. Back up configuration, catalogs and affected data before changing resources or stored locale values, and do not mix old and new processes. Update `APP_LOCALE`, `APP_FALLBACK_LOCALE` and `APP_FAKER_LOCALE` explicitly; the application never edits your `.env` automatically.

| Existing project value/path | Replacement |
| --- | --- |
| `zh_CN`, `lang/zh_CN`, `docs/zh_CN` | `zh-Hans`, `lang/zh-Hans`, `docs/zh-Hans` |
| `zh_TW`, `lang/zh_TW`, `docs/zh_TW` | `zh-Hant`, `lang/zh-Hant`, `docs/zh-Hant` |
| Regional `en_US`, `en_GB`, `zh_HK`, `zh_Hant_HK` | `en-US`, `en-GB`, `zh-HK`, `zh-Hant-HK` |

The first two mappings apply only to this project's default catalogs and confirmed data. Valid `zh-CN` and `zh-TW` remain regional tags. Rename sentence-key JSON files and package overrides as well as grouped directories. Detect duplicate destination files or JSON keys before moving anything; preserve both versions for manual resolution. No old URL redirects or underscore aliases are installed by default; configure any transitional redirects at your hosting layer.

Clear old configuration and view caches with the old binary (`./main artisan config:clear`, `./main artisan view:clear`) before replacing it, or use the existing source `./artisan` entrypoint. Deploy the new code and resources together, rebuild with `config:cache` and `view:cache` using the new configuration, and restart all long-lived processes. Translation directory caches are process-local and disappear on restart. Drain old queued mail/notification jobs before upgrading, or inspect their serialized `locale` fields with application-specific tooling; encrypted payloads require the old application's encryption keys. A generic text replacement in queue, cache or settings blobs is unsafe.

For database translations, inventory `language_lines.text` JSON keys and any application-owned settings or locale columns first. The templates in `vendor/openrust/localization-database/database/upgrade-bcp47/` include separate read-only inventory queries and PostgreSQL, MySQL and SQLite transactions. They map the two default catalogs plus `en_US`/`en_GB`; customize table names and any other legacy locale mappings after review. Unknown underscore keys or any old/new collision abort the transaction, including equal or null values; no content is overwritten. The final statement is **ROLLBACK** by default. Inspect the result and backup before explicitly changing it to **COMMIT**. On error roll back and close that dedicated database session. Re-running a successful migration is a no-op; rollback after a committed migration requires restoring the backup, since existing new-format keys cannot safely be distinguished from migrated ones.

Faker keeps native provider keys private; `locale()` and `random_locale()` return BCP 47. Script-only `zh-Hans`/`zh-Hant` use the corresponding existing Chinese providers, while the public label retains its original region and extensions. Third-party protocols such as WeChat's native `lang=zh_CN` are unchanged at that boundary.


## Replacing plural selection

`Localizer::get_selector()` returns the active `Arc<dyn Selector>`. Implement `openrust::localization::Selector` and pass the instance to `set_selector` to customize plural branch selection. Its `choose` method receives the untranslated line, `PluralCount`, and resolved locale, and returns `TranslationResult<String>`. Replacement of `:count` and other placeholders happens afterward. Restore the default with `set_selector(MessageSelector)`. `get_loader()` returns the actual shared `Arc<FileLoader>`; previously returned selector and loader handles remain valid.

## Bundled Language Resources {#bundled-locales}

`en`, `zh-Hans`, `zh-Hant`, `ja`, `ko`, `fr`, `de`, `es`, `pt`, `it`, `ru`, `ar`, `hi`, `id`, `vi`, `th`, `tr`, `nl`.

Deploy the binary, welcome template and any application language overrides together; built-in translations do not need a separate lang/ tree. Rebuild configuration and view caches and restart workers. Explicit candidate lists remain application-controlled. Resources, number formatting, plural selection and regional Faker datasets are separate capabilities; configure Faker independently.


### Locale capability boundaries

| Capability | Scope |
| --- | --- |
| Runtime resources | All 18 bundled locales have welcome, language names, console, schema, authentication, password, pagination and validation groups, plus notification sentence catalogs. |
| Plural selection | Uses the existing message selector. Arabic uses six branches, Russian three; invariant languages use the first branch. Decimal modulo behavior follows the selector contract, independently of ICU number formatting. |
| Numbers | Decimal formatting accepts all 18 locale tags through ICU. This does not promise localized spelling for every language. |
| Faker | Configure `app.faker_locale` independently. A generic UI locale such as `fr` does not automatically select `fr-FR`; formatter-specific regional datasets and fallback behavior remain in effect. `zh-Hans` / `zh-Hant` retain the existing Chinese provider mappings. |
| Developer documentation | English, Simplified Chinese and Traditional Chinese. Other welcome-page languages link to the English documentation. |

The welcome view reads the documentation directory from `welcome.documentation_locale` in the active language resources. Bundled languages without corresponding documentation use `en`; a custom language without this key also uses `en`. When adding documentation for another language, override this key in that language’s application `welcome.json` (for example, `"documentation_locale": "ja"`). This changes only the documentation links, not the page language.

The welcome description uses a `:name` placeholder. The view passes `config('app.name')`, so changing `APP_NAME` updates the introduction in every bundled language; the rendered text is HTML-escaped.

Standard translations retain the bundled MIT attribution. Additional framework translations are technically checked translations, including machine-assisted drafts; native-language editorial review is separate. Neither local capture nor queue delivery tests certify delivery through an external mail provider.

## Language directories

At creation, the application uses `resources/lang/` when that directory exists; otherwise it selects the root `lang/`. Path selection does not create directories. The application contract exposes `lang_path("")` and accepts a suffix such as `lang_path("en/validation.json")`.

To override the directory, set it during application assembly, before resolving the translation loader:

```rust
use openrust::contracts::foundation::Application as _;

app.use_lang_path(app.base_path().join("translations"))?;
```

`App::lang_path("")` and `openrust::support::lang_path("")` read the same application path inside an HTTP, Artisan or queue scope. `App::use_lang_path(...)` is also available. These paths are application-wide; do not switch them per request. A loader that has already been resolved keeps its original directory, so configure paths before loading services rather than expecting live reload.

`lang:publish` writes to this directory and preserves existing files by default. `--force` overwrites available templates, while `--existing` overwrites only templates already published. For package resources, use `app.lang_path("vendor/package/...")` as the destination passed to `publishes_paths`; `vendor:publish` supports explicitly configured absolute paths, including directories outside the application. Relative traversal and symbolic links remain rejected. Application package overrides are loaded from this same language directory.
