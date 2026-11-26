# AI SDK

## Support status {#support-status}

The optional `ai` feature provides Laravel-style `Ai`, `AiManager`, `Agent`, and a text fake. The SDK supports text and text SSE via OpenAI Responses and OpenAI-compatible Chat Completions.

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

Use `agent(instructions).with_messages(history)` to include earlier turns. Build the history in order with `Message::user(text)` and `Message::assistant(text)`. Both `prompt` and `stream` send those turns before the latest prompt. Artisan accepts the same history as a JSON array of `role` and `content` entries in `--messages`.

In an HTTP route, return the default SDK SSE events with `agent(instructions).stream(prompt).await?.to_response()`. Use `into_stream()` to consume the events directly.

Call `using_vercel_data_protocol(message_id)` before `to_response()` to return Vercel AI SDK UI text events. Call `using_agent_user_interaction_protocol(thread_id, run_id)` for AG-UI text events. Pass `None` to let the SDK provide IDs. Both formats include the end state and reported token usage. Use `using_protocol` to provide a custom renderer for the same Agent events.

Use `agent(instructions).with_schema(schema).prompt(text)` to request a JSON object. Build the schema with the framework’s `JsonSchema` builder. Read members from `response.structured` or use `response.decode::<YourType>()`. A model must support the connection’s native structured-output format. The structured stream emits the generated JSON as text deltas. To try a schema from Artisan, pass a JSON object schema to `./artisan ai:prompt` with `--schema`.

For a connection needed only during the current request or command, use `Ai::build(config)` with the same fields as a named provider. Use the resulting provider's `name()` in a prompt or look it up with `Ai::text_provider` during that scope. Names reserved by the SDK or configured in the application are unavailable for temporary connections. `Ai::flush_state()` clears the current scope’s temporary connections.

## Scope {#scope}

Responses provide text, the connection, the model, and input and output token counts when reported by the upstream service. Native Anthropic and Gemini connections also support text, SSE, and paginated model discovery. Persistent conversations are pending.

## Model gateway {#model-gateway}

Add `ai-gateway` to `[features].default` in your application’s `Cargo.toml` and set `AI_GATEWAY_ENABLED=true`. Run `./artisan vendor:publish --tag=ai-gateway-config`. Update `models` and `keys` in `config/ai_gateway.rs` with your public model mappings and client permissions, then rebuild the application.

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

Set a separate randomly generated secret for `AI_GATEWAY_SUPPORT_KEY` in the application environment. The upstream connection uses `DEEPSEEK_API_KEY`. Set the external client’s base URL to `https://your-app.example/v1`, use the client Key as a Bearer credential, and select `support-chat`. `GET /v1/models` lists published models available to that Key. The optional `requests_per_minute` uses the application's configured rate-limit cache.

`POST /v1/chat/completions` supports OpenAI and compatible connections. `POST /v1/responses` can route natively to OpenAI connections or translate text requests to Anthropic Messages. Both support JSON and SSE. Responses runs in stateless mode with `store` set to `false`. It rejects upstream conversations, previous response IDs, and server-side tools. The `ai_gateway.call` structured log records outcomes and upstream-reported token counts. Set `usage_channel` to select a logging channel that accepts `info` records. Unreported token counts are recorded as `null`.

For an Anthropic client, set the base URL to your application domain and send the client Key in `x-api-key`. The gateway accepts `/v1/messages` and `/v1/messages/count_tokens` for models mapped to Anthropic connections. Plain-text Messages requests can also route to a published OpenAI Responses model. `/v1/models` and `/v1/models/{model}` use the Anthropic format with `x-api-key` and the OpenAI format with Bearer authentication.

Both conversion directions support buffered JSON and SSE, and use the published model name in client responses. When upstream reports cache reads or writes, the gateway includes those counts using the destination protocol’s usage fields.

The same published Anthropic model can serve Messages and Responses clients. The conversion supports text, system instructions, SSE, and basic sampling options. For structured output, tools, or attachments, choose a native endpoint supported by the model and client.

Gemini clients use the `/v1beta` API and send the client Key in `x-goog-api-key`. The gateway lists authorized Gemini models and accepts `:generateContent`, `:streamGenerateContent?alt=sse`, and `:countTokens` under `/v1beta/models/{model}`. In the SDK and Artisan, choose the `gemini` connection and enter its model ID with or without the `models/` prefix.

Add `fallbacks` to a public model mapping to try backup connections in order. Each entry specifies `provider` and `model`, for example `[{"provider":"openai","model":"backup-model"}]`. The gateway can retry stateless requests after eligible upstream failures, including 429 and 5xx responses. It keeps the same public model and records each attempt under one request ID. Requests with tools or stored output, and streams already sent to the client, are excluded from replay.

## Agent tools {#agent-tools}

Implement `Tool` with a name, description, JSON Schema, and asynchronous `handle` method. Use `ToolRequest::validate` to apply framework validation rules or `decode` to read arguments into a Rust type. The tool receives an ID for the provider call and a separate ID for its execution.

```rust
let response = agent("Check the catalog before answering.")
    .with_tools(vec![std::sync::Arc::new(LookupInventory)])
    .prompt("How many units are available?")
    .await?;
let answer = response.text;
```

Agent tools work with buffered Chat Completions, Responses, Anthropic Messages, and Gemini GenerateContent. Claude and Gemini receive their original model content again with the tool results so the provider can verify its signatures. The Agent sends results back with their call IDs and exposes `tool_calls` and `tool_results` on the response. Use `with_max_steps(n)` to limit model steps. Tool execution stops on the last step, and any calls proposed in that step appear as failed results.

## Generate an Agent or tool {#generators}

```shell
./artisan make:agent CatalogAgent
./artisan make:agent Products/StockAgent --structured
./artisan make:tool LookupInventory
./artisan vendor:publish --tag=ai-stubs
```

Agents and tools are generated under `app/ai/agents` and `app/ai/tools`. Declare the generated modules in their parent Rust modules and recompile. Add `--force` to replace an existing generated file. Use the published files in `stubs/` to customize the templates.

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
