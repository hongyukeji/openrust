# New Relic APM (Extension)

## Enable telemetry {#configuration}

Add `"new-relic"` to `[features].default` in the application's `Cargo.toml`. Create the array if absent and keep any existing features. Rebuild the application. Package discovery registers `NewRelicServiceProvider`. An independent consumer can register that provider explicitly.

Set these environment variables on the HTTP and Worker processes that should report telemetry.

```dotenv
NEW_RELIC_ENABLED=true
NEW_RELIC_APP_NAME=Orders
NEW_RELIC_REGION=us
```

Provide the New Relic ingest license as `NEW_RELIC_LICENSE_KEY` through the runtime environment. The provider reads it at startup, outside the configuration cache. The service name defaults to the application name. Choose `us`, `eu`, `jp`, or `us-gov` for the account's ingest region. Rebuild the application configuration cache after changing the enable flag, name, or region.

For an OpenTelemetry Collector, set `NEW_RELIC_COLLECTOR_ENDPOINT` to its base URL. Use HTTPS or a loopback IP with HTTP, with no credentials, path, query, or fragment. The application sends OTLP HTTP/protobuf to `/v1/traces` and `/v1/metrics`. Configure the Collector's New Relic exporter with the license.

When a trusted downstream service should receive W3C trace context, set `NEW_RELIC_PROPAGATE_ORIGINS` to a comma-separated list of exact origins such as `https://api.example.com,https://internal.example.com:8443`. Each origin includes scheme, host, and effective port. Leave it empty to disable automatic injection. Rebuild the configuration cache after changing this list.

## What is recorded {#telemetry}

HTTP telemetry includes server spans and the `http.server.request.duration` histogram. Matched requests use the registered route template. Unmatched requests have no route label. Server responses carry status codes, and 5xx responses are marked as errors. Incoming W3C trace context can join the upstream trace. HTTP duration ends when the application produces the response.

Artisan command completion and timed queue attempts export spans and duration metrics. Unsuccessful commands and attempts are marked as errors. Request and response bodies, URL parameters, cookies, command arguments, job payloads, and exception messages are excluded. Standalone commands and the HTTP process flush telemetry on normal exit. Export uses a bounded span queue and retries transient failures. Overload or sustained failures can lose data.

Outgoing HTTP calls and SQL operations recorded during an execution use its active Span as their parent. HTTP client spans cover a transport attempt up to response headers. Database spans and duration metrics cover prepared statements, transactions, and cursors. Database attributes use the driver and a fixed vocabulary of operations. SQL text and bindings are excluded.

This integration does not forward logs or collect exception stack traces. Queue and command duration metrics can be queried by their `openrust.queue.job.duration` and `openrust.artisan.command.duration` names.
