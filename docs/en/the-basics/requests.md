# HTTP Requests

## Retrieving Input

Use `request.query(Some("key"))` to read an exact top-level query key. Use `request.input(Some("team.name")).await` for nested input. Pass `None` to retrieve the input bag. JSON and form fields take precedence over query values at the top level. Use `request.json` to read the JSON body alone.

OpenRust preserves the body bytes for subsequent reads after successful collection. The default limit is 8 MiB. Invalid input returns 400, and oversized input returns 413.

### Typed Input Values

Use `request.integer("value", 42, true).await?`, `request.float("value", 1.25, true).await?`, or `request.boolean("value", true, true).await?`. The arguments specify the input key, fallback, and strict mode. All three methods use the same dotted keys and input precedence as `request.input`.

A missing value uses the fallback. With strict mode enabled, invalid values also use the fallback. For example, `"01"` is invalid as a strict integer but is valid as a float. With strict mode disabled, numeric reads use PHP casts and can read a leading numeric portion. Boolean reads recognize `1`, `true`, `on`, and `yes` case-insensitively. The recognized false values are `0`, `false`, `off`, and `no`. Unrecognized boolean input returns false in permissive mode. Malformed request bodies still return an input error.

Use `request.string("label", "fallback", true).await?.trim()` for a fluent string. `request.str` is an alias. Strict strings accept text and numbers. `request.array(Some("items")).await?` preserves lists and keyed objects, wraps scalars into a one-item list, and returns an empty list for null or missing values. `request.collect` returns a collection with the same keys. Pass `None` to read the input bag.

Implement `BackedEnum` with a `String` or `i64` backing. Use `request.r#enum::<Stage>("stage", None).await?` for one case, or `request.enums::<Stage>("list").await?` for a collection. The list keeps original keys and duplicate cases while skipping unknown cases. A single unknown case uses the supplied fallback. An invalid backing type returns HTTP 400.

Use `request.date("when", None, None).await?` to parse a date using the application's time zone. Pass a PHP format and an explicit time zone with `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?`. Missing, null, and blank values return `None`. Invalid dates return HTTP 400. The parsed date preserves its resolved time zone and microseconds.

In a test fixture, `TestApp::travelTo` and the freeze helpers set the reference for relative dates and components omitted by the format. For example, `tomorrow` is resolved from the fixture's frozen date in the requested time zone. A format starting with `!` resets omitted fields. See [Interacting with time](../testing/mocking.md#interacting-with-time).

Use `request.only(&["user.name", "user.note"]).await?` to select nested input while retaining explicit null values. `request.except` removes paths from a copy of the input. Use `arrayOnly` and `collectOnly` for the multi-key forms of `array` and `collect`. These methods select input values. Read multipart uploads through `request.file`.

Use `request.interval("duration", None).await?` for ISO 8601 intervals or expressions such as `2 hours 30 minutes`. Use `request.interval("seconds", Some("second")).await?` when a field supplies a value in a named unit. The result preserves calendar components and microseconds. An explicit value of 90 seconds stays in the seconds field. Blank or missing input returns `None`. Invalid interval syntax or units return HTTP 400.

## Content Negotiation {#content-negotiation}

Use `request.getAcceptableContentTypes()` to read the `Accept` entries in quality order. Use `request.accepts(&["text/html", "application/json"])` to check a set of acceptable types, or `request.prefers(&["html", "json"])` to select the best candidate. `prefers` returns `None` when the header is absent or there is no match. `request.format("html")` uses the first recognized response format and falls back to the supplied value.

`acceptsAnyContentType()`, `acceptsJson()`, `acceptsHtml()` and `acceptsMarkdown()` check acceptance. `wantsJson()` and `wantsMarkdown()` inspect the top-ranked entry. `expectsJson()` also recognizes a non-PJAX XMLHttpRequest with an empty or wildcard `Accept` header. `isJson()` checks the request body's `Content-Type`.

With repeated `Accept` header fields, the first field is used, as in the pinned Laravel request implementation. Send several types as a comma-separated list in one field.

## Files

Use `request.file("attachments.0").await` for a multipart upload, or `request.hasFile("attachments").await` to check whether a field contains an upload. The upload provides bytes and client-supplied filename and media type. Check the content before storing it.

See the [controller example](./controllers.md) for input handling and a JSON response.

## Cookies

Use `request.cookie(Some("theme"))?` to retrieve a cookie or `request.cookie(None)?` to retrieve the cookie bag. In a stack containing `EncryptCookies`, protected cookies are decrypted and validated before the handler runs. A cookie with invalid ciphertext or a mismatched name binding has a null value.

## Old Input

Call `request.old(Some("profile.name"))` to retrieve a nested flashed field or `request.old(None)` to retrieve the input bag. The value is available on the next request. An explicit null is preserved.

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
