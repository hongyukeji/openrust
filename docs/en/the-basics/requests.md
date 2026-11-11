# HTTP Requests

## Retrieving Input

Use `request.query(Some("key"))` to read an exact top-level query key. Use `request.input(Some("team.name")).await` for nested input. Pass `None` to retrieve the input bag. JSON and form fields take precedence over query values at the top level. Use `request.json` to read the JSON body alone.

OpenRust preserves the body bytes for subsequent reads after successful collection. The default limit is 8 MiB. Invalid input returns 400, and oversized input returns 413.

## Files

Use `request.file("attachments.0").await` for a multipart upload, or `request.hasFile("attachments").await` to check whether a field contains an upload. The upload provides bytes and client-supplied filename and media type. Check the content before storing it.

See the [controller example](./controllers.md) for input handling and a JSON response.
