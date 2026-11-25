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

Each queue dispatch through the manager can carry optional trace metadata to a Worker. The consumer's Span Link identifies the origin of an asynchronous task. Older payloads without these fields continue to run. Queue and command duration metrics can be queried by their `openrust.queue.job.duration` and `openrust.artisan.command.duration` names.

## Logs and errors {#logs-and-errors}

Set `NEW_RELIC_LOGS_ENABLED=true` and add an opted-in channel under `logging.channels`. The following shows the channel's configuration value.

```json
{
  "driver": "monolog",
  "handler": "new_relic",
  "formatter": "default",
  "level": "error",
  "bubble": true,
  "with": {"fields": ["context.operation"], "report_errors": true}
}
```

Only the selected channel sends logs. The default formatter uses the structured record and its message. List top-level context or extra fields by name in `with.fields` to include them. The handler retains bounded scalars and simple arrays and attaches the trace and span IDs captured when the record was created. Use `report_errors=false` for log-only forwarding. An error already reported through the application exception handler does not produce another error span when forwarded by this channel. Use the default formatter to avoid adding extra fields to the exported message.

The exception handler reports an error class and a safe summary. `NEW_RELIC_EXCEPTION_STACKTRACE=true` additionally sends a bounded stack trace when the application has already provided a `stacktrace` field in the exception snapshot. Review that opt-in field before exporting it.

## Runtime controls {#runtime-controls}

Traces and metrics are enabled by default when the extension is on. Disable either with `NEW_RELIC_TRACES_ENABLED=false` or `NEW_RELIC_METRICS_ENABLED=false`. Set `NEW_RELIC_TRACE_SAMPLE_RATIO` between 0 and 1 to sample root spans while honoring the parent decision. Sampling does not change metric counts. Set `NEW_RELIC_INSTANCE_ID` when a service needs an explicit replica ID. Changing these values requires rebuilding the configuration cache if it is in use.

Resolve `openrust_new_relic::NewRelic` from the application container. Inside a transaction, `name_transaction` and `add_attribute` provide bounded span customization. HTTP metrics retain their route labels. Embedding code can call `flush().await` or `shutdown().await` after draining its own work. `export_stats()` counts HTTP attempts and failures, including retries. It cannot count records dropped before an HTTP request.
