# HTTP Tests

## Dispatching requests

Use [TestApp](getting-started.md) to configure the application and dispatch requests through the HTTP kernel in the test process. Use `get` for an empty GET or `call` with an HTTP request containing the method, headers, and body.

```rust
use openrust::{foundation::configuration::Routing, testing::TestApp};

#[tokio::test]
async fn health_route_responds() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| {
        builder.with_routing(Routing::default().health("/up"))
    })?;
    test.get("/up").await?.assertOk();
    Ok(())
}
```

## Inspecting responses

The returned `TestResponse` exposes status, headers, and body. Chain `assertStatus`, `assertHeader`, `assertContent`, or `assertExactJson` to check them. Body capture has a 1 MiB limit and a five-second timeout. Use a running listener and an HTTP client to test transport behavior such as connection metadata.
## Response Assertions

Use `assertJsonPath` for dot paths, `assertJsonStructure` for nested/wildcard structure, and `assertJsonValidationErrors` for keys in the JSON `errors` object. `assertRedirect`, `assertSessionHas`, and `assertDownload` inspect actual response and session state. `assertCookie` decrypts and validates framework encrypted string cookies when an expected value is supplied; use `assertPlainCookie` for unencrypted cookies. PHP cookie unserialization and the full set of optional PHP assertion arguments are not provided. Each failed assertion panics with the mismatched value or missing key.

## Status and body assertions

Use `assertSuccessful`, `assertClientError`, and `assertServerError` for status ranges, or a named assertion such as `assertCreated`, `assertNotFound`, and `assertUnprocessable` for an exact status. `assertNoContent(None)` checks status 204 and an empty body; pass `Some(StatusCode::OK)` to select another status. `assertSuccessfulPrecognition` also requires the success header. `assertHeader("x-probe", None)` checks presence; `assertHeaderMissing` checks absence.

`assertSee` and `assertDontSee` accept a string or string list and an explicit HTML escaping flag. `assertSeeHtml` and `assertDontSeeHtml` search raw HTML. `assertSeeInOrder` and `assertSeeHtmlInOrder` require the values in order. The `assertSeeText`, `assertDontSeeText`, and `assertSeeTextInOrder` variants remove tags, decode HTML401 entities, and normalize whitespace before matching. Ordered HTML matching decodes entities. Text and ordered assertions require valid UTF-8.

```rust
response.assertSuccessful().assertHeaderMissing("x-error");
response.assertSee(["Ada", "Bob"], true);
response.assertSeeHtml("<strong>Ada</strong>");
response.assertSeeTextInOrder(["Ada", "Bob"], false);
```

## JSON assertions

`assertJson(&expected, false)` checks a recursive subset with loose scalar comparison; pass `true` for strict types. Array subset positions remain significant. `assertExactJson` compares the complete document while ignoring associative key order; `assertSimilarJson` also sorts lists recursively. `assertJsonCount(count, key)` counts the whole document with `None` or a dot path with `Some("data")`.

`assertJsonFragment` searches each key/value fragment anywhere in the document; `assertJsonFragments` checks several fragments. `assertJsonMissing(&fragment, false)` requires every supplied pair to be absent. `assertJsonMissingExact` succeeds when at least one pair is absent.

`assertJsonPath` and `assertJsonPaths` compare exact types at dot paths; a missing path resolves to null. Use `assertJsonPathCanonicalizing` or `assertJsonPathsCanonicalizing` to ignore list order. Batch path expectations use `serde_json::Map<String, Value>`. `assertJsonMissingPath` and `assertJsonMissingPaths` reject existing paths, including wildcards. `assertJsonStructure` checks required fields; `assertExactJsonStructure` also rejects extra fields at every declared level. Use `{"data":{"*":["id"]}}` for an item structure. `assertJsonIsArray(key)` and `assertJsonIsObject(key)` distinguish list and associative shapes after decoding; an empty object is treated as an empty list.

Assertions take JSON values and explicit typed arguments. Dynamic fluent assertion callbacks and model objects are not accepted. JSON depth and path checks use the framework's existing support limits.

## Session and validation assertions

`assertSessionHas(key, expected)` checks a non-null value when `expected` is `None`. An explicit value uses loose scalar comparison. `assertSessionHasAll(&bindings)` accepts a key list or an object of expected values; `assertSessionMissing(key, unexpected)` checks absence or a different value. `assertSessionHasInput` and `assertSessionMissingInput` check flashed input.

`assertSessionHasErrors(&keys, format, errorBag)` accepts a field list or an object of exact messages. Use `None` for the default message format and `"default"` for the default bag. `assertSessionHasErrorsIn(errorBag, &keys, format)` selects the bag first. `assertSessionDoesntHaveErrors` rejects selected errors; with an empty selection it requires no errors entry at all. `assertSessionHasNoErrors` also checks that entry is absent.

`assertValid(keys, errorBag, responseKey)` accepts `None` to reject all errors, or `Some(&["email"])` to reject errors for selected fields. `assertInvalid(errors, errorBag, responseKey)` checks supplied fields and message substrings. `assertOnlyInvalid` rejects unexpected error fields. These methods inspect JSON when the content type is exactly `application/json`, otherwise they inspect the session. Common arguments are `"default"` and `"errors"`.

`assertJsonValidationErrors(&["email"])` checks standard JSON field errors. Use `assertJsonValidationErrorsAt(&errors, responseKey)` for message expectations or another errors path, `assertJsonValidationErrorFor` for one field, `assertOnlyJsonValidationErrors` for an exclusive field set, and `assertJsonMissingValidationErrors` for absent errors. Assertions inspect the captured session snapshot; they do not consume flashed values. Dynamic PHP closures and serialized PHP objects are not accepted.

## Configuring body capture

Each `TestApp` starts with a 1 MiB body limit and a five-second capture timeout. Override these for large or slow responses with `withResponseCapture`; settings apply to subsequent requests and stay local to that test application. A limit or timeout error drops the body stream and the application can serve later requests.

```rust
use std::time::Duration;
use openrust::testing::{ResponseCaptureOptions, TestApp};

let mut test = TestApp::create(|builder| builder)?;
test.withResponseCapture(ResponseCaptureOptions {
    maxBytes: 8 * 1024 * 1024,
    timeout: Duration::from_secs(30),
});
```

These are capture limits for the in-process test tool. Use a running listener and an HTTP client for transport behavior and unbounded streaming.

## Redirect, stream, cookie and view assertions

`assertLocation(uri)` normalizes both addresses through the application URL generator. `assertRedirectContains(uri)` checks a nonempty, case-sensitive substring; `assertRedirectBack` checks the previous address. Use `assertRedirectBackWithErrors(&keys, format, errorBag)` or `assertRedirectBackWithoutErrors` for session errors. `assertRedirectToRoute` and `assertRedirectToAction` accept explicit route parameters. `assertRedirectToSignedRoute(name, parameters, absolute)` checks the actual signature and expiration; pass `None` to omit the named-route comparison.

`assertHeader(name, value)` ignores ASCII letter case in the value. `assertHeaderContains(name, value)` checks a nonempty, case-sensitive substring.

`assertStreamed` and `assertNotStreamed` check the response type. `streamedContent` and `assertStreamedContent` also accept file responses. `assertStreamedJsonContent` compares encoded bytes, including Unicode and slash escaping; it does not compare decoded JSON. Replacing response content clears the original stream or view metadata.

`assertCookieExpired`, `assertCookieNotExpired` and `assertCookieMissing` inspect response cookies. Expiration uses the current test clock; Max-Age starts at response creation. A session cookie has no expiration.

`assertViewIs`, `assertViewHas`, `assertViewHasAll`, `assertViewMissing` and `viewData` inspect the original view data after creators and composers, including shared data. They do not parse rendered HTML. Dynamic callbacks and PHP objects are not accepted.
