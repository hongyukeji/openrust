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
    test.get("/up").await?.assert_ok();
    Ok(())
}
```

## Inspecting responses

The returned `TestResponse` exposes status, headers, and body. Chain `assert_status`, `assert_header`, `assert_content`, or `assert_exact_json` to check them. Body capture has a 1 MiB limit and a five-second timeout. Use a running listener and an HTTP client to test transport behavior such as connection metadata.
## Response Assertions

Use `assert_json_path` for dot paths, `assert_json_structure` for nested/wildcard structure, and `assert_json_validation_errors` for keys in the JSON `errors` object. `assert_redirect`, `assert_session_has`, and `assert_download` inspect actual response and session state. `assert_cookie` decrypts and validates framework encrypted string cookies when an expected value is supplied; use `assert_plain_cookie` for unencrypted cookies. PHP cookie unserialization and the full set of optional PHP assertion arguments are not provided. Each failed assertion panics with the mismatched value or missing key.

## Status and body assertions

Use `assert_successful`, `assert_client_error`, and `assert_server_error` for status ranges, or a named assertion such as `assert_created`, `assert_not_found`, and `assert_unprocessable` for an exact status. `assert_no_content(None)` checks status 204 and an empty body; pass `Some(StatusCode::OK)` to select another status. `assert_successful_precognition` also requires the success header. `assert_header("x-probe", None)` checks presence; `assert_header_missing` checks absence.

`assert_see` and `assert_dont_see` accept a string or string list and an explicit HTML escaping flag. `assert_see_html` and `assert_dont_see_html` search raw HTML. `assert_see_in_order` and `assert_see_html_in_order` require the values in order. The `assert_see_text`, `assert_dont_see_text`, and `assert_see_text_in_order` variants remove tags, decode HTML401 entities, and normalize whitespace before matching. Ordered HTML matching decodes entities. Text and ordered assertions require valid UTF-8.

```rust
response.assert_successful().assert_header_missing("x-error");
response.assert_see(["Ada", "Bob"], true);
response.assert_see_html("<strong>Ada</strong>");
response.assert_see_text_in_order(["Ada", "Bob"], false);
```

## JSON assertions

`assert_json(&expected, false)` checks a recursive subset with loose scalar comparison; pass `true` for strict types. Array subset positions remain significant. `assert_exact_json` compares the complete document while ignoring associative key order; `assert_similar_json` also sorts lists recursively. `assert_json_count(count, key)` counts the whole document with `None` or a dot path with `Some("data")`.

`assert_json_fragment` searches each key/value fragment anywhere in the document; `assert_json_fragments` checks several fragments. `assert_json_missing(&fragment, false)` requires every supplied pair to be absent. `assert_json_missing_exact` succeeds when at least one pair is absent.

`assert_json_path` and `assert_json_paths` compare exact types at dot paths; a missing path resolves to null. Use `assert_json_path_canonicalizing` or `assert_json_paths_canonicalizing` to ignore list order. Batch path expectations use `serde_json::Map<String, Value>`. `assert_json_missing_path` and `assert_json_missing_paths` reject existing paths, including wildcards. `assert_json_structure` checks required fields; `assert_exact_json_structure` also rejects extra fields at every declared level. Use `{"data":{"*":["id"]}}` for an item structure. `assert_json_is_array(key)` and `assert_json_is_object(key)` distinguish list and associative shapes after decoding; an empty object is treated as an empty list.

Assertions take JSON values and explicit typed arguments. Dynamic fluent assertion callbacks and model objects are not accepted. JSON depth and path checks use the framework's existing support limits.

## Session and validation assertions

`assert_session_has(key, expected)` checks a non-null value when `expected` is `None`. An explicit value uses loose scalar comparison. `assert_session_has_all(&bindings)` accepts a key list or an object of expected values; `assert_session_missing(key, unexpected)` checks absence or a different value. `assert_session_has_input` and `assert_session_missing_input` check flashed input.

`assert_session_has_errors(&keys, format, error_bag)` accepts a field list or an object of exact messages. Use `None` for the default message format and `"default"` for the default bag. `assert_session_has_errors_in(error_bag, &keys, format)` selects the bag first. `assert_session_doesnt_have_errors` rejects selected errors; with an empty selection it requires no errors entry at all. `assert_session_has_no_errors` also checks that entry is absent.

`assert_valid(keys, error_bag, response_key)` accepts `None` to reject all errors, or `Some(&["email"])` to reject errors for selected fields. `assert_invalid(errors, error_bag, response_key)` checks supplied fields and message substrings. `assert_only_invalid` rejects unexpected error fields. These methods inspect JSON when the content type is exactly `application/json`, otherwise they inspect the session. Common arguments are `"default"` and `"errors"`.

`assert_json_validation_errors(&["email"])` checks standard JSON field errors. Use `assert_json_validation_errors_at(&errors, response_key)` for message expectations or another errors path, `assert_json_validation_error_for` for one field, `assert_only_json_validation_errors` for an exclusive field set, and `assert_json_missing_validation_errors` for absent errors. Assertions inspect the captured session snapshot; they do not consume flashed values. Dynamic PHP closures and serialized PHP objects are not accepted.

## Configuring body capture

Each `TestApp` starts with a 1 MiB body limit and a five-second capture timeout. Override these for large or slow responses with `with_response_capture`; settings apply to subsequent requests and stay local to that test application. A limit or timeout error drops the body stream and the application can serve later requests.

```rust
use std::time::Duration;
use openrust::testing::{ResponseCaptureOptions, TestApp};

let mut test = TestApp::create(|builder| builder)?;
test.with_response_capture(ResponseCaptureOptions {
    max_bytes: 8 * 1024 * 1024,
    timeout: Duration::from_secs(30),
});
```

These are capture limits for the in-process test tool. Use a running listener and an HTTP client for transport behavior and unbounded streaming.

## Redirect, stream, cookie and view assertions

`assert_location(uri)` normalizes both addresses through the application URL generator. `assert_redirect_contains(uri)` checks a nonempty, case-sensitive substring; `assert_redirect_back` checks the previous address. Use `assert_redirect_back_with_errors(&keys, format, error_bag)` or `assert_redirect_back_without_errors` for session errors. `assert_redirect_to_route` and `assert_redirect_to_action` accept explicit route parameters. `assert_redirect_to_signed_route(name, parameters, absolute)` checks the actual signature and expiration; pass `None` to omit the named-route comparison.

`assert_header(name, value)` ignores ASCII letter case in the value. `assert_header_contains(name, value)` checks a nonempty, case-sensitive substring.

`assert_streamed` and `assert_not_streamed` check the response type. `streamed_content` and `assert_streamed_content` also accept file responses. `assert_streamed_json_content` compares encoded bytes, including Unicode and slash escaping; it does not compare decoded JSON. Replacing response content clears the original stream or view metadata.

`assert_cookie_expired`, `assert_cookie_not_expired` and `assert_cookie_missing` inspect response cookies. Expiration uses the current test clock; Max-Age starts at response creation. A session cookie has no expiration.

`assert_view_is`, `assert_view_has`, `assert_view_has_all`, `assert_view_missing` and `view_data` inspect the original view data after creators and composers, including shared data. They do not parse rendered HTML. Dynamic callbacks and PHP objects are not accepted.
