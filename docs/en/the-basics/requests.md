# HTTP Requests

## Request Headers {#request-headers}

Use `request.header("X-Trace")?` to read the first text value or `None` if the header is absent. Invalid names or non-UTF-8 values return HTTP 400 when the error is rendered by the kernel. Use `hasHeader` for presence, `headers()` for the complete header map and `bearerToken()?` to extract a bearer token from Authorization.

## Request Path, Host and URL {#request-path-host-and-method}

`path()` strips surrounding slashes. `decodedPath()` also percent-decodes the path. Use `segments()` or the one-based `segment(index)` to read its components. `is(pattern)` checks the decoded path, `routeIs(&["reports.*"])` checks the matched route name, and `isMethod("GET")` checks the effective HTTP method.

`host()?`, `httpHost()?` and `schemeAndHttpHost()?` use the effective connection and trusted proxy settings. The HTTP host includes a non-default port. `root()?` also includes the trusted proxy base path. `url()?` returns the current URL without a query string. `fullUrl()?` and `fullUrlIs(patterns)?` use a normalized query string. Use `fullUrlWithQuery(&serde_json::json!({"page": 2}))?` or `fullUrlWithoutQuery(&["page"])?` to build a URL from the request query data. These helpers return a new URL string. Signature verification uses the original request target.

## Retrieving Input

### Retrieving All Input Data {#retrieving-all-input-data}

Use `request.all().await?` to retrieve ordinary input and uploaded files together. `RequestValue` holds JSON scalars, ordered groups with integer or string keys, and typed `UploadedFile` objects.

When a regular input value and an upload share a key, the input value wins, including explicit null. Nested groups retain fields supplied on either side. The upload is still available through the file bag.

Use `request.allOnly(&["items.1.photo", "name"]).await?` to select paths and retain missing fields as null. Use `request.keys().await?` to list input keys followed by file keys. A name supplied by both sources appears twice.

### Retrieving an Input Value {#retrieving-an-input-value}

Use `request.query(Some("key"))` to read an exact top-level query key. Use `request.input(Some("team.name")).await` for nested input. Pass `None` to retrieve the input bag. JSON and form fields take precedence over query values at the top level. Use `request.json` to read the JSON body alone.

Use `request.post(None).await?` for the body parameter bag or `request.post(Some("name")).await?` for an exact top-level key. On a JSON request this reads the decoded JSON body. Query parameters and multipart file objects use their own bags. An explicit null remains in the full body bag, and a named lookup of that value returns `None`.

Use `request.fluent(Some("user")).await?` to wrap a group of ordinary input as an independent `Fluent` bag. Pass `None` for all ordinary input, or use `fluentOr("user", default)` for a default attribute bag if the field is missing or null. A scalar cannot initialize the bag and returns HTTP 400. Use `fluentOnly(&["user.name", "document"]).await?` for an independent typed snapshot that retains uploads. Use `get` for paths and `value` for exact top-level keys.

Use `scope("user")` for an independent nested bag. Scalar and file scopes use index `0`. Use `set("user.name", RequestValue::Value(serde_json::json!("Ada")))?` to edit a typed bag. Wildcard paths update existing children, and the original request keeps its input.

OpenRust preserves the body bytes for subsequent reads after successful collection. The default limit is 8 MiB. Invalid input returns 400, and oversized input returns 413.

### Request Boundaries {#request-boundaries}

The kernel rejects a path with invalid UTF-8 after percent decoding with HTTP 400. A declared `Content-Length` over 8 MiB returns HTTP 413 before the framework reads the body. When the framework collects a body for input parsing or `body_bytes()`, the actual bytes are also capped at 8 MiB, including chunked requests.

### Typed Input Values

Use `request.integer("value", 42, true).await?`, `request.float("value", 1.25, true).await?`, or `request.boolean("value", true, true).await?`. The arguments specify the input key, fallback, and strict mode. All three methods use the same dotted keys and input precedence as `request.input`.

A missing value uses the fallback. With strict mode enabled, invalid values also use the fallback. For example, `"01"` is invalid as a strict integer but is valid as a float. With strict mode disabled, numeric reads use PHP casts and can read a leading numeric portion. Boolean reads recognize `1`, `true`, `on`, and `yes` case-insensitively. The recognized false values are `0`, `false`, `off`, and `no`. Unrecognized boolean input returns false in permissive mode. Malformed request bodies still return an input error.

Use `request.string("label", "fallback", true).await?.trim()` for a fluent string. `request.str` is an alias. Strict strings accept text and numbers. `request.array(Some("items")).await?` preserves lists and keyed objects, wraps scalars into a one-item list, and returns an empty list for null or missing values. `request.collect` returns a collection with the same keys. Pass `None` to read the input bag.

Implement `BackedEnum` with a `String` or `i64` backing. Use `request.r#enum::<Stage>("stage", None).await?` for one case, or `request.enums::<Stage>("list").await?` for a collection. The list keeps original keys and duplicate cases while skipping unknown cases. A single unknown case uses the supplied fallback. An invalid backing type returns HTTP 400.

Use `request.date("when", None, None).await?` to parse a date using the application's time zone. Pass a PHP format and an explicit time zone with `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?`. Missing, null, and blank values return `None`. Invalid dates return HTTP 400. The parsed date preserves its resolved time zone and microseconds.

In a test fixture, `TestApp::travelTo` and the freeze helpers set the reference for relative dates and components omitted by the format. For example, `tomorrow` is resolved from the fixture's frozen date in the requested time zone. A format starting with `!` resets omitted fields. See [Interacting with time](../testing/mocking.md#interacting-with-time).

Use `request.interval("duration", None).await?` for ISO 8601 intervals or expressions such as `2 hours 30 minutes`. Use `request.interval("seconds", Some("second")).await?` when a field supplies a value in a named unit. The result preserves calendar components and microseconds. An explicit value of 90 seconds stays in the seconds field. Blank or missing input returns `None`. Invalid interval syntax or units return HTTP 400.

### Retrieving Input via Dynamic Properties {#retrieving-input-via-dynamic-properties}

Use `request.field("name").await?` for the lookup Laravel performs on dynamic request properties. It reads merged input and files first, including explicit nulls. If the field is absent, it looks for a bound model or raw parameter with that name on the matched route. Match `RequestField::RouteModel(model)` and use `model.downcast::<T>()` for a bound model. Ordinary fields, uploads and raw route parameters use `RequestField::Value`.

### Retrieving a Portion of the Input Data {#retrieving-a-portion-of-the-input-data}

Use `request.only(&["user.name", "document"]).await?` to select fields from input and uploads. It omits missing paths and preserves explicit null and typed files. `request.except(&["document"]).await?` removes fields without renumbering numeric keys. Use `arrayOnly` or `collectOnly` to return the selected data as a typed array or collection.

## Content Negotiation {#content-negotiation}

Use `request.getAcceptableContentTypes()` to read the `Accept` entries in quality order. Use `request.accepts(&["text/html", "application/json"])` to check a set of acceptable types, or `request.prefers(&["html", "json"])` to select the best candidate. `prefers` returns `None` when the header is absent or there is no match. `request.format("html")` uses the first recognized response format and falls back to the supplied value.

`acceptsAnyContentType()`, `acceptsJson()`, `acceptsHtml()` and `acceptsMarkdown()` check acceptance. `wantsJson()` and `wantsMarkdown()` inspect the top-ranked entry. `expectsJson()` also recognizes a non-PJAX XMLHttpRequest with an empty or wildcard `Accept` header. `isJson()` checks the request body's `Content-Type`.

With repeated `Accept` header fields, the first field is used, as in the pinned Laravel request implementation. Send several types as a comma-separated list in one field.

### Native HTTP Requests {#native-http-requests}

The HTTP server passes each owned transport request to `Kernel::handleTransport`, which constructs the framework request with `Request::from(native)`. The kernel applies trusted proxies and method overrides within that request’s lifecycle.

For interoperability with Rust HTTP libraries, convert an owned `http::Request<Body>` using `Request::from(native)` and move it back with `into()`. Ownership conversion moves the current body stream along with its headers, protocol version and extensions. Inside a route, use `request.toHttpRequest().await?` for an independent native request with a buffered body. It uses the same 8 MiB limit as framework input, and the body remains available to the framework request.

The native request uses the transport URI from `originalUri()` and the effective method from `method()`, including any method override. Use `uri()` for the framework URI after trusted-proxy processing and `getRealMethod()` for the original method. `version()` and `extensions()` read the corresponding transport metadata.

See [Native HTTP responses](./responses.md#native-http-responses) for returning a native response.

### Input Presence {#input-presence}

Use `request.has(&["profile.name"]).await?` when a key must exist, including a key explicitly set to `null`. Use `hasAny` to check alternatives and `missing` to test whether any supplied key is absent. These checks also recognize uploaded files.

Use `filled`, `isNotFilled` and `anyFilled` to check input values. Missing values, `null` and blank strings are empty. Booleans, zero and empty arrays count as filled. A file-only field is checked with `hasFile`.

Use `whenHas`, `whenFilled` or `whenMissing` to run an async callback when the condition holds. `whenHas` and `whenFilled` pass the combined field as `Option<RequestValue>`. Match `RequestValue::File(file)` to work with an upload. The method returns `Some(result)` when the callback runs. Use `unwrap_or_else` to supply a fallback.

### Merging Additional Input {#merging-additional-input}

Use `request.merge(serde_json::json!({"profile.name": "Ada"})).await?` to set values by dotted path. `mergeIfMissing` preserves keys already present in the request, including explicit `null`, query fallbacks and uploads. Use `replace` with an object or array to replace the active input source.

A JSON request updates its JSON input. A non-JSON GET or HEAD updates the query bag. Other requests update their form input and still fall back to query values. These methods preserve the original URI, raw body and uploads.

## Old Input {#old-input}

In a route with session middleware, use `request.flash().await?` to store the current input for the next request. Use `request.flashOnly(&["profile.name", "page"]).await?` to select specific fields or `request.flashExcept(&["password", "password_confirmation"]).await?` to exclude fields. Selection supports dotted paths. Uploaded files remain in the file bag.

Read the result with `request.old(Some("profile.name"))` or retrieve the bag with `request.old(None)`. An exact key takes precedence over a dotted path. Explicit null remains available as `Some(Value::Null)`. Use `Option` methods to supply a fallback when the value is missing.

Use `request.flush()?` to replace the old input with an empty bag. The other session values remain available. Old input expires after the following request.

## Cookies {#cookies}

Use `request.cookie(Some("theme"))?` to retrieve a cookie or `request.cookie(None)?` to retrieve the cookie bag. `request.hasCookie("theme")?` checks presence, including an empty string.

With `EncryptCookies`, protected cookies are decrypted and validated before the handler runs. A failed validation leaves a null entry in the bag. A lookup by that name returns `None`, so callers can supply a default with `Option`.

## Input Trimming and Normalization {#input-trimming-and-normalization}

The global middleware trims string values in query, JSON and form inputs, then converts empty strings to `null`. The top-level fields `current_password`, `password` and `password_confirmation` are exempt from trimming. Empty strings in these fields still become `null`. The original URI, body bytes and uploaded files remain available.

Configure field patterns or skip either middleware for selected requests in `bootstrap/app.rs`:

```rust
.withMiddleware(|middleware| {
    middleware
        .trimStrings(["rows.*.secret"])
        .skipTrimStringsWhen(|request| request.is("hooks/*"))
        .convertEmptyStringsToNull(|request| request.is("hooks/*"));
})
```

Field patterns use dotted paths and `*` wildcards. Each application's exceptions apply to its own requests.

## Files {#files}

### Retrieving Uploaded Files {#retrieving-uploaded-files}

Use `request.file("attachments.0").await?` to retrieve one multipart upload. The returned `UploadedFile` provides bytes, the client filename and the client media type. Validate the content before storing it.

Use `request.fileValue("photos.*.image").await?` for a group or wildcard selection and `request.allFiles().await?` for the whole file bag. Both use `UploadedFileValue` to represent files and keyed groups. Fields submitted without a filename are omitted, while a named zero-byte file remains available. Surviving numeric keys keep their original positions.

`request.hasFile("attachments").await?` checks a file or the immediate members of a group. To check deeper uploads, select the nested field or use a wildcard path.

See the [controller example](./controllers.md) for input handling and a JSON response.

## Configuring Trusted Proxies {#configuring-trusted-proxies}

Configure trusted proxy addresses during application assembly. Forwarding headers from an untrusted peer have no effect. The policy accepts exact IP addresses and CIDRs for IPv4 and IPv6. The header mask selects which fields the proxy can set.

```rust
use openrust::http::TrustedProxyHeaders;

.withMiddleware(|middleware| {
    middleware
        .trustProxies(&["192.0.2.10", "10.20.0.0/16"], TrustedProxyHeaders::DEFAULT)
        .expect("valid trusted proxy policy");
})
```

The default mask is `TrustedProxyHeaders::DEFAULT` for X-Forwarded fields. Use `FORWARDED` for RFC 7239 or `AWS_ELB` for the AWS load balancer fields. A conflict between two enabled header families returns HTTP 400.

`Request::ip` returns the nearest untrusted client address from the chain. `Request::ips` returns the resolved addresses, and `Request::secure` reflects the effective protocol. URL generation uses the forwarded origin and prefix while route matching uses the incoming path.

When application assembly does not supply a proxy policy, HTTP startup reads `trustedproxy.proxies` from the configuration repository. Use a comma-separated string or an array. The optional `trustedproxy.headers` accepts a numeric mask or a Laravel preset name such as `HEADER_X_FORWARDED_AWS_ELB`. An explicit `trustProxies` policy takes precedence, including an empty address list.

## Configuring Trusted Hosts {#configuring-trusted-hosts}

Enable `trustHosts` to restrict the original and effective hosts. Patterns are case-insensitive PCRE expressions matched against the hostname without its port. A rejected host returns HTTP 400 before routing.

```rust
.withMiddleware(|middleware| {
    middleware
        .trustHosts(&[r"^edge\.example\.com$"], true)
        .expect("valid host patterns");
})
```

With `subdomains` enabled, the hostname in `app.url` and its subdomains are also trusted. Use `trustHostsWith` to resolve patterns asynchronously for each request. Host checks are skipped in the exact `local` and `testing` environments.
