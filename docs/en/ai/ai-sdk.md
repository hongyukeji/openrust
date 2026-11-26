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

Use `agent(instructions).with_messages(history)` to include earlier turns. Build the history in order with `Message::user(text)` and `Message::assistant(text)`. Both `prompt` and `stream` send those turns before the latest prompt. Artisan accepts the same history as a JSON array of `role` and `content` entries in `--messages`.

In an HTTP route, return the default SDK SSE events with `agent(instructions).stream(prompt).await?.to_response()`. Use `into_stream()` to consume the events directly.

Call `using_vercel_data_protocol(message_id)` before `to_response()` to return Vercel AI SDK UI text events. Call `using_agent_user_interaction_protocol(thread_id, run_id)` for AG-UI text events. Pass `None` to let the SDK provide IDs. Both formats include the end state and reported token usage. Use `using_protocol` to provide a custom renderer for the same Agent events.

Use `agent(instructions).with_schema(schema).prompt(text)` to request a JSON object. Build the schema with the framework’s `JsonSchema` builder. Read members from `response.structured` or use `response.decode::<YourType>()`. A model must support the connection’s native structured-output format. The structured stream emits the generated JSON as text deltas. To try a schema from Artisan, pass a JSON object schema to `./artisan ai:prompt` with `--schema`.

For a connection needed only during the current request or command, use `Ai::build(config)` with the same fields as a named provider. Use the resulting provider's `name()` in a prompt or look it up with `Ai::text_provider` during that scope. Names reserved by the SDK or configured in the application are unavailable for temporary connections. `Ai::flush_state()` clears the current scope’s temporary connections.

## Scope {#scope}

Responses provide text, the connection, the model, and input and output token counts when reported by the upstream service. Native Anthropic and Gemini connections also support text, SSE, and paginated model discovery.

## Model gateway {#model-gateway}

Add `ai-gateway` to `[features].default` in your application’s `Cargo.toml` and set `AI_GATEWAY_ENABLED=true`. Run `./artisan vendor:publish --tag=ai-gateway-config`. Update `models` and `keys` in `config/ai_gateway.rs` with your public model mappings and client permissions, then rebuild the application.

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

Set a separate randomly generated secret for `AI_GATEWAY_SUPPORT_KEY` in the application environment. The upstream connection uses `DEEPSEEK_API_KEY`. Set the external client’s base URL to `https://your-app.example/v1`, use the client Key as a Bearer credential, and select `support-chat`. `GET /v1/models` lists published models available to that Key. The optional `requests_per_minute` uses the application's configured rate-limit cache.

`POST /v1/chat/completions` supports OpenAI and compatible connections. `POST /v1/responses` can route natively to OpenAI connections or translate requests to Anthropic Messages. Both support JSON and SSE. Responses can also route to a Chat Completions-only connection. For this mapping, the gateway converts stateless text, system instructions, basic generation options, JSON object mode, and client-side function tools. It keeps tool call IDs and streams argument deltas. Attachments and JSON Schema require a connection or conversion that supports them. Responses runs in stateless mode with `store` set to `false`. It rejects upstream conversations, previous response IDs, and server-side tools. The `ai_gateway.call` structured log records outcomes and upstream-reported token counts. Set `usage_channel` to select a logging channel that accepts `info` records. Unreported token counts are recorded as `null`.

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

The SDK saves completed user and assistant messages together. The next prompt receives the latest 100 messages in chronological order by default. Change `conversations.max_messages` in `config/ai.rs` to adjust that limit. The store uses the default database connection unless `AI_CONVERSATIONS_CONNECTION` names another connection. For a history screen, use `Ai::conversation_store()?.paginate_conversation_messages(id, &participant, 15, cursor)` for newest-first cursor pagination. Use `stream()` for a streamed turn and pass its `response` to your SSE renderer. The conversation ID is available immediately. The SDK saves the turn before sending the final completion event. An interrupted stream leaves no new messages. For an Agent with a schema, use `prompt_structured()` or `stream_structured()`.

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

## Generate an Agent or tool {#generators}

```shell
./artisan make:agent CatalogAgent
./artisan make:agent Products/StockAgent --structured
./artisan make:tool LookupInventory
./artisan make:agent-middleware CheckInventory
./artisan vendor:publish --tag=ai-stubs
```

Agents and tools are generated under `app/ai/agents` and `app/ai/tools`. Declare the generated modules in their parent Rust modules and recompile. Add `--force` to replace an existing generated file. Use the published files in `stubs/` to customize the templates.

Declare reusable middleware with `Agent::middleware`, or pass it to an anonymous Agent using `with_middleware`. For each buffered generation step, `PendingStep` exposes the selected model, step number, previous steps and usage. Call `next.run(step)` to continue. Use `with_model`, `with_instructions` and `only_tools` to change that step.

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

Call `cache(Some(600), None)` to cache embeddings for 600 seconds. The configuration under `ai.caching.embeddings` defines the default lifetime, the cache store and whether to enable caching for all SDK calls. Individual caching reuses each vector across batches and sends only misses to the provider. Pass `Some(false)` as the second argument to cache the full, ordered batch. Use `cache(Some(0), None)` to bypass caching for an individual call. Usage on a partial hit reflects the inputs sent upstream.

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
