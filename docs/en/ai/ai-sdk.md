# AI SDK

## Support status {#support-status}

The optional `ai` feature provides Laravel-style `Ai`, `AiManager`, `Agent`, and a text fake. Use OpenAI Responses, OpenAI-compatible Chat Completions, Anthropic Messages, or Gemini GenerateContent for text and streaming. Agents can save text conversations in the application database.

The SDK and gateway use the framework’s [HTTP client](../digging-deeper/http-client.md).

## Available entry points {#available-entry-points}

Add `ai` to `[features].default` in your application’s `Cargo.toml` to register the SDK through package discovery. Choose a supported connection and set its credential and model ID. For example, connect to DeepSeek using the compatible driver:

```dotenv
AI_PROVIDER=deepseek
DEEPSEEK_API_KEY=
DEEPSEEK_MODEL=deepseek-flash
```

Set the API key in the application environment. Override the official base URL with `DEEPSEEK_URL` when needed. You can also define named connections in `ai.providers` in `config/ai.rs`. Set the model ID as the connection's `models.text.default` or specify it on an individual call.

```rust
use openrust_ai::{Agent, agent};

let response = agent("Answer briefly.").prompt("How do I request a refund?").await?;
let answer = response.text;
```

With the SDK enabled, run `./artisan vendor:publish --tag=ai-config` to publish the configuration template. In application tests, use `Ai::fake::<YourAgent>(responses)` to replace an agent's responses and assert prompt content and counts.

Run `./artisan ai:models deepseek` to list models, or add `--json` to distinguish discovered and configured entries. Add manual model IDs under the connection’s `models.text.ids`. The listing merges IDs and keeps manual entries when discovery fails. Run `./artisan ai:prompt "Reply briefly" --provider=deepseek --model=deepseek-flash` to test the selected model.

Run `./artisan ai:select-model --save --default` to choose a model and save its connection as the application default. Artisan uses the current default or suggests the only configured connection with credentials. To select a specific connection, supply its name. Use `--model=your-model-id` to enter a model manually and `--no-interaction` to run without prompts. Saving uses the application’s selected environment file. If the configuration is cached, rebuild it after saving.

With DeepSeek Chat Completions in thinking mode, use automatic tool selection. OpenRust keeps the upstream reasoning field with the tool step and sends it back when continuing that call. For strict JSON Schema output on DeepSeek, add a named connection with `driver: "openai"` and `url: "https://api.deepseek.com/v1"` to use the Responses API. It can use the same `DEEPSEEK_API_KEY` environment variable. [DeepSeek thinking mode](https://api-docs.deepseek.com/guides/thinking_mode/) · [DeepSeek Responses API](https://api-docs.deepseek.com/api/create-response/)

AI command help follows the application locale. Run `./artisan help ai:select-model` or `./artisan ai:prompt --help` to see localized descriptions. Override a package description in `lang/vendor/ai/<locale>/console.json` under `commands.<command>.description`, or set a command-specific argument description under `commands.<command>.arguments.<argument>`.

`make:agent`, `make:tool`, and `make:agent-middleware` use the application locale for prompts, generation results, and name validation. Override these messages under `make` in `lang/vendor/ai/<locale>/console.json`. The `:path` placeholder in `created` expands to the generated file path.

Interactive connection and model selection, including validation messages, follow the application locale. Numeric menu choices and the returned model ID stay stable across languages. Override the prompts and validation messages through `lang/vendor/ai/<locale>/console.json` under `select_model`.

Artisan displays connection and provider diagnostics in the application locale. To override a message, use `console` in `lang/vendor/ai/<locale>/errors.json`. Scripts that call `ai:models --json` continue to receive the same discovery data.

Use `agent(instructions).with_messages(history)` to include earlier turns. Build the history in order with `Message::user(text)` and `Message::assistant(text)`. Both `prompt` and `stream` send those turns before the latest prompt. Artisan accepts the same history as a JSON array of `role` and `content` entries in `--messages`.

In an HTTP route, return the default SDK SSE events with `agent(instructions).stream(prompt).await?.to_response()`. Use `into_stream()` to consume the events directly.

Call `using_vercel_data_protocol(message_id)` before `to_response()` to return Vercel AI SDK UI text events. Call `using_agent_user_interaction_protocol(thread_id, run_id)` for AG-UI text events. Pass `None` to let the SDK provide IDs. Both formats include the end state and reported token usage. Use `using_protocol` to provide a custom renderer for the same Agent events.

Attach `.catch(|error| { /* record the error */ })` to a `StreamableAgentResponse` before consuming it. Each registered callback runs once when the stream reports an error, in registration order. The consumer still receives the stream error. Handle connection and request startup failures through the result of `stream(...).await`.

Use `stream.each(|event| { inspect(event); true }).await?` to visit events and keep the response for later use. Returning `false` pauses the visit. Later consumption through `each`, `into_stream`, or `to_response` replays the inspected events with their original IDs and reads any remaining events from the same upstream request.

Register `stream.then(callback)` to receive a `StreamedAgentResponse` when consumption completes. It carries the combined text and usage, tool calls and results, and original events. A stream that pauses for approval also includes its saved approval batch. Register a callback after a complete `each` visit to receive the result immediately.

Read `result.response.steps` in the callback for each step’s text, public reasoning, citations, tool calls and results. Each step carries the model, usage, and finish reason reported by the provider. If the provider leaves the model unspecified, the step uses the model from its start event. A step paused for approval includes its tool calls and any reported usage. The response uses the last step’s model and includes reported usage from the paused step.

OpenAI Responses text streams expose web citations as `citation` events in native SSE, `source-url` in Vercel, and `CUSTOM` events named `citation` in AG-UI. Each native citation is associated with its text message. Use `StreamedAgentResponse::citations()` in a completion callback to read the ordered citations and their reported text offsets. Tool continuations keep the citations from every model step in emission order.

Anthropic Messages streams emit web search and web fetch citations through the same native SSE, Vercel, and AG-UI formats. Each model turn emits the first occurrence of a source URL and retains its title. Gemini GenerateContent streams emit the selected candidate’s grounding sources in those formats. Sources repeated across frames are emitted once per model turn.

Buffered OpenAI Responses results expose the connection, model, and URL citations through `response.meta()`. Citations keep the upstream order and each text part’s reported offsets. The method is also available on structured results. Custom text providers can use `AgentResponse::new` to initialize a response and then fill its citations. For buffered tool calls, the top-level metadata belongs to the final model step.

Buffered responses from built-in text drivers expose each model turn through `response.steps`. A step includes its own model, usage, finish reason, public summaries and citations, plus the tool calls and results for that turn. The top-level totals cover the entire invocation.

Buffered Anthropic Messages includes web search and web fetch citations. Buffered Gemini GenerateContent includes web sources from the selected candidate’s grounding metadata. Both preserve the first occurrence of each URL and expose its public title.

For a Responses model that provides public reasoning summaries, set `reasoning.summary` to `auto` in `with_provider_options`. Summary fragments have their own events in native SSE, Vercel, and AG-UI. Use `StreamedAgentResponse::reasoning()` to read the joined summary in a completion callback. Buffered Responses expose the public summary as `response.reasoning`, including structured results. A buffered tool call exposes the final model step’s summary.

The Vercel and AG-UI stream adapters use the application locale for generation failure messages. Override the generic message under `stream` in `lang/vendor/ai/<locale>/errors.json`.

Use `agent(instructions).with_schema(schema).prompt(text)` to request a JSON object. Build the schema with the framework’s `JsonSchema` builder. Read members from `response.structured` or use `response.decode::<YourType>()`. A model must support the connection’s native structured-output format. The structured stream emits the generated JSON as text deltas. To try a schema from Artisan, pass a JSON object schema to `./artisan ai:prompt` with `--schema`. The SDK disables additional properties on objects in the outgoing schema, including nested objects. Mark every property required when your model enforces strict OpenAI schemas.

For a connection needed only during the current request or command, use `Ai::build(config)` with the same fields as a named provider. Use the resulting provider's `name()` in a prompt or look it up with `Ai::text_provider` during that scope. Names reserved by the SDK or configured in the application are unavailable for temporary connections. `Ai::flush_state()` clears the current scope’s temporary connections.

### Provider failover {#provider-failover}

Plain text, structured output, and text streams can try named connections in order. Use `with_providers` on an anonymous agent or implement `Agent::providers` on a custom agent. Each connection can select a model or use its configured model tier.

```rust
use openrust_ai::{Agent, ProviderTarget, agent};

let responder = agent("Answer briefly.").with_providers([
    ProviderTarget::new("openai").with_model("first-model-id"),
    ProviderTarget::new("deepseek"),
]);
let answer = responder.prompt("What can I do next?").await?;
```

A confirmed connection setup failure, rate limit, insufficient credit, or supported overload response can trigger failover. Timeouts, disconnects after transmission, and interrupted streams after a successful HTTP response return an error to avoid replaying a request the upstream may have processed. Listen for `openrust_ai::events::AgentFailedOver` to correlate attempts by invocation ID. Setting `PromptOptions.provider` explicitly selects a single connection. A stream stays with its current connection once it delivers its first event. Tool streams can switch at startup. Tool results and subsequent model turns stay with the selected connection. Buffered tool calls can switch if the first model turn fails. The first successful model turn selects the connection and subsequent errors are returned to the caller. Calls with prompt middleware currently use only the first connection.

For built-in text protocols, the SDK retries the same request once when a rate limit or supported overload response supplies an integer `Retry-After` from 0 to 10 seconds. An exhausted quota or credit response goes directly to failover handling. The retry keeps the original invocation ID and emits a failover event only if the connection changes.

Anthropic Messages can retry or fail over on HTTP 529 and fail over on credit errors recognized by the fixed Laravel baseline. Applications can match `AiError::ProviderOverloaded` or `AiError::ProviderInsufficientCredits` to read the HTTP status. The error omits the upstream body.

### Agent prompt events {#agent-prompt-events}

Buffered prompts dispatch `PromptingAgent` before each provider attempt, followed by `AgentPrompted` with the response or `AgentFailed` with the typed error. Listen through the framework’s Event facade using each event’s `NAME`. Every attempt and any `AgentFailedOver` event share the invocation ID. `event.prompt.as_prompt()` exposes the request resolved for that connection before step middleware.

The SDK dispatches `StreamingAgent` before starting each provider attempt for a stream. A fully consumed stream dispatches `AgentStreamed` with the result, and startup or read errors dispatch `AgentFailed`. Approval pauses carry the saved approval batch. A remembered conversation is saved before the success event, and the event includes its conversation ID. Register listeners before starting the request. A client that stops consuming a stream can leave it without a success event.

For buffered or streaming generation, listen to `StartingStep`, `StepCompleted`, and `StepFailed` to inspect actual model attempts. Step numbers start at zero. The starting event carries the resolved request and the preceding tool results. The request snapshot reflects any changes made by step middleware. The completion event carries the provider’s reported model, usage, and finish reason before the step’s tools run. Middleware that returns a response directly produces no model-step event.

### Connect to a Responses gateway {#responses-gateway}

For a Sub2API connection that exposes OpenAI Responses, use `driver: "openai"` and the gateway's v1 base URL. The SDK uses `/models` for discovery and `/responses` for generation. Set `store: false` to send a request without provider-side storage.

```rust
config["providers"]["sub2api"] = serde_json::json!({
    "driver": "openai",
    "url": env.get_utf8("SUB2API_URL")?.unwrap_or("https://ai.example.com/v1"),
    "key_env": "SUB2API_API_KEY",
    "store": false,
    "models": {"text": {"default": "gpt-6.1-sol"}},
    "headers": {"x-openai-actor-authorization": "local-image-extension"}
});
```

Set the bearer key in `SUB2API_API_KEY`. Add a `headers` object only if your deployment requires extra headers. Names and values must be valid HTTP headers. Authentication and headers managed by the SDK remain controlled by the connection and protocol. Run `./artisan ai:models sub2api --json` to check whether the configured model is also discoverable.

## Azure OpenAI {#azure-openai}

To use the `azure` preset, set `AZURE_OPENAI_URL` to your Azure resource URL and set `AZURE_OPENAI_API_KEY`. The preset appends `/openai/v1` when needed. Use your deployment name as `AZURE_OPENAI_DEPLOYMENT` for text. For a different named connection, use `driver: "openai"`, the full v1 URL, and `auth: "api-key"`.

## Scope {#scope}

Responses provide text, the connection, the model, and input and output token counts when reported by the upstream service. Native Anthropic and Gemini connections also support text, SSE, and paginated model discovery.

For text, `Usage` exposes the total prompt and completion tokens and any cache-read, cache-write, or reasoning counters reported by the provider. Cache tokens are included in the prompt total, and reasoning tokens in the completion total. `total_tokens()` and `uncached_input_tokens()` derive totals when the required counts are available. Stream formats carry the same reported counters.

## Model gateway {#model-gateway}

Add `ai-gateway` to `[features].default` in your application’s `Cargo.toml` and set `AI_GATEWAY_ENABLED=true`. Run `./artisan vendor:publish --tag=ai-gateway-config`. Update `models` and `keys` in `config/ai_gateway.rs` with your public model mappings and client permissions, then rebuild the application.

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

Set a separate randomly generated secret for `AI_GATEWAY_SUPPORT_KEY` in the application environment. The upstream connection uses `DEEPSEEK_API_KEY`. Set the external client’s base URL to `https://your-app.example/v1`, use the client Key as a Bearer credential, and select `support-chat`. `GET /v1/models` lists published models available to that Key. The optional `requests_per_minute` uses the application's configured rate-limit cache.

Gateway authentication errors and terminal SSE failure messages follow the application’s locale, including its configured language header. Override `invalid_api_key` and `stream` in `lang/vendor/ai-gateway/<locale>/errors.json`.

`POST /v1/chat/completions` supports OpenAI and compatible connections. `POST /v1/responses` can route natively to OpenAI connections or translate requests to Anthropic Messages. Both support JSON and SSE. Responses can also route to a Chat Completions-only connection. For this mapping, the gateway converts stateless text, system instructions, basic generation options, JSON object mode, and client-side function tools. It keeps tool call IDs and streams argument deltas. Attachments and JSON Schema require a connection or conversion that supports them. Responses runs in stateless mode with `store` set to `false`. Upstream conversations and previous response IDs are unavailable. Server-side web search requires explicit permission on the published model. The `ai_gateway.call` structured log records outcomes and upstream-reported token counts. Set `usage_channel` to select a logging channel that accepts `info` records. Unreported token counts are recorded as `null`.

For an Anthropic client, set the base URL to your application domain and send the client Key in `x-api-key`. The gateway accepts `/v1/messages` and `/v1/messages/count_tokens` for models mapped to Anthropic connections. Messages requests can also route to a published OpenAI Responses or Chat Completions model. The gateway preserves client tool call IDs and sends each tool's content block in order. `/v1/models` and `/v1/models/{model}` use the Anthropic format with `x-api-key` and the OpenAI format with Bearer authentication.

Use `POST /v1/responses/input_tokens` to count the input for a published OpenAI or Anthropic model. Send the model and stateless input in JSON. The gateway uses the upstream tokenizer and returns `input_tokens`. The Messages counting endpoint can use an OpenAI Responses connection as well. Counting requests share the Key's request limit, and generation logs omit these counts.

Set `unversioned_paths` to `true` in `config/ai_gateway.rs` when a client needs paths without `/v1`. This adds `/models`, `/models/{model}`, `/chat/completions`, `/responses`, `/responses/input_tokens`, `/embeddings`, and `/messages/count_tokens` with the same authorization and limits.

Both conversion directions support buffered JSON and SSE, and use the published model name in client responses. When upstream reports cache reads or writes, the gateway includes those counts using the destination protocol’s usage fields.

The same published Anthropic model can serve Messages, Responses, and Chat Completions clients. Translation supports text, system instructions, basic sampling options and client function tools in JSON and SSE. Tool conversion preserves call IDs, streams argument deltas and accepts text results in both directions. Use a native endpoint for structured output or attachments.

Gemini clients use the `/v1beta` API and send the client Key in `x-goog-api-key`. The gateway lists authorized Gemini models and accepts `:generateContent`, `:streamGenerateContent?alt=sse`, and `:countTokens` under `/v1beta/models/{model}`. In the SDK and Artisan, choose the `gemini` connection and enter its model ID with or without the `models/` prefix.

Add `fallbacks` to a public model mapping to try backup connections in order. Each entry specifies `provider` and `model`, for example `[{"provider":"openai","model":"backup-model"}]`. The gateway can retry stateless requests after eligible upstream failures, including 429 and 5xx responses. It keeps the same public model and records each attempt under one request ID. Requests with tools or stored output, and streams already sent to the client, are excluded from replay.

Use `accounts.deepseek.cooldown_seconds = 30` in `config/ai_gateway.rs` to pause a named upstream connection after HTTP 429, 503, or 529. A valid upstream `Retry-After` takes precedence, up to 24 hours. The pause applies to every published model using that connection. When all eligible connections are cooling down, the gateway returns HTTP 503 with `Retry-After`. Set `account_cache_store` to a shared store such as `redis` to coordinate across gateway processes.

To keep a client session on the last successful upstream target, set `account_session_ttl` to a value from 1 to 86400 seconds and send `X-Session-ID` on each request. The ID is scoped to the downstream Key and public model. The preferred target still goes through authorization, cooldown and concurrency checks. Successful responses refresh the TTL. During a cache outage the gateway uses the configured order. Reuse `account_cache_store` for shared affinity across replicas.

Set `accounts.deepseek.max_concurrent_requests` to cap active requests for that connection across replicas using the same cache. A stream holds a slot until it finishes or the caller disconnects. When the connection is full, the gateway checks the next eligible connection before sending the request. If all available connection slots are full, it returns HTTP 503 with `upstream_busy`.

To share an access policy across keys, add a group and set the key's `group` field. A group's `models` lists published aliases. Its optional `providers` limits upstream connections for both initial requests and fallback. A key can also specify `models` to narrow that group's model list.

```rust
"groups": {
    "support-team": {"models": ["support-chat"], "providers": ["deepseek", "openai"]},
},
"keys": {
    "support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "group": "support-team"},
},
```

For database-managed keys, set `managed_keys.enabled` to `true`. The optional `managed_keys.connection` selects a named database connection. Run the package migration and issue a key for an existing group.

```shell
./artisan migrate
./artisan ai:gateway:key:create support-client support-team --expires-in=86400 --requests-per-minute=60
./artisan ai:gateway:key:list
./artisan ai:gateway:key:rotate KEY_ID
./artisan ai:gateway:key:revoke KEY_ID
```

Creation and rotation return the bearer token once. The database stores its SHA-256 digest. Rotation preserves the key ID, group, expiry and request limit. Subsequent requests with rotated, revoked or expired tokens receive HTTP 401. To customize the migration, publish it with `./artisan vendor:publish --tag=ai-gateway-migrations` and rebuild before running migrations.

### Responses over WebSocket {#responses-websocket}

Connect to `wss://your-app.example/v1/responses` with the same Bearer Key as the HTTP endpoint. With `unversioned_paths` enabled you can also connect to `/responses`. Send a `response.create` JSON text frame and read the Responses events until `response.completed` or `response.incomplete`.

```json
{
  "type": "response.create",
  "model": "support-chat",
  "input": "How do I return an item?"
}
```

Use a distinct `stream_id` for each parallel stream. A socket accepts up to four active streams with one response per stream. Events carry their stream ID. Every turn uses the same authorization, routing, limits and usage accounting as HTTP. Closing the socket cancels all its active responses.

For text and client-side function calls, send the latest completed response ID as `previous_response_id` and send only new messages or function results in `input`. Use the same stream to continue or a new `stream_id` to branch from an available response. Omit `model` to reuse the parent alias. `instructions` applies to the current turn. The gateway retains up to four recent histories on the socket, each bounded to 512 KiB and 256 items. If you reconnect, change the model, or receive `previous_response_not_found`, resend the full context.

Text frames are limited to 2 MiB. Reconnect after one hour.

### Codex model catalog {#codex-model-catalog}

A Bearer-authenticated request to `/v1/models?client_version=0.170.0` returns the Codex catalog. Codex clients can also set the base URL to `https://your-app.example/backend-api/codex`. That prefix serves `/models`, `/responses` over HTTP and WebSocket, and `/responses/input_tokens`. Only aliases available to the current Key through the Responses endpoint appear. The ordinary OpenAI and Anthropic model endpoints keep their original formats.

Add an optional `codex` object to a public model mapping to set its display name, description, context window, input modalities and reasoning choices. By default the catalog uses the public alias for its display name and advertises text input. A conditional request with `If-None-Match` can reuse the existing authorized catalog.

```json
{
  "models": {
    "support-chat": {
      "provider": "openai",
      "model": "upstream-model",
      "codex": {
        "display_name": "Support Assistant",
        "context_window": 40000,
        "input_modalities": ["text", "image"]
      }
    }
  }
}
```

Set a Responses provider for Codex with the gateway base URL, such as `https://your-app.example/v1`, and the downstream Key. Use a published alias as the model. Native Responses upstreams can receive client tool namespaces. For upstream web search, allow `"server_tools": ["web_search"]` on the public model. Set `web_search = "disabled"` in the Codex client configuration when the model has no web search enabled.

For native xAI Responses, publish the tools needed by the public model. Use `code_interpreter` for the xAI sandbox, and `file_search` for searching collections listed in `file_search_collections`. Every requested collection ID must match an allowed ID exactly. The downstream key must also be eligible to use a native connection that supports all requested tools. Tool requests with an unknown cost ceiling follow the configured budget policy. The default rejects these requests when a hard budget applies, and `unknown: "monitor"` allows them with billing observation.

```rust
"grok-tools": {
    "provider": "xai",
    "model": "grok-4.7",
    "endpoints": ["responses"],
    "server_tools": ["web_search", "code_interpreter", "file_search"],
    "file_search_collections": ["collection-id"],
},
```


### Gateway image generation and editing {#gateway-images}

Publish an image model in `ai_gateway.models` with explicit `images/generations` and `images/edits` endpoints. Authorize its public name in the usual Key or group configuration. Use an OpenAI connection or an OpenAI-compatible connection with image support configured under `models.images`.

```json
{
  "catalog-image": {
    "provider": "openai",
    "model": "gpt-image-1",
    "endpoints": ["images/generations", "images/edits"]
  }
}
```

Use `POST /v1/images/generations` with JSON and `POST /v1/images/edits` with JSON or multipart. Multipart accepts `image` or an ordered `image[]` list and an optional `mask`. Use inline URLs or data for JSON references. Provider file IDs are unavailable through shared gateway credentials. `stream: true` relays partial and completed SSE images. The gateway records upstream-reported usage and sends each image request upstream once. Groups with text output-token quotas need a separate group for images.

### Connect DeepSeek Harness {#dsh-model-connection}

In DeepSeek Harness, open **Settings → Models → Add model provider** and choose **Custom model API**. Enter a provider ID, the gateway's downstream Key, and the base URL for the chosen protocol.

| DSH protocol | Base URL |
| --- | --- |
| OpenAI Chat Completions | `https://your-app.example/v1` |
| OpenAI Responses | `https://your-app.example/v1` |
| Anthropic Messages | `https://your-app.example` |
| Gemini API | `https://your-app.example/v1beta` |

Choose an alias from **Fetch available models**, or enter a published alias with **Add model**. Create the provider, then select its model in the composer.

### Persist gateway usage {#gateway-usage}

Set `usage.enabled` to `true` in `config/ai_gateway.rs` and run the migration. Set `usage.connection` to a named database connection when gateways share a usage database.

```shell
./artisan migrate
./artisan ai:gateway:usage:list --limit=20
./artisan ai:gateway:usage:list --request-id=REQUEST_ID
```

When persistent usage is enabled, the gateway saves reported final usage before sending the downstream completion marker. A stream can end before the provider reports final counts. In that case, any available counts remain in the attempt record.

The Artisan attempt list reports `usage_state` as `not_dispatched`, `unknown`, `partial`, `final`, or `legacy`. Explicit zero counts are preserved. It also shows the dispatch time, upstream request ID and reported model when available. A recorded dispatch time means the gateway was about to send the request, so an interrupted attempt may still require reconciliation.

### Text billing {#text-billing}

Set `billing.enabled` and `usage.enabled` to true. Publish and run the gateway migrations. Specify the price version, upstream rates by connection and upstream model, and receivable rates by published model.

```json
{
  "billing": {
    "enabled": true,
    "version": "prices-2026-11",
    "upstream": {
      "openai": {"upstream-model": {"currency": "USD", "input": "1", "output": "4"}}
    },
    "downstream": {
      "published-model": {"currency": "USD", "input": "1.25", "output": "5"}
    }
  }
}
```

Each rate is a decimal string in the specified currency per million tokens. Set `cache_read`, `cache_write` or `reasoning` only when those tokens have distinct rates. Each charge uses its captured tariff. Missing usage evidence or prices can leave an amount pending. Use `./artisan ai:gateway:usage:list --billing` to inspect cost and receivable records, their price versions and their exact decimal amounts. Each page reads usage, charges and invoice evidence in one database statement.

Set `groups.<group>.rate_multiplier` to a nonnegative decimal string such as `"0.5"` to adjust the published model’s downstream charge for that group. The gateway applies the multiplier before rounding the amount and before reserving group funds. The recorded price snapshot retains the multiplier in effect when the request started.

Each upstream attempt has its own record. Retries share a `request_id`. The record includes the downstream Key or group, published model, connection, upstream model, response status, and the token counts the upstream actually reports. Missing counts are `null`. The ledger omits Key secrets, request bodies and response text.

Set `"forward_usage_context": true` on the SDK connection to your gateway in `ai.providers`. The gateway records `consumer_invocation` and the zero-based `model_turn` for each actual text request. Use `./artisan ai:gateway:usage:list --billing --invocation-id=<id>` to inspect the attempts, and add `--invocation-scope=<scope>` to limit results to the originating Key scope. Each model turn gets its own gateway request ID. Correlation IDs have no effect on charging or request deduplication.

To use native xAI tools from an SDK Agent through your gateway, add a separate connection in `ai.providers` with `driver: "xai"`. Set its URL to the gateway base URL ending in `/v1`, use a downstream gateway key, and set `models.text.default` to the published model alias. Set `store: false` and enable `forward_usage_context` to correlate the model steps in the gateway ledger. Pass `ProviderTool::options` under this SDK connection name. When replaying provider tool results or encrypted reasoning between steps, pin the public model to the upstream connection that can read that state.

A request cannot be sent upstream until its attempt is recorded. The gateway settles complete responses, errors and cancellations. A process that stops before settlement can leave a `pending` record for reconciliation.

With the same Key used for generation, call `GET /v1/usage?days=7` using Bearer or `x-api-key` authentication. The response includes today's totals, lifetime totals, and a daily series in UTC for that Key. Set `days` from 1 to 90. Retries count once as a request and separately as attempts. Missing token counts remain `null`.

`model_stats` reports the Key’s usage by public model name over the last 30 days. Set `start_date` and `end_date` to UTC dates in `YYYY-MM-DD` format to choose another range. Both dates are inclusive. Missing or invalid dates use the corresponding default. This range applies only to `model_stats`.

Managed Keys retain their usage history after rotation. Replacing an environment Key starts a new history for its new credential. The operator's Artisan ledger includes earlier attempts and upstream details.

#### Import provider cost evidence {#import-cost-evidence}

Use `./artisan ai:gateway:billing:import invoice.json --actor=operator` for a provider statement with per-request amounts. The command matches the named provider and its request ID within the UTC time window. The start is inclusive and the end is exclusive.

```json
{
  "source": "provider-invoice",
  "statement_id": "daily-2026-11-26",
  "revision": 1,
  "complete": true,
  "starts_at": 1795651200,
  "ends_at": 1795737600,
  "entries": [
    {
      "id": "invoice-line-1",
      "provider": "openai",
      "upstream_request_id": "req_example",
      "currency": "USD",
      "amount": "0.03"
    }
  ]
}
```

Use `complete: false` for a partial statement. For a corrected statement, keep its source, statement ID and time window, then increase the revision. Reimporting the same revision and contents is idempotent. The usage list's original `amount` preserves the rate-based estimate, and `actual` shows the confirmed supplier cost when available. A currency mismatch or conflicting report is visible for operator review.

### Aggregate provider statements {#provider-cost-totals}

For a statement grouped by upstream connection and currency, supply `totals` to the same import command. Convert the provider’s original statement window to UTC timestamps. Use `category: "tokens"` for token charges. Other categories are retained with their original amounts.

```json
{
  "source": "daily-provider-costs",
  "statement_id": "2026-11-26",
  "revision": 1,
  "complete": true,
  "starts_at": 1795651200,
  "ends_at": 1795737600,
  "totals": [
    {
      "id": "tokens-usd",
      "provider": "openai",
      "currency": "USD",
      "category": "tokens",
      "amount": "0.16"
    }
  ]
}
```

At import, the gateway compares each total with the ledger for that connection, currency and window. Confirmed per-request evidence takes precedence over the corresponding price calculation. The result stores the reported and recorded amounts, the signed difference and counts of priced and pending attempts. Partial statements, unknown costs and unmapped categories stay available for review. Aggregate differences stay at the statement level.

Run `./artisan ai:gateway:billing:report <report-id>` to read the saved entries, comparison and import details. Use the `report_id` printed by the import command.

For a statement split across files, include `"page": {"number": 1, "total": 3}` and keep the source, statement ID, revision, UTC window and `complete` value identical across files. Run the same import command for each file, in any order. Use `billing:report` and its `pagination.received_pages` field to find missing pages. The gateway assembles the statement and records its evidence after all pages arrive. A complete set of pages declared with `complete: false` stays a partial statement.

### Debit and credit adjustments {#billing-adjustments}

Run `./artisan ai:gateway:billing:adjust adjustment.json --actor=operator` to add a debit or credit to a finalized charge. Use `direction: "cost"` for upstream expense or `"receivable"` for a downstream charge. Use the charge currency. The reference identifies the supporting document or line. When the supporting evidence is a complete imported statement, its `report_id` can be included.

```json
{
  "attempt_id": "01J9GVMJMGV80VGEX9TEXA8ETB",
  "direction": "receivable",
  "kind": "credit",
  "currency": "USD",
  "amount": "0.05",
  "reference": "credit-memo-42",
  "reason": "Approved correction"
}
```

The original rate, amount and evidence remain available. `usage:list --billing` displays `adjustments` with debits, credits, net amount and audit entries. Submit the same attempt, direction and reference with the same contents to retry safely. If the original amount still needs confirmation, the net stays pending. Aggregate comparisons use the net available when the statement is imported.

### Daily group billing budget {#billing-budget}

In `config/ai_gateway.rs`, enable usage and billing and run the gateway migrations. Set the group budget in `groups.<group>.budget`. In `billing.upstream.<connection>.<model>`, set `input_token_limit` to the upstream model's verified maximum billable input Tokens per request, including cached Tokens. Set a downstream rate in the same currency. The limit applies across all Keys in the group and all gateway instances using the same database.

```json
{
  "groups": {
    "team": {
      "models": ["published"],
      "budget": {
        "currency": "USD",
        "daily_limit": "20.00",
        "max_output_tokens": 4096
      }
    }
  }
}
```

The gateway caps outgoing text generation before reserving the maximum receivable. Successful calls settle at the recorded rate and release the difference. Failed attempts without a downstream charge release their receivable reservation. If final usage is missing or the process stops after dispatch, the reservation stays in its original UTC day. An upstream amount above the declared maximum stops further admissions for that group in that window and remains visible in the charge ledger.

Run `./artisan ai:gateway:budget:show team` to see the current UTC window's limit, reserved amount, amount booked within the limit and available balance. Exhaustion returns HTTP 429 with `budget_exceeded`. Routes without a verified ceiling return `budget_unbounded` unless you set `unknown` to `monitor`. Monitor routes keep their regular usage and billing behavior.

After a restart, run `./artisan ai:gateway:budget:recover team` to release stale reservations for attempts that never reached the dispatch marker. The default age is 300 seconds. Use `--after=SECONDS` and `--limit=COUNT` to control the scan. Recovery closes the attempt and its charge records together with the reservation release. Attempts that reached the dispatch marker stay available for reconciliation.

### Daily upstream account cost budget {#upstream-cost-budget}

Configure `accounts.<connection>.budget` with the same fields as a group budget. Use the currency of the upstream rate. An account budget applies to every public model and group routed through that connection. When both budgets apply, the gateway reserves the cost and receivable in one transaction before dispatch.

```json
{
  "accounts": {
    "openai": {
      "budget": {
        "currency": "USD",
        "daily_limit": "100.00",
        "max_output_tokens": 4096
      }
    }
  }
}
```

If the account budget has no room, the gateway can select another authorized upstream. The gateway returns HTTP 429 with `upstream_budget_exceeded` when all eligible accounts are out of budget. A failed upstream attempt with unknown cost keeps its cost reservation. The group receivable from that attempt can still be zero.

Run `./artisan ai:gateway:budget:show openai --account` to inspect the account balance or `./artisan ai:gateway:budget:recover openai --account` to recover stale, never-dispatched attempts by connection. Recovery releases both reservations for an attempt that held both.

After a supplier statement or charge adjustment resolves a cost, run `./artisan ai:gateway:budget:reconcile <attempt-id> --direction=cost --reference=<evidence-id> --actor=operator`. Use `--direction=receivable` for a downstream adjustment. The command settles the confirmed amount in the original budget window. Retrying the same evidence reference returns its original record. For a corrected amount, use a new evidence reference. An amount above the original reservation ceiling stops new admissions for that window and remains visible for review.

Run `./artisan help ai:gateway:budget:reconcile` for gateway command help in the application locale. Override descriptions in `lang/vendor/ai-gateway/<locale>/console.json` under `commands.<command>.description` or `commands.<command>.arguments.<argument>`. Command, argument, and option names stay the same across locales.

### Group output-token quotas {#gateway-output-quota}

Enable the usage ledger and run the migrations before adding a group quota. All keys in the group share a daily output-token budget across gateway instances. The window resets at midnight UTC.

```json
{
  "groups": {
    "support-team": {
      "models": ["support-chat"],
      "quota": {
        "output_tokens_per_day": 100000,
        "max_output_tokens": 4096
      }
    }
  }
}
```

The gateway caps each attempt at `max_output_tokens` and reserves that amount before calling upstream. Explicit lower limits reduce the reservation. Successful responses settle against reported output tokens. Failed or cancelled attempts without a final count keep their reservation until the window resets. Each fallback needs its own reservation.

Exhaustion returns HTTP 429 with `Retry-After`. A provider that reports more output than reserved blocks further generation for the group until the next window. Apply the quota to Chat Completions, Responses and Anthropic Messages generation. Token-count and embedding requests use their existing request limits.

## Remembered conversations {#conversations}

Implement `RemembersConversations` on an Agent to store its conversation in the application's database. Publish the migration, rebuild the application, then migrate.

```bash
cargo run --features ai -- artisan vendor:publish --tag=ai-migrations
cargo run --features ai -- artisan migrate
```

```rust
use openrust_ai::{ConversationParticipant, RemembersConversations};

impl RemembersConversations for SupportAgent {}

let participant = ConversationParticipant::new("customer", authenticated_id);
let first = agent.for_participant(participant.clone()).prompt("Hello").await?;
let next = agent.r#continue(first.conversation_id, participant).prompt("Continue").await?;
```

Pass the authenticated participant's type and ID when continuing a conversation. `for_user(&user)` can start a conversation directly from an authenticatable model. `continue_last_conversation(participant)` finds that participant's last conversation with the same Agent type and starts a new conversation if none exists.

The SDK saves completed user and assistant messages together. The next prompt receives the latest 100 messages in chronological order by default. Change `conversations.max_messages` in `config/ai.rs` to adjust that limit. The store uses the default database connection unless `AI_CONVERSATIONS_CONNECTION` names another connection. For a history screen, use `Ai::conversation_store()?.paginate_conversation_messages(id, &participant, 15, cursor)` for newest-first cursor pagination. Use `stream()` for a streamed turn and pass its `response` to your SSE renderer. The conversation ID is available immediately. The SDK saves the turn before sending the final completion event. If an upstream call fails after a tool step completed, the database keeps the completed steps and marks the assistant message as `failed`. An upstream error before the first tool step completes leaves no new messages. If the final structured output is invalid, the same rule preserves any tool steps that already completed. For an Agent with a schema, use `prompt_structured()` or `stream_structured()`.

For a new conversation, the database store generates a title using the successful connection’s cheapest text model. The title Agent receives at most 500 display columns from the first prompt. Set `conversations.generate_title` to `false` in `config/ai.rs` or set `AI_CONVERSATIONS_GENERATE_TITLE=false` to use a 50-column preview. A failed title request uses a 100-column preview. An Agent Fake can supply a title through `ConversationTitleAgent::fake`.

Override the table names before running the published migration. Set `conversations.tables.conversations` and `conversations.tables.messages` in `config/ai.rs`, or set `AI_CONVERSATIONS_TABLE` and `AI_CONVERSATION_MESSAGES_TABLE`. The defaults are `agent_conversations` and `agent_conversation_messages`. Use distinct SQL identifiers with letters, digits, or underscores and an optional schema supported by the database. Keep the same configuration for migration, runtime, and rollback. To change the names on an existing deployment, migrate the existing tables and their data before switching the configuration.

Assistant history for completed SDK turns includes the model, total usage, and `steps` with each step’s reported model, usage, finish reason, and tool outcomes. Older records retain their stored steps. The next prompt reconstructs the recorded assistant tool calls and their results in order. `conversations.max_messages` limits the stored rows before expansion. Conversation history pagination still returns each stored row with its `steps`. For manual histories, use `Message::assistant_with_tool_calls` followed by `Message::tool_results`.

A tool step can replay Chat Completions private reasoning, Responses encrypted reasoning, Anthropic signed blocks, or Gemini thought signatures on the same connection, credentials, and requested model. The selected alias remains eligible when the provider reports a different model revision. Changing the connection or model uses the recorded calls and results. Public history, callbacks, and stream events keep their existing response format.

Conversation messages and pending approval checkpoints store file references in the database. Files are written with private visibility to the `local` disk by default. Set `conversations.attachments.disk` in `config/ai.rs` or `AI_CONVERSATION_ATTACHMENTS_DISK` to select a private disk shared by your application processes. Keep disks referenced by existing records available when changing the setting.

If you have older records with inline Base64 attachments, run `./artisan ai:migrate-attachments`. The command writes and verifies each file before replacing its database value with a reference. Failed records remain readable in the previous format. You can run the command again, and its JSON output reports the migrated message and approval counts.

Implement `ConversationStore` and install it during application boot with `Ai::set_conversation_store(Arc::new(store))` to use a different backend. `Ai::conversation_store()` returns the configured store. Each implementation checks participant ownership and saves the user and assistant messages atomically. Use `DatabaseConversationStore` for durable tool approvals. A custom store can opt into attachments through `supports_attachments` and `store_turn_with_message`.

To import conversation history, call `store_conversation`, `store_user_message`, and `store_assistant_message` through `Ai::conversation_store()`. Pass an existing ID when creating a conversation, or pass `None` to generate a UUIDv7. Each write takes a participant, and appending checks conversation ownership. The database store writes user attachments to the configured private disk. Pass `Some(&error)` when saving an assistant message to record the failure and its error summary. Each call commits independently. Regular Agent conversations still save each turn atomically.

## Text generation options {#text-generation-options}

Set the output token limit and sampling options on an Agent. Use `with_generation_options` for an anonymous Agent or implement `Agent::generation_options` for a named Agent. The settings also apply to structured output and streaming tool steps. Agent middleware can override them for one step.

```rust
use openrust_ai::{Agent, TextGenerationOptions, agent};

let response = agent("Keep the answer concise.")
    .with_generation_options(TextGenerationOptions {
        max_tokens: Some(256),
        temperature: Some(0.5),
        ..Default::default()
    })
    .prompt("Summarize our return policy.")
    .await?;
```

The driver maps options to the provider’s wire format. With Anthropic, the SDK sends sampling options for supported Claude models. If you set both temperature and top-p, it sends temperature.

### Provider-specific options {#provider-options}

Use `with_provider_options` for a named connection or implement `Agent::provider_options`. Extensions apply when that connection is selected and continue through its tool steps. Core request fields such as the model, messages, streaming mode, and structured schema are set through the SDK. Gemini’s `generationConfig` accepts extra entries alongside the SDK’s settings.

```rust
use openrust_ai::{Agent, ProviderOptions, agent};
use serde_json::json;

let options = ProviderOptions::from_iter([("reasoning".into(), json!({"effort": "low"}))]);
let assistant = agent("Keep the answer concise.")
    .with_provider_options("openai", options);
let response = assistant.prompt("Summarize our return policy.").await?;
```

## Text summarization {#text-summarization}

Import the extension traits to use the same summarization agent from `Str` and `Stringable`. The default requests at most three sentences. `summarize_with` accepts a sentence count and the usual `PromptOptions` for a connection, model and timeout.

```rust
use openrust::support::Str;
use openrust_ai::{PromptOptions, Summarizes, SummarizesText};

let short = Str::summarize(article).await?;
let detailed = Str::of(article)
    .summarize_with(5, PromptOptions::default())
    .await?;
```

Summarization uses `models.text.cheapest` for the selected connection. Agents can also choose `TextModelTier::Smartest`, which uses `models.text.smartest`. For presets, set `OPENAI_MODEL_CHEAPEST` and `OPENAI_MODEL_SMARTEST` or use the same suffixes with the preset’s model variable. An unset tier uses the connection’s default model. An explicit model takes precedence over a tier.

## Image attachments {#prompt-images}

```rust
use openrust_ai::{Agent, ImageAttachment, agent};

let photo = ImageAttachment::from_storage("photos/sample.png", None).await?;
let response = agent("Describe the scene.")
    .with_attachments([photo])
    .prompt("What can you see?")
    .await?;
```

Read an uploaded image with `ImageAttachment::from_upload(&file)`, a local file with `ImageAttachment::from_path(path).await`, or raw bytes with `ImageAttachment::from_bytes(bytes)`. Supply the attachments through `Agent::attachments` on a named agent or `with_attachments` on an anonymous agent.

The selected model must support image input. OpenAI Responses, Chat Completions, Anthropic Messages, and Gemini each use their native image format. The SDK accepts PNG, JPEG, and WebP, with at most 16 attachments and less than 50 MB of raw images per prompt. Provider limits also apply. Attachments can accompany text or be the only input. They travel with the current turn and its tool steps.

### Document attachments {#document-attachments}

Load a PDF, UTF-8 text file, or Markdown file with `DocumentAttachment`. It supports `from_bytes(filename, bytes)`, `from_upload(&file)`, `from_path(path).await`, and `from_storage(path, disk).await`. Wrap mixed files in `Attachment` to keep them in the order you supply.

```rust
use openrust_ai::{Agent, Attachment, DocumentAttachment, ImageAttachment, agent};

let report = DocumentAttachment::from_storage("reports/quarterly.pdf", None).await?;
let chart = ImageAttachment::from_storage("reports/chart.png", None).await?;
let response = agent("Review the quarterly report.")
    .with_attachments([Attachment::from(report), Attachment::from(chart)])
    .prompt("Summarize the results.")
    .await?;
```

Use a document-capable model. Responses, Anthropic Messages, and Gemini accept these three document formats. Chat Completions accepts PDF only. Documents and images share the per-prompt count and size limits.

Use `Message::user_with_attachments(text, attachments)` with `with_messages` to supply attachments on a specific earlier user message. The current turn and its history share the attachment limits of 16 files and less than 50 MB of raw data. Read the files through `Message::attachments` in Rust. The standard JSON representation of `Message` contains the role and text.

## Agent tools {#agent-tools}

Implement `Tool` with a name, description, JSON Schema, and asynchronous `handle` method. Use `ToolRequest::validate` to apply framework validation rules or `decode` to read arguments into a Rust type. The tool receives an ID for the provider call and a separate ID for its execution.

```rust
let response = agent("Check the catalog before answering.")
    .with_tools(vec![std::sync::Arc::new(LookupInventory)])
    .prompt("How many units are available?")
    .await?;
let answer = response.text;
```

Agent tools work with buffered and streaming Chat Completions, Responses, Anthropic Messages, and Gemini GenerateContent. Claude and Gemini receive their original model content again with the tool results so the provider can verify its signatures. The SDK streams text as it arrives, then emits the completed tool call and its result before the next model step. The default SSE, Vercel UI, and AG-UI formats carry these tool events. The Agent sends results back with their call IDs and exposes `tool_calls` and `tool_results` on the response. Use `with_max_steps(n)` to limit model steps. Tool execution stops on the last step, and any calls proposed in that step appear as failed results.

### Tool execution events {#tool-execution-events}

Listen to `InvokingTool`, `ToolInvoked`, and `ToolFailed` through the Event facade. The events share the Agent invocation ID and a tool invocation ID for the handler run. `event.invocation.call` contains the tool name, provider call ID, and actual arguments, including edits made during approval. The terminal event carries the result or typed error and the handler duration. Pending, rejected, and skipped calls do not run the handler or dispatch execution events.

### Choose a tool {#tool-choice}

Use `with_tool_choice` on an anonymous Agent or `Agent::tool_choice` on a named Agent. `Auto` lets the model choose, `None` forbids tool calls, and `Required` requires at least one tool call. `ToolChoice::tool(name)` selects one registered tool. Required and named choices apply to the first step. Subsequent steps can use the result and answer normally.

```rust
use std::sync::Arc;
use openrust_ai::{Agent, ToolChoice, agent};

let response = agent("Check inventory before answering.")
    .with_tools(vec![Arc::new(LookupInventory)])
    .with_tool_choice(ToolChoice::tool("lookup_inventory"))
    .prompt("Is this item in stock?")
    .await?;
```

### Similarity search tool {#similarity-search}

`SimilaritySearch::using_model` converts a tool’s search text into a vector and queries an Eloquent model, returning document fields. Use `using_model_with` to add your application’s query constraints or `using` to supply an async search function. Pass `rerank` a function that extracts text from each result and an optional limit to rank the candidates with your configured reranking connection.

The default tool and `query` descriptions use the current locale when the schema is generated. Override these phrases with `similarity_search.description` and `similarity_search.query` in `lang/vendor/ai/<locale>/tools.json`. An explicit `with_description` is used as provided.

```rust
use std::sync::Arc;
use openrust_ai::{Agent, SimilaritySearch, agent};

let search = SimilaritySearch::using_model::<PolicyDocument>("embedding", 0.6, 15)?;
let response = agent("Search the policy documents before answering.")
    .with_tools(vec![Arc::new(search)])
    .prompt("When can I return an order?")
    .await?;
```

### Use an agent as a tool {#agent-as-tool}

Wrap an agent with `AgentTool::new(your_agent)` and pass it to the parent like any other tool. The tool uses the child type name by default and accepts a required `task` string. Implement `CanActAsTool` and call `into_tool()` to set the name and description explicitly. Use `with_structured_output()` on a child implementing `HasStructuredOutput` to return its output as a JSON string within the tool result.

The child uses its own instructions, model and provider. The parent sends the task only. The child's response is returned when it completes, including when the parent is streaming.

The default description and the `task` field help are translated when the tool schema is generated, using the current application locale and its configured fallback. Override individual phrases in `lang/vendor/ai/<locale>/tools.json` under `agent.description` and `agent.task`. Use `:agent` in the description to insert the tool name.

### MCP client tools {#mcp-tools}

Use a connected `rmcp` client to discover the tools your application wants to expose. Pass its peer and the selected definition to `McpTool::new`. The application manages the MCP connection, authentication, and transport. The model sees the tool name with a `mcp_tools_` prefix, and OpenRust sends the original name and arguments to MCP. Unsupported input schemas return `AiError::InvalidSchema` when creating the adapter.

```rust
use std::sync::Arc;
use openrust_ai::{McpTool, agent};

let tool = McpTool::new(client.peer().clone(), definition)?;
let assistant = agent("").with_tools(vec![Arc::new(tool)]);
```

Use `with_approval` to apply an application policy to the tool call. `annotations()` returns the hints published by the MCP server.

For Streamable HTTP, enable `transport-streamable-http-client-reqwest` on the application’s `rmcp` dependency and connect with `StreamableHttpClientTransport`. Reuse the connected peer across Agent calls when they share the same MCP session.

To expose an application-owned MCP server tool directly to an Agent, pass its definition and async handler to `McpServerTool::new`. The handler receives `CallToolRequestParams` and returns `AiResult<CallToolResult>`. The Agent uses the tool’s declared name and can apply the same approval policy.

### Skills {#agent-skills}

Use `Skill::from_directory` to open a directory with `SKILL.md`. YAML metadata supplies the skill name and description, and the Markdown body supplies the instructions. Use `Skill::new` to define a skill in Rust and add any inline resources to its `files`.

Use `with_skills` for an anonymous Agent or implement `Agent::skills` for a named Agent. Sources can be a `Skill`, a directory of skill folders, or a resolver. A resolver runs when a real call prepares its tools. When the Agent also registers a `LoadSkill`, OpenRust merges its declared skills into that tool.

The model calls `load_skill` with `name` to read the instructions and file list. Add a relative `path` to read a bundled UTF-8 file up to 256 KiB. The tool's prompts use the application locale.

Use `LoadSkill::new` to register resolved skills directly. `LoadSkill::from_sources([])` discovers skills under the application's `resources/skills` directory.

```rust
use openrust_ai::{Skill, agent};

let skill = Skill::from_directory("resources/skills/catalog")?;
let assistant = agent("").with_skills(skill);
```

### Tool approval policy {#tool-approval}

Implement `Tool::should_request_approval` to request approval for selected arguments. When the invocation has no resumable approval context, the SDK returns `AiError::ApprovalNotResumable` before executing any tool in the step. The buffered error carries the pending call IDs, tool names, arguments, and reasons.

```rust
use openrust_ai::{Approval, ToolRequest};

fn should_request_approval(&self, request: &ToolRequest<'_>) -> Option<Approval> {
    let amount = request.get("amount")?.as_u64()?;
    (amount > 100).then(|| Approval::new(Some("Confirm this amount.".into())))
}
```

### Resume a conversation after tool approval {#resume-tool-approval}

Publish the AI migrations with `./artisan vendor:publish --tag=ai-migrations`, rebuild the application to register the migration files, and run `./artisan migrate --force`. An agent that implements `RemembersConversations` can use `prompt_with_approvals` to pause before running any tool in the current batch.

```rust
use openrust_ai::{ApprovalResponse, Decision, RemembersConversations};

let result = agent
    .for_participant(participant.clone())
    .prompt_with_approvals(prompt, Default::default())
    .await?;

if let ApprovalResponse::Pending(pending) = result.response {
    let conversation_id = result.conversation_id;
    let approval_id = pending.id;
    // Resume after the authenticated participant submits their decisions.
    let _result = agent
        .r#continue(conversation_id, participant)
        .resume_approval(&approval_id, Decision::approve_all())
        .await?;
}
```

After approval completes, `ApprovalResponse::Completed(response)` contains a boxed `AgentResponse`. Read `response.steps` from it.

Keep the conversation ID and approval ID. For the same authenticated participant and agent, use `pending_approvals_for` to reload the requests before showing them to the user. Call `resume_approval` with a decision for each request. `Decisions::new` accepts per-call approval, rejection or edited arguments and can apply a default to the remaining requests.

Rejecting with `Decision::reject(None)`, boolean `false`, or a blank result ends the turn after saving the approval decisions and completed tool results. The resumed call incurs zero model usage. Pass a nonempty result if the model should continue based on the rejection, for example `Decision::reject(Some("The user requested the original amount.".into()))`. Tools explicitly approved in the same batch still run.

`ToolResult.denied` marks a rejected approval. `ToolResult.failed` marks an execution failure. `successful()` checks both flags and `error()` returns the result text for an unsuccessful call. Vercel streams emit `tool-output-denied` for a rejection. AG-UI reports it in `metadata.denied` on the tool result. To construct a tool result, use `ToolResult::new(id, name, result)` and set the applicable flag.

`ToolResult.arguments` contains the arguments used for the call, including any approved edits. For rejected calls it contains the proposed arguments. Old records without this field deserialize with an empty map.

A Responses function call with distinct item and call IDs exposes the item ID as `ToolCall.id` and the call ID as `result_id`. Submit approval decisions using the ID returned in the approval request. Provider replies and history replay use `result_id` when present.

When a turn pauses for approval, history immediately shows its user message and a `paused` assistant message. After approval, the SDK saves the tool results on that assistant message. The message keeps its ID through further pauses, completion, or failure. The original user message retains its attachment references.

Resuming approval saves the step’s tool results before the next model request. Call `approval_results_for` with the same participant, agent, conversation, and approval ID to read the saved results while the turn is active. After completion, read them from conversation history. If the resuming process exits before it finishes, the saved results remain available and duplicate approval calls are rejected to prevent repeating tools.

With `Decision::edit`, the tool runs with the approved arguments. The approval checkpoint saves the edit with its result, and the response and conversation history retain the executed arguments. When replay requires native reasoning blocks for the same connection and model, the SDK preserves the original blocks and supplies the actual tool results. Generic replay uses the edited arguments.

The SDK verifies participant, agent and provider connection before resuming. Only one request can claim a pending batch. The current approval window is 24 hours. A failed or interrupted request after claiming approval may already have executed a tool. The SDK does not automatically replay it. If the next model call fails after the tool ran, the SDK saves the completed tool results to the failed turn and settles the approval. Approval checkpoints preserve current and historical pictures and documents through private file references. Resuming verifies the participant, Agent and connection before loading the files.

For streamed text, start with `stream_with_approvals` and pass the result through your usual SSE response adapter. The SDK saves the checkpoint before sending the approval event. The default `tool_approval_request` event carries `conversation_id` and `approval_id`. Vercel sends the batch IDs in `data-tool-approval` and one `tool-approval-request` per call. AG-UI sends the IDs in each interrupt's metadata. Submit the decisions with `resume_approval` to continue the conversation and receive the buffered result.

To continue with streaming, call `resume_approval_stream(&approval_id, decisions)` and return the result through your SSE adapter. The stream sends the saved tool results first, then delivers model deltas. If another step requires approval, it uses the same approval ID. The final frame follows successful conversation persistence, which retains steps and usage from before and after the pause.

When your application renders an approval error, its display message follows the current locale. Override individual messages in `lang/vendor/ai/<locale>/errors.json` under `approval`. Match the `AiError` variant when choosing an HTTP status or handling a retry.

Listen to `openrust_ai::events::ToolApprovalRequested::NAME` for a stored approval request and `ToolApprovalResolved::NAME` for saved resolution results. The typed events carry the conversation, participant, agent type, invocation and approval identifiers. The resolved event carries the tool results for that approval batch.

## Queued agents {#queued-agents}

Implement `QueueableAgent` on a named Agent that derives `Serialize` and `Deserialize`. Give it a stable `QUEUE_JOB` name. Register it in the application service provider’s `boot` method with `TicketAgent::register_queue(queue.as_ref())`, where `queue` is the application’s `QueueManager`.

```rust
use openrust_ai::Agent;

let delivery = agent.queue("Summarize the latest conversation")
    .on_connection("database")
    .on_queue("ai")
    .then("persist")
    .catch("record_failure")
    .dispatch()
    .await?;
```

Define `queued_then` and `queued_catch` to resolve callback names to typed functions. A success callback receives the restored Agent and its `AgentResponse`. A failure callback receives the Agent and the terminal queue error. The queue uses its normal retries and backoff. Make callback side effects idempotent because the queue may redeliver a job. Credentials come from the worker’s named provider configuration.

### Broadcast streamed events {#broadcast-agents}

Call `broadcast_now` to publish each event immediately, or `broadcast` to enqueue delivery through the framework. Both return a stream, so consume it or return `to_response()` to start delivery. Use a private channel for user data and authorize subscriptions with the application’s channel rules.

```rust
use openrust::broadcasting::PrivateChannel;
use openrust_ai::Agent;

let channel = PrivateChannel { name: format!("tickets.{ticket_id}") };
let stream = agent.broadcast_now("Summarize this ticket", [channel.into()]).await?;
let response = stream.to_response();
```

Use `broadcast_on_queue` to generate and publish the stream in a worker. Read `invocation_id()` before dispatch to correlate client events. Success callbacks receive the combined text, usage across all model steps, tool outcomes, and each step’s metadata. A terminal error emits `stream_failed` with the same invocation ID. The worker defaults to one attempt for a broadcast because subscribers may have already received some events.

An Agent can override `without_broadcasting` to exclude event types from delivery while keeping them in the HTTP stream. Immediate delivery failures are logged. The broadcaster’s payload limits still apply.

## Generate an Agent or tool {#generators}

```shell
./artisan make:agent CatalogAgent
./artisan make:agent Products/StockAgent --structured
./artisan make:tool LookupInventory
./artisan make:agent-middleware CheckInventory
./artisan vendor:publish --tag=ai-stubs
```

Agents and tools are generated under `app/ai/agents` and `app/ai/tools`. Declare the generated modules in their parent Rust modules and recompile. Add `--force` to replace an existing generated file. Use the published files in `stubs/` to customize the templates.

Declare reusable middleware with `Agent::middleware`, or pass it to an anonymous Agent using `with_middleware`. For each generation step, `PendingStep` exposes the selected model, step number, previous steps and usage. Call `next.run(step)` to continue. Use `with_model`, `with_instructions` and `only_tools` to change that step.

Implement `handle` for buffered calls and `handle_stream` for streaming calls. The generated middleware stub includes both. In `handle_stream`, `NextStreamStep::run` returns a `StepResult` that keeps the upstream stream live. Use `StepResult::then` to inspect the terminal response or `StepResult::from_response` to supply a response from middleware. A middleware that defines only `handle` returns `StreamingMiddlewareNotSupported` on a streaming call.

Call `result.response().await` to inspect a complete step before returning it. This buffers that step and preserves its events for the caller. Return `StepResult::stop()` before `next.run` to end the stream with a cancellation status. The response retains the steps and usage already produced.

Use `with_middleware(vec![Arc::new(Stop::when(|step: &PendingStep<'_>| !step.is_first_step()))])` to stop an Agent after its first model request. Import `Stop` and `PendingStep` from `openrust_ai`. `Stop::new`, `Stop::when`, and `Stop::unless` accept a boolean or a step predicate. `unless` stops when its condition is false.

Stopping skips the pending model request. Stopping before the first step leaves the steps and usage empty and skips conversation persistence. Stopping later preserves completed tool results and usage. A stream ends with reason `cancelled`. When a stop leaves structured generation without a final object, the buffered result has an empty `structured` map and retains any completed steps. Custom buffered middleware can return `StepResponse::stop()` before calling `next.run`.

### Testing Agents {#agent-fake}

Use `SupportAgent::fake` to supply an ordered list of strings, or `SupportAgent::fake_with` to return a result based on the `RecordedPrompt`. A callback can return text, a JSON object, or a complete `AgentResponse` using `.into()`. The Fake records each invocation once for `assert_prompted` and `assert_prompted_times`, including callbacks that return an error. The callback runs for each model step and sees the model selected by middleware plus any preceding tool results in `prompt.steps`. A fake model response that requests a registered tool goes through the normal tool handler. A streamed response keeps the supplied model, usage, public reasoning, citations, and tool outcomes.

Use `SupportAgent::fake_responses` for an ordered list that mixes text, JSON objects, full responses, and `ToolCall` values. An empty or exhausted sequence supplies a sample for a structured prompt. The sample follows declared properties, enum values, defaults, sizes, and numeric bounds. Enable `prevent_stray_prompts(true)` to require a supplied result for each model step.

Use `SupportAgent::assert_queued`, `assert_not_queued`, and `assert_never_queued` to check dispatched prompts. Queued prompts and executed prompts have separate records. A separately started Worker uses its own application’s Fake registration.

## Embeddings {#embeddings}

Set `AI_EMBEDDINGS_PROVIDER=openai` and `OPENAI_EMBEDDING_MODEL` to the model ID from your provider. The SDK sends text inputs as a batch and orders the vectors by the original input index.

```rust
use openrust_ai::Embeddings;

let response = Embeddings::r#for(["Shipping", "Returns"])?
    .generate()
    .await?;
let vectors = response.embeddings;
```

Use `dimensions(n)` when the selected model supports explicit dimensions. For an OpenAI-compatible connection, configure its `models.embeddings` before using it for embeddings. To publish a vector model through `POST /v1/embeddings`, map its public name and set `"endpoints": ["embeddings"]` on that mapping. Grant the alias to the client Key as with a text model.

Select `cohere`, `jina`, or `voyageai` in `AI_EMBEDDINGS_PROVIDER` to use one of the named retrieval connections. Each shares its API key and base URL with reranking.

| Connection | Embedding model |
| --- | --- |
| `cohere` | `COHERE_EMBEDDING_MODEL` |
| `jina` | `JINA_EMBEDDING_MODEL` |
| `voyageai` | `VOYAGE_EMBEDDING_MODEL` |

Cohere uses its native float embeddings and billed input tokens. Jina uses `retrieval.passage`. Voyage uses the native text API and routes text for `voyage-multimodal-3` and `voyage-multimodal-3.5` to its multimodal endpoint. The original `voyage-multimodal-3` returns 1024 dimensions. For Cohere, requested dimensions are checked against the model output.

For Gemini’s native API, set `AI_EMBEDDINGS_PROVIDER=gemini` and `GEMINI_EMBEDDING_MODEL`. The SDK uses `batchEmbedContents` for text, including one-item batches, and accepts the model ID with or without the `models/` prefix. It preserves input order, passes the requested dimensions and reports input tokens when Gemini supplies them.

For Groq, set `GROQ_API_KEY`, `AI_PROVIDER=groq` and `GROQ_MODEL`. To override the default `https://api.groq.com/openai/v1`, set `GROQ_URL`. The driver supports Chat Completions, SSE, local tool rounds and image input on capable models. A buffered structured request without tools uses native JSON Schema. With tools or streaming, the SDK embeds the schema in the prompt and checks that the final result is a JSON object. For audio transcription, set `AI_TRANSCRIPTION_PROVIDER=groq` and `GROQ_TRANSCRIPTION_MODEL`. Groq returns text, timestamps and reported duration. For speaker diarization, select another transcription connection.

For Amazon Bedrock Converse, set `AI_PROVIDER=bedrock`, `BEDROCK_MODEL`, and your AWS region in `AWS_REGION` or `AWS_DEFAULT_REGION`. You can override the runtime endpoint with `BEDROCK_URL`. Use `AWS_BEARER_TOKEN_BEDROCK`, explicit AWS credentials with an optional `AWS_SESSION_TOKEN`, or an AWS credential chain. The SDK supports ordinary text, conversation history, and inline PNG, JPEG, WebP, PDF, plain text, and Markdown. Set `BEDROCK_MODEL_CHEAPEST` or `BEDROCK_MODEL_SMARTEST` to select a separate model tier. For an assumed role, set `assume_role.arn` on the named connection. Optional settings are `session_name`, `duration_seconds`, and `external_id` inside `assume_role`. The AWS SDK obtains and refreshes the resulting credentials.

Use `Agent::stream` for Bedrock ConverseStream. The SDK reads the native AWS event stream and delivers text and visible reasoning as SSE. It reports the finish reason and token usage from the final metadata. A truncated stream or invalid AWS event produces a stream error, and the configured request timeout also applies while reading events.

Bedrock supports local functions configured with `with_tools` in both Converse and ConverseStream. The SDK retains the original tool IDs and replays provider-signed reasoning with the tool result in the next step. Set `ToolChoice::Required` or `ToolChoice::tool` only on models that support the corresponding Bedrock tool choice. Streamed tools execute after the complete stop and usage events arrive.

Bedrock supports `with_schema` for ordinary and streamed calls. The SDK sends the schema in a dedicated tool definition and treats its arguments as the final JSON response. With local functions, it allows them on earlier steps and selects the schema tool on the final step. For models in the pinned Laravel list that reject forced tool selection, it sends the schema as a system instruction and checks that the final text decodes to a JSON object.

For Bedrock text embeddings, set `AI_EMBEDDINGS_PROVIDER=bedrock` and `BEDROCK_EMBEDDING_MODEL=amazon.titan-embed-text-v2:0`, then use `Embeddings::for` (`Embeddings::r#for` in Rust). Titan sends one input per request and defaults to 1024 dimensions. Use `.dimensions(256)` or `.dimensions(512)` with Titan V2 when your vector index requires that size. Cohere Embed models send the inputs in one request as `search_document` and use the model's native vector size.

For Bedrock reranking, set `AI_RERANKING_PROVIDER=bedrock` and `BEDROCK_RERANKING_MODEL=cohere.rerank-v3-5:0`. Call `Reranking::of(documents)?.limit(3).rerank(query).await?`. Results contain the zero-based index and relevance score in the order returned by the provider. To return the original records in ranked order, use `Reranks` on a `Collection`.

For xAI text, set `AI_PROVIDER=xai`, `XAI_API_KEY`, and `XAI_MODEL`. Override the default `https://api.x.ai/v1` with `XAI_URL`. Use `XAI_MODEL_CHEAPEST` and `XAI_MODEL_SMARTEST` for model tiers. The connection uses native Responses with SSE, structured output, local function calls, and image input on supported models. Local tool rounds carry encrypted reasoning in the request when the provider returns it. Run `./artisan ai:models xai` to see discovered and configured models.

With the native xAI connection, SDK usage and the gateway ledger count reasoning tokens once in the output total and keep the reasoning count available separately. If xAI also reports a total, the accounting checks whether the output count already includes reasoning. Native gateway responses retain the upstream usage fields on the wire.

Native xAI reports actual charges in USD ticks. Read `response.usage.cost_in_usd_ticks` for text or `response.usage.tokens.cost_in_usd_ticks` for images. One US dollar equals 10¹⁰ ticks. A buffered multi-step Agent response totals the charges when every step reports one. With gateway billing enabled, `./artisan ai:gateway:usage:list --billing` shows final native Responses charges in `reported_cost_usd`, alongside the configured rate calculation and downstream receivable. A conflicting invoice appears as `actual.state=conflicting_evidence`. Apply the latest `ai-gateway-migrations` to existing installations.

For provider-side tools on xAI, pass `ProviderTool::options` to `with_provider_options` for the connection. Available choices are `WebSearch` with up to five allowed domains, `CodeExecution` in the provider sandbox, and `FileSearch` over existing xAI collections. Use the collection IDs as `vector_store_ids`. The provider executes these tools, and the same agent can also run local functions configured through `with_tools`.

```rust
use openrust_ai::{ProviderTool, agent};

let assistant = agent("").with_provider_options(
    "xai",
    ProviderTool::options([
        ProviderTool::WebSearch {
            allowed_domains: vec!["example.com".into()],
        },
        ProviderTool::FileSearch {
            vector_store_ids: vec!["collection-id".into()],
            max_num_results: Some(10),
        },
    ]),
);
```


For Mistral’s native API, set `MISTRAL_API_KEY`, `AI_PROVIDER=mistral` and `MISTRAL_MODEL`. Override the base URL `https://api.mistral.ai/v1` with `MISTRAL_URL`. List models with `./artisan ai:models mistral`. Native chat supports buffered and SSE responses, structured output, tool rounds, images and document parts. The SDK separates reasoning chunks from answer text. For embeddings, set `AI_EMBEDDINGS_PROVIDER=mistral` and `MISTRAL_EMBEDDING_MODEL`. The SDK requests the model’s native vector size and validates any dimensions you specify.

For the native Ollama SDK connection, set `AI_PROVIDER=ollama` and `OLLAMA_MODEL`. The default is `http://127.0.0.1:11434`. Override it with `OLLAMA_URL` and set `OLLAMA_API_KEY` for an authenticated proxy. The catalog uses `/api/tags`. Chat, structured output, and tool rounds use `/api/chat`, with both buffered and native NDJSON streaming responses. The SDK emits text and reasoning incrementally and executes tools only after the upstream sends a complete step. Set `AI_EMBEDDINGS_PROVIDER=ollama` and `OLLAMA_EMBEDDING_MODEL` to use `/api/embed`.

To publish a native Gemini embedding model, set its mapping to the Gemini connection and specify `"endpoints": ["embedContent", "batchEmbedContents"]`. The authorized model appears under `/v1beta/models`. Send Google-format requests to `POST /v1beta/models/{alias}:embedContent` or `POST /v1beta/models/{alias}:batchEmbedContents` with `x-goog-api-key`. When a request includes a model name in its body, it must match the published alias. The gateway substitutes the configured upstream model and accepts inline input.

Call `cache(Some(600), None)` to cache embeddings for 600 seconds. The configuration under `ai.caching.embeddings` defines the default lifetime, the cache store and whether to enable caching for all SDK calls. Individual caching reuses each vector across batches and sends only misses to the provider. Pass `Some(false)` as the second argument to cache the full, ordered batch. Use `cache(Some(0), None)` to bypass caching for an individual call. Usage on a partial hit reflects the inputs sent upstream.

### Search vectors using text {#querying-embeddings}

Import `ToEmbeddings` to generate one vector from a string. The query builder and Eloquent can also embed search text through the configured SDK connection. For PostgreSQL, enable pgvector and use a vector column with the same dimensions as the embedding model.

```rust
use openrust::{database::Db, support::Str};
use openrust_ai::ToEmbeddings;

let embedding = Str::of("return policy").to_embeddings().await?;
let documents = Db::table("documents")?
    .where_vector_similar_to_text("embedding", "return policy", 0.8, true)
    .await?
    .get()
    .await?;
```

Both paths use the cache settings under `ai.caching.embeddings`. An application that registers its own database embedding provider can keep using it.

### Testing embeddings {#embeddings-fake}

Register `Embeddings::fake` with one vector batch per expected generation. Use `fake_with` to build the vectors from the recorded request. A Fake with an empty sequence generates unit vectors when you specify a dimension. Turn on `prevent_stray_embeddings(true)` to require a scripted response for every call.

```rust
let fake = Embeddings::fake([vec![vec![0.5, 0.2]]])?;
fake.prevent_stray_embeddings(true);
let result = Embeddings::r#for(["Shipping"])?.dimensions(2).generate().await?;
Embeddings::assert_generated(|call| call.inputs == ["Shipping"]);
```

Faked calls are recorded by the SDK and do not contact the provider or write to the configured embeddings cache.

## Reranking {#reranking}

Set `AI_RERANKING_PROVIDER` to choose the default connection, then fill in its API key and the reranking model ID.

| Connection | Base URL | URL override | API Key | Model ID |
| --- | --- | --- | --- | --- |
| `cohere` | `https://api.cohere.com/v2` | `COHERE_URL` | `COHERE_API_KEY` | `COHERE_RERANK_MODEL` |
| `jina` | `https://api.jina.ai/v1` | `JINA_URL` | `JINA_API_KEY` | `JINA_RERANK_MODEL` |
| `voyageai` | `https://api.voyageai.com/v1` | `VOYAGE_URL` | `VOYAGE_API_KEY` | `VOYAGE_RERANK_MODEL` |
| `openrouter` | `https://openrouter.ai/api/v1` | `OPENROUTER_URL` | `OPENROUTER_API_KEY` | `OPENROUTER_RERANK_MODEL` |

```rust
use openrust_ai::Reranking;

let result = Reranking::of(["Shipping", "Exchanges", "Returns"])?
    .limit(2)
    .rerank("How do I return an item?")
    .await?;
```

Results keep the provider’s ranking order. Each `index` points to the document’s original position, so you can select the ranked items from the input collection.

Import `Reranks` to call `collection.rerank(|item| item.title.clone(), query).await?`. The result contains the original items in ranked order. Use `rerank_with` and `RerankingOptions` to change the result limit, timeout, connection, or model.

Use `Reranking::fake([])?` to generate a fake ranking without an outbound request. Supply scripted results or use `Reranking::fake_with` for a callback. Assert calls with `assert_reranked`, `assert_not_reranked`, or `assert_nothing_reranked`. Call `prevent_stray_rerankings(true)` on the fake to reject unconfigured responses.

## Image generation {#images}

Select a connection and an image model. For OpenAI, use the existing `OPENAI_API_KEY` and optional `OPENAI_URL` override.

```dotenv
AI_IMAGE_PROVIDER=openai
OPENAI_IMAGE_MODEL=gpt-image-1.5
```

```rust
use openrust_ai::Image;

let response = Image::of("A watercolor pear")?.square().quality("high").generate().await?;
let path = response.store_as("generated", "pear.png", None).await?;
```

Use `square()`, `portrait()` or `landscape()` for standard sizes. Use `size()`, `quality()` or `timeout()` to set request options. To edit an image, pass `ImageAttachment::from_bytes` or load one from a Storage disk with `ImageAttachment::from_storage`, then call `attachments()` before `generate()`.

OpenAI and OpenAI-compatible connections that implement the Images API can return inline PNG, JPEG, or WebP. Add `models.images` to enable this capability on a compatible connection. Read MIME type and bytes through `first_image()`, or save them with `store` and `store_as`. Use `store_publicly` or `store_publicly_as` to request public visibility from the selected disk.

For Gemini image generation and edits, set `AI_IMAGE_PROVIDER=gemini` and `GEMINI_IMAGE_MODEL` to an image-capable model. The connection uses `GEMINI_API_KEY` and accepts a `GEMINI_URL` override. The SDK calls the Interactions API. Set an aspect ratio with `size("16:9")`, or use `square()`, `portrait()` or `landscape()`. Quality values `low`, `medium` and `high` map to `1K`, `2K` and `4K`, subject to model support. The response includes each returned image and any reported image token counts.

For xAI, set `AI_IMAGE_PROVIDER=xai`, `XAI_API_KEY`, and `XAI_IMAGE_MODEL`. The connection accepts a `XAI_URL` override. Use the same `Image` entry point for generation and edits with up to five reference images. Set an aspect ratio with `size("16:9")`. Quality values `low`, `medium`, and `high` request `1k`, `2k`, and `2k` resolution respectively.

Use `Image::fake([])?` to return placeholder bytes, or supply an `ImageResponse` with valid images when a test reads or stores the result. Use `Image::fake_with` to build the response from the recorded prompt, reference images, size, quality and selected connection. Call `assert_generated` or `assert_queued` for the corresponding step. Enable `prevent_stray_images(true)` to catch an exhausted response sequence.

To queue a generation or edit, use `queue()` or `queue_with(provider, model)`, configure the job and call `dispatch().await`. Load queued reference images with `ImageAttachment::from_storage` or `from_path` so the Worker can reopen them. The queue stores file references in attachment order. Register named callbacks during application boot with `Image::register_queue_then` and `Image::register_queue_catch`.

## Speech and transcription {#audio}

Choose a connection and model for each capability. OpenAI uses `OPENAI_API_KEY` and supports the optional `OPENAI_URL` override.

```dotenv
AI_AUDIO_PROVIDER=openai
OPENAI_AUDIO_MODEL=gpt-4o-mini-tts
AI_TRANSCRIPTION_PROVIDER=openai
OPENAI_TRANSCRIPTION_MODEL=gpt-4o-transcribe
```

```rust
use openrust_ai::{Audio, AudioFormat, Transcription};

let speech = Audio::of("Read the notice")?.format(AudioFormat::Wav).generate().await?;
let path = speech.store_as("audio", "notice.wav", None).await?;
let transcript = Transcription::from_storage(&path, None).await?.language("en").generate().await?;
```

Use `Audio::of(text)` or the `ToAudio` extension on a string. Set the voice with `voice()`, `male()` or `female()` and select MP3, WAV, Opus, AAC, FLAC or PCM output. Save the bytes to the configured Storage disk with `store` or `store_as`.

Use `Transcription::from_storage` for recordings on a disk, or `from_path`, `from_upload` or `from_bytes` for local input. Call `language()` when the language is known. To get speaker labels and timed segments, enable `diarize(true)` and select a model that supports it. Responses include the text and upstream usage when reported. OpenAI-compatible connections can enable either capability with `models.audio` or `models.transcription`.

Mistral uses the same `Audio` and `Transcription` entry points. Set `MISTRAL_API_KEY` and optionally override the connection with `MISTRAL_URL`.

```dotenv
AI_AUDIO_PROVIDER=mistral
MISTRAL_AUDIO_MODEL=voxtral-mini-tts-3-0
AI_TRANSCRIPTION_PROVIDER=mistral
MISTRAL_TRANSCRIPTION_MODEL=voxtral-mini-transcribe-3-0
```

Speech defaults to MP3 and also supports WAV, Opus, FLAC and PCM. `male()` selects `en_paul_neutral`, while the default and `female()` select `gb_jane_neutral`. Use `voice()` for a Mistral voice ID. The Mistral driver follows Laravel’s mapping and takes speech direction from the input text. `diarize(true)` requests speaker segments and takes precedence over `language()`.

Gemini uses the native Interactions API for speech and transcription. Set `AI_AUDIO_PROVIDER=gemini` with `GEMINI_AUDIO_MODEL`, or `AI_TRANSCRIPTION_PROVIDER=gemini` with `GEMINI_TRANSCRIPTION_MODEL`. Both use `GEMINI_API_KEY` and accept a `GEMINI_URL` override. Speech defaults to WAV and also accepts explicit PCM. The SDK wraps legacy PCM replies as WAV when needed. Transcription models can return speaker turns assembled from word timestamps. General-purpose Gemini models can return plain text or timed segments through the same SDK.

For ElevenLabs, set `ELEVENLABS_API_KEY` and select the `eleven` connection with `AI_AUDIO_PROVIDER` or `AI_TRANSCRIPTION_PROVIDER`. Choose the model with `ELEVENLABS_AUDIO_MODEL` or `ELEVENLABS_TRANSCRIPTION_MODEL`. Use `ELEVENLABS_URL` to override the default `https://api.elevenlabs.io/v1`. List models with `./artisan ai:models eleven`. Speech supports MP3, WAV, Opus and PCM. Call `voice()` with an ElevenLabs voice ID to override the default. With diarization enabled, transcription returns timestamped words and the speaker IDs reported by the provider.

In application tests, install `Audio::fake([])?` or `Transcription::fake(["Recorded notice".into()])?` inside the application scope. Use `fake_with` for a request-based callback. The default speech fixture is placeholder bytes. Supply an `AudioResponse::from_bytes` fixture when the test consumes or transcribes the generated audio. Assert calls with `assert_generated`, `assert_not_generated` or `assert_nothing_generated`. Enable `prevent_stray_audio(true)` or `prevent_stray_transcriptions(true)` on the returned gateway to catch exhausted response sequences.

### Queue a media request {#queued-audio}

Both builders expose `queue()` and `queue_with(provider, model)`. Configure the queue and retry options before calling `dispatch().await`. For queued transcription, select an existing file with `Transcription::from_storage` or `from_path`. Keep the named disk and object accessible to the Worker, or use a local path shared with the Worker. The payload stores the file reference, and the Worker resolves provider credentials when it runs.

```rust
use openrust_ai::Transcription;

Transcription::from_storage("recordings/notice.mp3", Some("local"))
    .await?
    .diarize(true)
    .queue()?
    .on_queue("media")
    .then("save_transcript")
    .dispatch()
    .await?;
```

Register named `then` and `catch` callbacks during application boot with `Audio::register_queue_then`, `Audio::register_queue_catch` or the corresponding `Transcription` methods. Use the same registration in producers and Workers. The terminal failure callback runs after the configured retries. In tests, use `assert_queued`, `assert_not_queued` and `assert_nothing_queued` to inspect dispatches separately from generated responses.

## Classification {#classification}

Set `AI_CLASSIFICATION_PROVIDER` to the connection name and set its default model to an ID that supports the provider's Decisions API.

| Connection | Default model environment variable |
| --- | --- |
| `openai` | `OPENAI_CLASSIFICATION_MODEL` |
| `typesafe` | `TYPESAFE_CLASSIFICATION_MODEL` |
| `openrouter` | `OPENROUTER_CLASSIFICATION_MODEL` |
| `gemini` | `GEMINI_CLASSIFICATION_MODEL` |

Each preset uses its own credentials and URL settings. TypeSafe uses `TYPESAFE_API_KEY` and `TYPESAFE_URL`. Custom connections can use the `system-one` driver, or set `models.classification.protocol = "openrouter-decisions"` on an OpenAI-compatible connection to send classification requests to OpenRouter’s `/api/alpha/decisions`. A compatible connection that implements OpenAI Decisions can enable it by setting its default model in `models.classification`.

```rust
use openrust_ai::{Classification, classification::{Boolean, Choice}};

let result = Classification::of("The item arrived damaged")?
    .question("eligible", Boolean::new("Is this a return request?"))
    .question("category", Choice::new("Choose a category", ["return", "other"]))
    .classify()
    .await?;

let category = result.answer("category");
```

Classification accepts text, a JSON object or a JSON array. Add keyed `Boolean`, `Choice` or `Score` questions. You can supply inline images with `attachments()`. Responses expose typed answers, explicit refusals, and the token counts reported by the provider.

TypeSafe accepts PNG and JPEG attachments. When the state is a JSON object, TypeSafe reserves `images` for the attachment references. The OpenRouter and Gemini connections also accept WebP. Availability depends on the selected model and account.

For a yes-or-no decision, import `Decides` for strings and `Stringable`, or `DecidesText` for `Str::decide`. Import `DecidesBetween` to select an original item from a collection. Use `decide_by` to supply each item’s label and description, or `decide_key` to select a key from a key-to-description collection. `DecideOptions` can specify a probability threshold, provider, model, and timeout. A choice below the threshold returns `None`.

```rust
use openrust::support::{Collection, Str};
use openrust_ai::{Decides, DecidesBetween};

let eligible = Str::of("The item arrived damaged")
    .decide("Is this a return request?")
    .await?;
let department = Collection::make(["sales", "support"]);
let assigned = department
    .decide("Which department should reply?", "My blender arrived damaged")
    .await?;
```

In tests, use `Classification::fake([])` for generated typed answers, supply a sequence of answer maps or complete responses, or use `fake_with` to compute answers from the request. Use `assert_classified`, `assert_not_classified`, and `assert_nothing_classified` to check calls. Enable `prevent_stray_classifications(true)` to fail after the supplied responses are exhausted.

## Provider files {#provider-files}

Set `AI_FILE_PROVIDER` to a named Files API connection, or use the default AI connection. OpenAI, Azure OpenAI, Anthropic, Gemini, and the global OpenRouter endpoint support files directly. Set `files: true` on a custom OpenAI-compatible connection if its endpoint exposes the same Files API.

```rust
use openrust_ai::{FileUpload, Files};

let document = FileUpload::from_path("policy.txt").await?
    .with_mime_type("text/plain")?;
let file = Files::put(document, None).await?;
let id = file.id();
let private_path = file.store_as("ai", "policy.txt", Some("local")).await?;
```

Use `FileUpload::from_upload` for an HTTP upload, `from_bytes` for in-memory data, or `Files::put_from_storage` for a configured disk. Change the upload filename, MIME type, or purpose on `FileUpload`. The default purpose is `user_data`. The Azure preset uses `assistants`. Anthropic and OpenRouter manage purpose on the server and accept the default setting.

`Files::get` retrieves metadata and `Files::list` supplies a cursor with each page. Handles retain their original connection for downloading, refreshing, and deleting. `store_as` writes the downloaded file to a private disk location. Before accepting a file ID from a user, check that the file belongs to that user in the application. If the provider reports `downloadable: false`, reading or storing the file returns `AiError::FileNotDownloadable`. Anthropic and OpenRouter apply this rule to user uploads.

Gemini returns a resource name such as `files/policy`. You can pass the resource name or its short ID to `Files::get` and `Files::delete`. Use `refresh()` to check for `ACTIVE` after upload, and pass the pagination token back unchanged. The Gemini adapter covers upload and metadata operations. Keep the uploaded original in Storage if the application needs to read it later.

When calling the shared gateway, send inline attachments using the endpoint’s native format. File IDs from an upstream account can be used through the SDK with the connection that created them.

In tests, call `Files::fake([])?` for in-memory uploads, lookups, and deletes. Pass a sequence of byte responses or use `fake_with` to generate bytes from the requested ID. Enable `prevent_stray_operations(true)` on the fake to reject unconfigured lookups. Use `assert_stored`, `assert_not_stored`, `assert_deleted`, and `assert_not_deleted` to check operations. `Files::fake_id("policy.txt")` returns the deterministic test ID.

## Vector stores {#vector-stores}

Set `AI_STORE_PROVIDER` to the named connection for vector stores, or use the default AI connection. OpenAI, Azure OpenAI, and Gemini File Search support them directly. Set `stores: true` for a custom OpenAI-compatible vector stores endpoint and `files: true` when that same connection also accepts file uploads.

```rust
use openrust_ai::{Files, Stores};

let store = Stores::create("Support policies", None).await?;
let file = Files::get("file-policy", Some(store.provider())).await?;
let member = store.add(&file, &Default::default()).await?;
let ready = store.refresh().await?.is_ready();
```

Use `StoreOptions` to set trusted initial file IDs. OpenAI also accepts a description and an idle expiry in days. `refresh()` reports indexing counts and readiness. Use `Stores::list` and `Store::files` for cursor pagination.

The store and file must come from the same connection. `Store::remove(&member, false)` detaches a member while retaining the source file. Passing `true` also deletes that source. If deletion fails after the detach, `DetachedFileDeletionFailed` reports the original file ID so it can be handled separately. Use `add_file_id` only with IDs already authorized by your application.

When using Gemini, pass a resource name such as `files/policy` as the source file ID. Imports wait for the indexing operation and return a separate document ID. `StoreFile::file_id()` returns `Some` when the source file ID is known. Gemini document listings omit that ID, so call `remove(&member, false)` to remove a listed document. If an initial import fails after creating a store, `StorePopulationFailed` includes the created store ID.

Use `Stores::fake([])?` to test store creation, membership, and deletion in memory. It also installs the File Fake, so a test can upload a file, add it to a store, and assert whether it was deleted. Use `Stores::fake_with_files([], false)?` to keep the current file provider. Supply a sequence of retrieval names or use `fake_with` for a callback. Use `assert_created`, `assert_not_created`, `assert_deleted`, and `assert_not_deleted` to check store operations.
