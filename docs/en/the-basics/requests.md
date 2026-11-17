# HTTP Requests

## Retrieving Input

Use `request.query(Some("key"))` to read an exact top-level query key. Use `request.input(Some("team.name")).await` for nested input. Pass `None` to retrieve the input bag. JSON and form fields take precedence over query values at the top level. Use `request.json` to read the JSON body alone.

OpenRust preserves the body bytes for subsequent reads after successful collection. The default limit is 8 MiB. Invalid input returns 400, and oversized input returns 413.

### Typed Input Values

Use `request.integer("value", 42, true).await?`, `request.float("value", 1.25, true).await?`, or `request.boolean("value", true, true).await?`. The arguments specify the input key, fallback, and strict mode. All three methods use the same dotted keys and input precedence as `request.input`.

A missing value uses the fallback. With strict mode enabled, invalid values also use the fallback. For example, `"01"` is invalid as a strict integer but is valid as a float. With strict mode disabled, numeric reads use PHP casts and can read a leading numeric portion. Boolean reads recognize `1`, `true`, `on`, and `yes` case-insensitively. The recognized false values are `0`, `false`, `off`, and `no`. Unrecognized boolean input returns false in permissive mode. Malformed request bodies still return an input error.

## Files

Use `request.file("attachments.0").await` for a multipart upload, or `request.hasFile("attachments").await` to check whether a field contains an upload. The upload provides bytes and client-supplied filename and media type. Check the content before storing it.

See the [controller example](./controllers.md) for input handling and a JSON response.

## Cookies

Use `request.cookie(Some("theme"))?` to retrieve a cookie or `request.cookie(None)?` to retrieve the cookie bag. In a stack containing `EncryptCookies`, protected cookies are decrypted and validated before the handler runs. A cookie with invalid ciphertext or a mismatched name binding has a null value.

## Old Input

Call `request.old(Some("profile.name"))` to retrieve a nested flashed field or `request.old(None)` to retrieve the input bag. The value is available on the next request. An explicit null is preserved.
