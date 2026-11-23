# HTTP Requests

## Request Headers {#request-headers}

Use `request.header("X-Trace")?` to read the first text value or `None` if the header is absent. Invalid names or non-UTF-8 values return HTTP 400 when the error is rendered by the kernel. Use `has_header` for presence, `headers()` for the complete header map and `bearer_token()?` to extract a bearer token from Authorization.

### Browser Language Preferences {#language-preferences}

`request.get_languages()` returns valid `Accept-Language` ranges in descending quality order, preserving input order for equal weights. Tags use BCP 47, such as `en-US` and `zh-Hant`; `*` remains a wildcard. Repeated header fields are combined, and repeated tags keep their highest valid weight. Invalid tags, malformed or duplicate quality parameters, and `q=0` entries are ignored. A quality must be between 0 and 1 with at most three decimal places. With no valid ranges, the list is empty.

```rust,ignore
let languages = request.get_languages();
let preferred = request.get_preferred_language(Some(&["en", "fr-FR"]))?;
```

`get_preferred_language` checks each preference against ordered candidates, starting with the full tag, then language/script/region combinations and the base language. A wildcard or no match selects the first candidate. With `None` or an empty candidate list, it returns the first request range, or `None` when absent. Candidates use the same BCP 47 validation as other locale APIs; an invalid candidate returns a translation error, even if an earlier candidate matches. The returned value is an owned `Option<String>` inside a `Result`. This is a preference helper, not a 406 rejection mechanism: ignored `q=0` ranges do not form a persistent exclusion list, and the ordered fallback still applies.

These methods always read standard `Accept-Language`, independently of `app.locale_header`, and do not change the application locale or infer Chinese script from region. Use `App::current_locale()?` for the actual language used by business code, translation and pages. The global `SetLocale` middleware separately chooses a language from the configured sources; changing its header does not change these Request methods. Applications may override or disable its candidates with `middleware.set_locale(...)`; see [Application language negotiation](../digging-deeper/localization.md#locale-header). Parsed standard ranges are cached per request; changing headers through `parts_mut()` invalidates that cache. Custom negotiation headers share the parser without using this standard-header cache.

## Request Path, Host and URL {#request-path-host-and-method}

`path()` strips surrounding slashes. `decoded_path()` also percent-decodes the path. Use `segments()` or the one-based `segment(index)` to read its components. `is(pattern)` checks the decoded path, `route_is(&["reports.*"])` checks the matched route name, and `is_method("GET")` checks the effective HTTP method.

`host()?`, `http_host()?` and `scheme_and_http_host()?` use the effective connection and trusted proxy settings. The HTTP host includes a non-default port. `root()?` also includes the trusted proxy base path. `url()?` returns the current URL without a query string. `full_url()?` and `full_url_is(patterns)?` use a normalized query string. Use `full_url_with_query(&serde_json::json!({"page": 2}))?` or `full_url_without_query(&["page"])?` to build a URL from the request query data. These helpers return a new URL string. Signature verification uses the original request target.

## Retrieving Input

### Retrieving All Input Data {#retrieving-all-input-data}

Use `request.all().await?` to retrieve ordinary input and uploaded files together. `RequestValue` holds JSON scalars, ordered groups with integer or string keys, and typed `UploadedFile` objects.

When a regular input value and an upload share a key, the input value wins, including explicit null. Nested groups retain fields supplied on either side. The upload is still available through the file bag.

Use `request.all_only(&["items.1.photo", "name"]).await?` to select paths and retain missing fields as null. Use `request.keys().await?` to list input keys followed by file keys. A name supplied by both sources appears twice.

### Retrieving an Input Value {#retrieving-an-input-value}

Keys can be passed directly; existing `Some(key)` and `None` calls remain valid. Use `request.input_or("name", "Guest").await?`, `request.query_or("page", "1")?`, and `request.post_or("name", "Guest").await?` for defaults. `input_or` preserves explicit null. `query_or` and `post_or` use the fallback for missing or null values, matching their default-value contract. All preserve empty strings, zero, and false. Parsing failures still propagate as errors. For query parameters prefer `query`, rather than introducing a second `get` alias.

```rust
let name = request.string_or("name", "Guest").await?;
let page = request.integer_or("page", 1).await?;
let remember = request.boolean("remember").await?;
let keyword = request.query_or("keyword", "")?;
let title = request.post_or("title", "").await?;
```

Use `request.query("key")` to read an exact top-level query key. Use `request.input("team.name").await` for nested input. Pass `None` to retrieve the input bag. JSON and form fields take precedence over query values at the top level. Use `request.json` to read the JSON body alone.

Use `request.post(None).await?` for the body parameter bag or `request.post("name").await?` for an exact top-level key. On a JSON request this reads the decoded JSON body. Query parameters and multipart file objects use their own bags. An explicit null remains in the full body bag, and a named lookup of that value returns `None`.

Use `request.fluent(Some("user")).await?` to wrap a group of ordinary input as an independent `Fluent` bag. Pass `None` for all ordinary input, or use `fluent_or("user", default)` for a default attribute bag if the field is missing or null. A scalar cannot initialize the bag and returns HTTP 400. Use `fluent_only(&["user.name", "document"]).await?` for an independent typed snapshot that retains uploads. Use `get` for paths and `value` for exact top-level keys.

Use `scope("user")` for an independent nested bag. Scalar and file scopes use index `0`. Use `set("user.name", RequestValue::Value(serde_json::json!("Ada")))?` to edit a typed bag. Wildcard paths update existing children, and the original request keeps its input.

OpenRust preserves the body bytes for subsequent reads after successful collection. The default limit is 8 MiB. Invalid input returns 400, and oversized input returns 413.

### Request Boundaries {#request-boundaries}

The kernel rejects a path with invalid UTF-8 after percent decoding with HTTP 400. A declared `Content-Length` over 8 MiB returns HTTP 413 before the framework reads the body. When the framework collects a body for input parsing or `body_bytes()`, the actual bytes are also capped at 8 MiB, including chunked requests.

### Typed Input Values

Use `request.integer("page").await?`, `request.float("price").await?`, `request.boolean("remember").await?`, or `request.string("name").await?`. Missing values use `0`, `0.0`, `false`, and an empty string respectively. Custom defaults use `integer_or("page", 1)`, `float_or("price", 1.25)`, `boolean_or("remember", true)`, or `string_or("name", "Guest")`, followed by `.await?`. Defaults do not replace explicit null; ordinary conversions still apply to present values.

Use `integer_strict("value", 42)`, `float_strict("value", 1.25)`, `boolean_strict("value", true)`, or `string_strict("label", "fallback")` for strict conversion with a fallback. These methods use the fallback for a missing or invalid value; strict mode is not validation and does not produce a 422 response. Dotted paths and input precedence are unchanged. `str`, `str_or`, and `str_strict` are aliases for the corresponding string methods.

A missing value uses the fallback. With strict mode enabled, invalid values also use the fallback. For example, `"01"` is invalid as a strict integer but is valid as a float. With strict mode disabled, numeric reads use PHP casts and can read a leading numeric portion. Boolean reads recognize `1`, `true`, `on`, and `yes` case-insensitively. The recognized false values are `0`, `false`, `off`, and `no`. Unrecognized boolean input returns false in permissive mode. Malformed request bodies still return an input error.

Use `request.string_strict("label", "fallback").await?.trim()` for a fluent string. `request.str` is an alias. Strict strings accept text and numbers. `request.array(Some("items")).await?` preserves lists and keyed objects, wraps scalars into a one-item list, and returns an empty list for null or missing values. `request.collect` returns a collection with the same keys. Pass `None` to read the input bag.

Implement `BackedEnum` with a `String` or `i64` backing. Use `request.r#enum::<Stage>("stage", None).await?` for one case, or `request.enums::<Stage>("list").await?` for a collection. The list keeps original keys and duplicate cases while skipping unknown cases. A single unknown case uses the supplied fallback. An invalid backing type returns HTTP 400.

Use `request.date("when", None, None).await?` to parse a date using the application's time zone. Pass a PHP format and an explicit time zone with `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?`. Missing, null, and blank values return `None`. Invalid dates return HTTP 400. The parsed date preserves its resolved time zone and microseconds.

In a test fixture, `TestApp::travel_to` and the freeze helpers set the reference for relative dates and components omitted by the format. For example, `tomorrow` is resolved from the fixture's frozen date in the requested time zone. A format starting with `!` resets omitted fields. See [Interacting with time](../testing/mocking.md#interacting-with-time).

Use `request.interval("duration", None).await?` for ISO 8601 intervals or expressions such as `2 hours 30 minutes`. Use `request.interval("seconds", Some("second")).await?` when a field supplies a value in a named unit. The result preserves calendar components and microseconds. An explicit value of 90 seconds stays in the seconds field. Blank or missing input returns `None`. Invalid interval syntax or units return HTTP 400.

### Retrieving Input via Dynamic Properties {#retrieving-input-via-dynamic-properties}

Use `request.field("name").await?` for the lookup the framework performs on dynamic request properties. It reads merged input and files first, including explicit nulls. If the field is absent, it looks for a bound model or raw parameter with that name on the matched route. Match `RequestField::RouteModel(model)` and use `model.downcast::<T>()` for a bound model. Ordinary fields, uploads and raw route parameters use `RequestField::Value`.

### Retrieving a Portion of the Input Data {#retrieving-a-portion-of-the-input-data}

Use `request.only(&["user.name", "document"]).await?` to select fields from input and uploads. It omits missing paths and preserves explicit null and typed files. `request.except(&["document"]).await?` removes fields without renumbering numeric keys. Use `array_only` or `collect_only` to return the selected data as a typed array or collection.

## Request facade {#request-facade}

Route callbacks without a request argument can use `openrust::support::facades::Request` directly. The facade borrows the original request: input, uploads, authentication and Session operations share its state without eagerly consuming or copying the body.

```rust
use openrust::{http::HttpError, support::facades::{Request, Route}};
use serde_json::json;

Route::post("/profile", || async {
    let name = Request::input("name").await?;
    Request::merge(json!({"verified": true})).await?;
    Ok::<_, HttpError>(json!({"name": name}))
})?;
```

Register a controller action without a request argument using `ControllerDispatcher::without_request(Controller::show)`; it still returns `HttpFuture<'_, Result<T, HttpError>>`. The action and its services can use the facade directly. A handler accepting `&mut http::Request` already borrows the request exclusively. Use the instance, or lend it to a facade-based service with `request.with_container(async { /* call service */ }).await`. Completion and cancellation restore the request. Inside `RequestHandle::with_mut` or `with_async`, use the request passed to the callback instead of borrowing the facade again.

| Capability | Example entry points |
| --- | --- |
| Metadata and URLs | `method`, `uri`, `path`, `header`, `full_url`, `get_port` |
| Input and uploads | `input`, `all`, `only`, `file`, `all_files`, `merge`, `replace` |
| Typed input | `integer`, `string`, `r#enum`, `date`, `interval`, `clamp` |
| Content negotiation | `accepts`, `prefers`, `get_format`, `get_preferred_format` |
| Session and flash | `session`, `old`, `flash`, `flash_only`, `flush` |
| Users and routes | `user`, `route`, `route_parameter`, `set_user_resolver`, `set_route_resolver` |
| Validation and signatures | `validate`, `validate_with_bag`, `has_valid_signature` |
| Extensions | `r#macro`, `mixin`, `has_macro`, `flush_macros`, `call` |

`session(|store| ...)` and `get_session(|store| ...)` access the Store within one protected borrow; references cannot escape. `route()` returns an owned `ResolvedRoute` containing its name, metadata, parameters and model bindings. Use `route_parameter(name)` for a named parameter or model. `UserResolver` and `RouteResolver` are typed, request-local callbacks. A user resolver receives the mutable request and optional guard and returns an asynchronous `HttpFuture<AuthResult<...>>`, so it can await application lookups. `set_json` accepts only an object or array and preserves the previous input when validation fails. Conditional input callbacks return `None` for an unselected branch and release their internal borrow before invoking application code.

`instance()` and `Facade::get_facade_root()` return a `RequestHandle` supporting `with`, `with_mut` and `with_async`. A handle can be reused across lending boundaries within the same HTTP request, including constructor injection, but is accessible only while that request is lent. It becomes invalid when the HTTP request ends. The container's `request` name and `RequestHandle` type resolve the current handle without caching it across requests. `Facade::swap(Arc::new(RequestHandle::from_request(request)))` replaces the current borrowed request for explicit reconstruction or tests, without replacing other requests' container bindings.

`get_payload()` reads the form or JSON payload without query parameters; `get_content()` returns bounded raw bytes. `uri()` returns a full `support::Uri`; use the request instance's `uri()` for the original transport URI. Request locale setters use BCP 47 independently of `App::set_locale`; custom formats are request-local as well. `dump` prints selected data; `dd` ends the current response through an HTTP exception instead of terminating the server.

### Native server configuration and construction {#request-facade-native-boundaries}

Process-wide PHP settings are represented by explicit Rust configuration rather than mutable static state shared by concurrent requests.

| Original mechanism | OpenRust entry point |
| --- | --- |
| Capture globals, global factories and writing globals | Pass the server's explicit `http::Request<Body>` to `Request::capture`, `create` or `create_from_base`. Custom factories are ordinary functions or closures; PHP globals are neither read nor modified |
| Construct from a framework request | `create_from` transfers ownership of the framework request, preserving parsed input, uploads and Session |
| Duplicate or reinitialize | `duplicate().await` exports an independent `http::Request<Bytes>` transport snapshot; `initialize(native_request)` replaces the current borrowed request. A snapshot retains the original body, excludes later input overrides, Session, route models and middleware state, and is not a complete framework-request clone. Use the handle when the original framework state is required |
| Global trusted proxy/host setters and getters | Own the policy values in `bootstrap/app.rs` and configure `trust_proxies`/`trust_hosts` through `with_middleware`, or use the existing `trustedproxy` configuration; no process-global Request switches |
| Method override settings | Construct and validate `MethodOverride::new(parameter, allowed)` outside the assembly callback, then call `middleware.method_override(policy)`: `None` permits targets under the existing safety rules, an empty list disables all overrides, and `parameter=false` disables only `_method`. The allowlist also applies to the override header. GET, HEAD, CONNECT and TRACE cannot be allowed |

`get_http_method_parameter_override()` and `get_allowed_http_method_override()` expose the effective per-application policy, applied before routing. On a native server without CGI, `get_script_name()` is empty and the base path comes from the trusted proxy prefix. `server(key)` exposes explicit transport metadata: `REMOTE_ADDR` is the original connection address; use `ip()` for the client address.

## Client IP addresses {#request-ip-address}

Use `request.ip()` for the first resolved client address and `request.ips()` for the address chain. A direct connection uses the transport peer, when available. With trusted proxies, the chain omits trusted hops and starts with the nearest untrusted address. If every hop is trusted, it contains the first trusted address instead. See [trusted proxy configuration](#configuring-trusted-proxies).

The IP facade is also available in controllers and services running inside the current HTTP request:

```rust
use openrust::support::facades::Request;

let ip = Request::ip()?;   // Option<std::net::IpAddr>
let ips = Request::ips()?; // Vec<std::net::IpAddr>
```

`Request::get_client_ip()` and `Request::get_client_ips()` expose the equivalent IP entry points; the request instance provides the same aliases. They use the connection information already resolved by the HTTP kernel and its trusted proxy policy. An HTTP request without transport address information returns `None` / an empty list. Outside the current HTTP request, the facade returns `BindingResolutionError::FacadeNotSet`; independently spawned tasks do not inherit this context. Pass the required address explicitly to background work. See the [Request facade](#request-facade) for other operations and borrowing boundaries.

## Content Negotiation {#content-negotiation}

Use `request.get_acceptable_content_types()` to read the `Accept` entries in quality order. Use `request.accepts(&["text/html", "application/json"])` to check a set of acceptable types, or `request.prefers(&["html", "json"])` to select the best candidate. `prefers` returns `None` when the header is absent or there is no match. `request.format("html")` uses the first recognized response format and falls back to the supplied value.

`accepts_any_content_type()`, `accepts_json()`, `accepts_html()` and `accepts_markdown()` check acceptance. `wants_json()` and `wants_markdown()` inspect the top-ranked entry. `expects_json()` also recognizes a non-PJAX XMLHttpRequest with an empty or wildcard `Accept` header. `is_json()` checks the request body's `Content-Type`.

With repeated `Accept` header fields, the first field is used, as in the pinned the framework request implementation. Send several types as a comma-separated list in one field.

### Native HTTP Requests {#native-http-requests}

The HTTP server passes each owned transport request to `Kernel::handle_transport`, which constructs the framework request with `Request::from(native)`. The kernel applies trusted proxies and method overrides within that request’s lifecycle.

For interoperability with Rust HTTP libraries, convert an owned `http::Request<Body>` using `Request::from(native)` and move it back with `into()`. Ownership conversion moves the current body stream along with its headers, protocol version and extensions. Inside a route, use `request.to_http_request().await?` for an independent native request with a buffered body. It uses the same 8 MiB limit as framework input, and the body remains available to the framework request.

The native request uses the transport URI from `original_uri()` and the effective method from `method()`, including any method override. Use `uri()` for the framework URI after trusted-proxy processing and `get_real_method()` for the original method. `version()` and `extensions()` read the corresponding transport metadata.

See [Native HTTP responses](./responses.md#native-http-responses) for returning a native response.

### Input Presence {#input-presence}

Use `request.has(&["profile.name"]).await?` when a key must exist, including a key explicitly set to `null`. Use `has_any` to check alternatives and `missing` to test whether any supplied key is absent. These checks also recognize uploaded files.

Use `filled`, `is_not_filled` and `any_filled` to check input values. Missing values, `null` and blank strings are empty. Booleans, zero and empty arrays count as filled. A file-only field is checked with `has_file`.

Use `when_has`, `when_filled` or `when_missing` to run an async callback when the condition holds. `when_has` and `when_filled` pass the combined field as `Option<RequestValue>`. Match `RequestValue::File(file)` to work with an upload. The method returns `Some(result)` when the callback runs. Use `unwrap_or_else` to supply a fallback.

### Merging Additional Input {#merging-additional-input}

Use `request.merge(serde_json::json!({"profile.name": "Ada"})).await?` to set values by dotted path. `merge_if_missing` preserves keys already present in the request, including explicit `null`, query fallbacks and uploads. Use `replace` with an object or array to replace the active input source.

A JSON request updates its JSON input. A non-JSON GET or HEAD updates the query bag. Other requests update their form input and still fall back to query values. These methods preserve the original URI, raw body and uploads.

## Old Input {#old-input}

In a route with session middleware, use `request.flash().await?` to store the current input for the next request. Use `request.flash_only(&["profile.name", "page"]).await?` to select specific fields or `request.flash_except(&["password", "password_confirmation"]).await?` to exclude fields. Selection supports dotted paths. Uploaded files remain in the file bag.

Read the result with `request.old(Some("profile.name"))` or retrieve the bag with `request.old(None)`. An exact key takes precedence over a dotted path. Explicit null remains available as `Some(Value::Null)`. Use `Option` methods to supply a fallback when the value is missing.

Use `request.flush()?` to replace the old input with an empty bag. The other session values remain available. Old input expires after the following request.

## Cookies {#cookies}

Use `request.cookie(Some("theme"))?` to retrieve a cookie or `request.cookie(None)?` to retrieve the cookie bag. `request.has_cookie("theme")?` checks presence, including an empty string.

With `EncryptCookies`, protected cookies are decrypted and validated before the handler runs. A failed validation leaves a null entry in the bag. A lookup by that name returns `None`, so callers can supply a default with `Option`.

## Input Trimming and Normalization {#input-trimming-and-normalization}

The global middleware trims string values in query, JSON and form inputs, then converts empty strings to `null`. The top-level fields `current_password`, `password` and `password_confirmation` are exempt from trimming. Empty strings in these fields still become `null`. The original URI, body bytes and uploaded files remain available.

Configure field patterns or skip either middleware for selected requests in `bootstrap/app.rs`:

```rust
.with_middleware(|middleware| {
    middleware
        .trim_strings(["rows.*.secret"])
        .skip_trim_strings_when(|request| request.is("hooks/*"))
        .convert_empty_strings_to_null(|request| request.is("hooks/*"));
})
```

Field patterns use dotted paths and `*` wildcards. Each application's exceptions apply to its own requests.

## Files {#files}

### Retrieving Uploaded Files {#retrieving-uploaded-files}

Use `request.file("attachments.0").await?` to retrieve one multipart upload. The returned `UploadedFile` provides bytes, the client filename and the client media type. Validate the content before storing it.

Use `request.file_value("photos.*.image").await?` for a group or wildcard selection and `request.all_files().await?` for the whole file bag. Both use `UploadedFileValue` to represent files and keyed groups. Fields submitted without a filename are omitted, while a named zero-byte file remains available. Surviving numeric keys keep their original positions.

`request.has_file("attachments").await?` checks a file or the immediate members of a group. To check deeper uploads, select the nested field or use a wildcard path.

See the [controller example](./controllers.md) for input handling and a JSON response.

## Configuring Trusted Proxies {#configuring-trusted-proxies}

Configure trusted proxy addresses during application assembly. Forwarding headers from an untrusted peer have no effect. The policy accepts exact IP addresses and CIDRs for IPv4 and IPv6. The header mask selects which fields the proxy can set.

```rust
use openrust::http::TrustedProxyHeaders;

.with_middleware(|middleware| {
    middleware
        .trust_proxies(&["192.0.2.10", "10.20.0.0/16"], TrustedProxyHeaders::DEFAULT)
        .expect("valid trusted proxy policy");
})
```

The default mask is `TrustedProxyHeaders::DEFAULT` for X-Forwarded fields. Use `FORWARDED` for RFC 7239 or `AWS_ELB` for the AWS load balancer fields. A conflict between two enabled header families returns HTTP 400.

`Request::secure` reflects the effective protocol. URL generation uses the forwarded origin and prefix while route matching uses the incoming path.

When application assembly does not supply a proxy policy, HTTP startup reads `trustedproxy.proxies` from the configuration repository. Use a comma-separated string or an array. The optional `trustedproxy.headers` accepts a numeric mask or the framework preset name such as `HEADER_X_FORWARDED_AWS_ELB`. An explicit `trust_proxies` policy takes precedence, including an empty address list.

## Configuring Trusted Hosts {#configuring-trusted-hosts}

Enable `trust_hosts` to restrict the original and effective hosts. Patterns are case-insensitive PCRE expressions matched against the hostname without its port. A rejected host returns HTTP 400 before routing.

```rust
.with_middleware(|middleware| {
    middleware
        .trust_hosts(&[r"^edge\.example\.com$"], true)
        .expect("valid host patterns");
})
```

With `subdomains` enabled, the hostname in `app.url` and its subdomains are also trusted. Use `trust_hosts_with` to resolve patterns asynchronously for each request. Host checks are skipped in the exact `local` and `testing` environments.
## Request Metadata

`ajax()` checks `X-Requested-With: XMLHttpRequest`; `pjax()` checks `X-PJAX`. `prefetch()` compares `X-Moz`, `Purpose`, and `Sec-Purpose` with `prefetch`, ignoring case but requiring an exact value. Prefetch requests do not replace the session's previous URL or route. `user_agent()` returns an optional header value. `fingerprint()` requires a matched route and hashes its methods, domain, URI and client IP. Use `user(guard).await` to resolve the current user through the selected guard.
