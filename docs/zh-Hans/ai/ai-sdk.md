# AI SDK

## 支持状态 {#support-status}

可选 `ai` feature 提供 Laravel 风格的 `Ai`、`AiManager`、`Agent` 和文本 Fake。当前支持 OpenAI Responses 和 OpenAI 兼容 Chat Completions 的普通文本与文本 SSE。

SDK 和网关复用框架的 [HTTP 客户端](../digging-deeper/http-client.md)。

## 可用入口 {#available-entry-points}

在应用 `Cargo.toml` 的 `[features].default` 中加入 `ai` 后，包发现会注册 SDK。选择一个支持的连接，填写对应的凭证和模型 ID。例如，使用兼容驱动连接 DeepSeek：

```dotenv
AI_PROVIDER=deepseek
DEEPSEEK_API_KEY=
DEEPSEEK_MODEL=deepseek-flash
```

API Key 填入应用环境。官方基础地址可通过 `DEEPSEEK_URL` 覆盖。也可在 `config/ai.rs` 自定义 `ai.providers` 的命名连接。模型 ID 可写入连接的 `models.text.default`，或在调用时显式指定。

```rust
use openrust_ai::{Agent, agent};

let response = agent("用简体中文简要回答。").prompt("如何申请退款？").await?;
let answer = response.text;
```

在启用 SDK 的应用发行版中，可运行 `./artisan vendor:publish --tag=ai-config` 发布配置模板。在应用测试中，使用 `Ai::fake::<YourAgent>(responses)` 安装应用级替身，可校验提示内容和调用次数。

运行 `./artisan ai:models deepseek` 查看模型，或加 `--json` 区分上游发现和本地配置。手填模型放在连接的 `models.text.ids`，列表会按原始 ID 合并并保留手填项。用 `./artisan ai:prompt "请简单回答" --provider=deepseek --model=deepseek-flash` 试调用所选模型。

运行 `./artisan ai:select-model --save --default` 选择模型，并将该连接设为应用默认。Artisan 使用当前默认连接，或在只有一家配置了凭证时直接提示该连接的模型。也可指定连接名。手填模型用 `--model=your-model-id`，自动化场景可加 `--no-interaction`。保存写入应用实际加载的环境文件。使用配置缓存时，保存后重新生成缓存。

通过 `agent(instructions).with_messages(history)` 带上历史对话。按顺序使用 `Message::user(text)` 和 `Message::assistant(text)` 构建历史消息，`prompt` 和 `stream` 会先发送历史，再发送本次提示词。Artisan 的 `--messages` 接受由 `role` 和 `content` 组成的 JSON 消息数组。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 返回 SDK 的默认 SSE 事件。也可以通过 `into_stream()` 消费事件。

在 `to_response()` 之前调用 `using_vercel_data_protocol(message_id)` 可返回 Vercel AI SDK UI 文本事件，调用 `using_agent_user_interaction_protocol(thread_id, run_id)` 可返回 AG-UI 文本事件。传入 `None` 时由 SDK 提供 ID。两种格式都会附上结束状态和上游报告的 Token 用量。应用可用 `using_protocol` 自定义同一批 Agent 事件的输出格式。

使用 `agent(instructions).with_schema(schema).prompt(text)` 请求 JSON 对象。Schema 用框架已有的 `JsonSchema` 构建。可读取 `response.structured`，或通过 `response.decode::<YourType>()` 转为 Rust 类型。所选模型须支持连接的原生结构化输出格式。结构化流将 JSON 作为文本增量发出。可用 `./artisan ai:prompt` 的 `--schema` 传入 JSON 对象 Schema 试调用。

当前请求或命令需要临时连接时，可用 `Ai::build(config)`，字段沿用具名连接配置。通过返回的 `name()` 在提示词调用中指定连接，或在同一作用域内通过 `Ai::text_provider` 查找。框架保留名及应用已有名称不能用作临时连接。`Ai::flush_state()` 会清除当前作用域的临时连接。

## 范围 {#scope}

当前响应提供文本、来源连接、模型以及上游实际报告的输入和输出 Token 数。Anthropic 与 Gemini 原生连接也支持普通文本、SSE 和分页模型目录。持久化会话仍待实现。

## 模型网关 {#model-gateway}

在应用 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，并设置 `AI_GATEWAY_ENABLED=true`。运行 `./artisan vendor:publish --tag=ai-gateway-config` 后，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填写发布关系及下游 Key 的权限，再重新构建应用：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在应用环境中为 `AI_GATEWAY_SUPPORT_KEY` 设置独立的随机密钥，`DEEPSEEK_API_KEY` 仍由上游连接单独使用。外部客户端将基础 URL 设为 `https://你的应用域名/v1`，用下游 Key 作为 Bearer 凭证，模型填写 `support-chat`。`GET /v1/models` 只显示当前 Key 有权调用的已发布模型。可选的 `requests_per_minute` 通过应用的限流缓存执行。

`POST /v1/chat/completions` 支持 OpenAI 与兼容连接。`POST /v1/responses` 可原生转发至 OpenAI 连接，也可将请求转换为 Anthropic Messages。两个入口均支持 JSON 和 SSE。Responses 当前采用无状态模式，令 `store` 为 `false`，并拒绝上游会话、旧 Response ID 和服务端工具。网关以结构化日志 `ai_gateway.call` 记录调用结果和上游报告的 Token 数。可用 `usage_channel` 指定日志通道，通道须允许 `info` 级别。未报告的计数记为 `null`。

使用 Anthropic 客户端时，基础 URL 指向应用域名，下游 Key 通过 `x-api-key` 提交。网关支持 `/v1/messages` 和 `/v1/messages/count_tokens`，模型可映射到 Anthropic 原生连接。Messages 请求也可以转发到已发布的 OpenAI Responses 模型。带 `x-api-key` 的 `/v1/models` 和 `/v1/models/{model}` 使用 Anthropic 格式，Bearer 模型目录使用 OpenAI 格式。

两个方向的转换都支持普通 JSON 和 SSE，给客户端返回公开模型名。上游报告缓存读写用量时，网关会按目标协议返回相应计数。

同一个 Anthropic 发布模型同时可由 Messages 和 Responses 客户端调用。转换支持文本、系统指令、基本采样参数和客户端函数工具，支持 JSON 和 SSE。工具调用 ID、流式参数增量和文本结果可双向传递。结构化输出和附件使用相应的原生端点。

Gemini 客户端使用 `/v1beta` API，通过 `x-goog-api-key` 提交下游 Key。网关列出授权的 Gemini 模型，并支持 `/v1beta/models/{model}` 下的 `:generateContent`、`:streamGenerateContent?alt=sse` 和 `:countTokens`。在 SDK 或 Artisan 中选择 `gemini` 连接，模型 ID 可带 `models/` 前缀。

公开模型映射可配置 `fallbacks`，依次尝试备用连接。每项填写 `provider` 和 `model`，例如 `[{"provider":"openai","model":"backup-model"}]`。无状态请求遇到上游 429、5xx 等可重试错误时，网关会尝试备用连接。客户端仍使用同一个公开名，每次尝试用同一个请求 ID 记录。带工具、要求存储输出或已经开始返回的流不参与重放。

可在 `config/ai_gateway.rs` 中设置 `accounts.deepseek.cooldown_seconds = 30`，让具名上游连接在返回 HTTP 429、503 或 529 后进入冷却。上游有效的 `Retry-After` 优先，最长 24 小时。引用同一连接的所有公开模型共用冷却状态。可用连接都在冷却时，网关返回 HTTP 503 和 `Retry-After`。多进程部署可把 `account_cache_store` 指向共享缓存，例如 `redis`。

设置 `accounts.deepseek.max_concurrent_requests` 可限制该连接的并发请求数。使用同一缓存的网关进程共用槽位。流式请求会占用槽位，直到结束或客户端断开。首选连接已满时，网关在发出请求前检查下一个可用连接。如果所有可用连接的槽位都已满，返回 HTTP 503 和 `upstream_busy`。

多个 Key 共用权限时，可配置分组，并在 Key 中填写 `group`。分组的 `models` 列出公开模型别名，可选的 `providers` 限制首选和回退时可以调用的上游连接。Key 也可以再填写 `models`，缩小分组授予的模型范围。

```rust
"groups": {
    "support-team": {"models": ["support-chat"], "providers": ["deepseek", "openai"]},
},
"keys": {
    "support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "group": "support-team"},
},
```

将 `managed_keys.enabled` 设为 `true` 可启用数据库托管 Key。选填的 `managed_keys.connection` 指定数据库连接。先运行包迁移，再为已有分组签发 Key。

```shell
./artisan migrate
./artisan ai:gateway:key:create support-client support-team --expires-in=86400 --requests-per-minute=60
./artisan ai:gateway:key:list
./artisan ai:gateway:key:rotate KEY_ID
./artisan ai:gateway:key:revoke KEY_ID
```

签发和轮换只返回一次可用的 Bearer Token。数据库保存 SHA-256 摘要。轮换沿用 Key ID、分组、有效期和请求额度。旧版本、撤销或过期的 Token 后续请求返回 HTTP 401。需要定制迁移时，执行 `./artisan vendor:publish --tag=ai-gateway-migrations`，重新编译后运行迁移。

## Agent 工具 {#agent-tools}

实现 `Tool` 的名称、描述、JSON Schema 和异步 `handle`。用 `ToolRequest::validate` 运行框架验证规则，或通过 `decode` 将参数读为 Rust 类型。工具可以读取上游调用 ID 和本次执行的独立 ID。

```rust
let response = agent("回答前先查目录。")
    .with_tools(vec![std::sync::Arc::new(LookupInventory)])
    .prompt("还有多少库存？")
    .await?;
let answer = response.text;
```

工具调用支持 Chat Completions、Responses、Anthropic Messages 和 Gemini GenerateContent 的非流式请求。Claude 和 Gemini 会收到原始模型内容及工具结果，以便供应商核对签名。Agent 用原调用 ID 把结果送回模型，并在响应中提供 `tool_calls` 和 `tool_results`。用 `with_max_steps(n)` 限制模型步骤数。最后一个步骤提出的工具调用会记录为失败结果，工具不会执行。

## 生成 Agent 或工具 {#generators}

```shell
./artisan make:agent CatalogAgent
./artisan make:agent Products/StockAgent --structured
./artisan make:tool LookupInventory
./artisan make:agent-middleware CheckInventory
./artisan vendor:publish --tag=ai-stubs
```

Agent 和工具分别生成到 `app/ai/agents` 和 `app/ai/tools`。在父模块中声明生成的 Rust 模块，再重新编译。加 `--force` 可覆盖已有文件。发布后可编辑 `stubs/` 下的文件，自定义生成模板。

在 Agent 上通过 `Agent::middleware` 配置中间件，匿名 Agent 则使用 `with_middleware`。非流式生成的每一步都会收到 `PendingStep`，其中包含当前模型、步骤编号、已完成步骤和用量。调用 `next.run(step)` 继续执行。用 `with_model`、`with_instructions` 和 `only_tools` 修改当前步骤。

## Embeddings {#embeddings}

设置 `AI_EMBEDDINGS_PROVIDER=openai`，并将供应商的模型 ID 填入 `OPENAI_EMBEDDING_MODEL`。SDK 批量发送文本，按输入顺序返回向量。

```rust
use openrust_ai::Embeddings;

let response = Embeddings::r#for(["配送", "退货"])?
    .generate()
    .await?;
let vectors = response.embeddings;
```

所选模型支持指定维度时，可使用 `dimensions(n)`。使用 OpenAI 兼容连接生成向量前，先配置该连接的 `models.embeddings`。通过 `POST /v1/embeddings` 发布向量模型时，映射公开名并为该模型配置 `"endpoints": ["embeddings"]`，再将公开名授权给下游 Key。

调用 `cache(Some(600), None)` 可将向量缓存 600 秒。在 `ai.caching.embeddings` 设置默认有效期、缓存存储以及是否为全部 SDK 调用启用缓存。逐条缓存能跨批次复用向量，只把未命中的输入发给供应商。第二个参数传 `Some(false)` 会按原顺序缓存整批。单次调用可用 `cache(Some(0), None)` 跳过缓存。部分命中时，用量只记录实际发给上游的输入。

### 测试 Embeddings {#embeddings-fake}

用 `Embeddings::fake` 按预期调用顺序提供向量批次，或用 `fake_with` 根据记录的请求生成向量。空序列在指定维度时生成单位向量。启用 `prevent_stray_embeddings(true)` 可以要求每次调用都使用预设响应。

```rust
let fake = Embeddings::fake([vec![vec![0.5, 0.2]]])?;
fake.prevent_stray_embeddings(true);
let result = Embeddings::r#for(["配送"])?.dimensions(2).generate().await?;
Embeddings::assert_generated(|call| call.inputs == ["配送"]);
```

SDK 记录 Fake 调用。Fake 不联系模型供应商，也不读写配置的向量缓存。

## 文档重排序 {#reranking}

用 `AI_RERANKING_PROVIDER` 选择默认连接，然后填写该连接的 API Key 和重排序模型 ID。

| 连接 | Base URL | URL 覆盖 | API Key | Model ID |
| --- | --- | --- | --- | --- |
| `cohere` | `https://api.cohere.com/v2` | `COHERE_URL` | `COHERE_API_KEY` | `COHERE_RERANK_MODEL` |
| `jina` | `https://api.jina.ai/v1` | `JINA_URL` | `JINA_API_KEY` | `JINA_RERANK_MODEL` |
| `voyageai` | `https://api.voyageai.com/v1` | `VOYAGE_URL` | `VOYAGE_API_KEY` | `VOYAGE_RERANK_MODEL` |
| `openrouter` | `https://openrouter.ai/api/v1` | `OPENROUTER_URL` | `OPENROUTER_API_KEY` | `OPENROUTER_RERANK_MODEL` |

```rust
use openrust_ai::Reranking;

let result = Reranking::of(["配送", "换货", "退货"])?
    .limit(2)
    .rerank("如何退货？")
    .await?;
```

结果保持供应商返回的排序顺序。每项的 `index` 都指向原始文档位置，可用来取回输入集合中的原始记录。
