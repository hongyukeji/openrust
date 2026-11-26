# AI SDK

## 支持状态 {#support-status}

可选 `ai` feature 提供 Laravel 风格的 `Ai`、`AiManager`、`Agent` 和文本 Fake。普通文本和流式调用支持 OpenAI Responses、OpenAI 兼容 Chat Completions、Anthropic Messages 和 Gemini GenerateContent。Agent 可以将文本会话保存在应用数据库。

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

## Azure OpenAI {#azure-openai}

使用 `azure` 预设时，将 `AZURE_OPENAI_URL` 设为 Azure 资源地址，再填写 `AZURE_OPENAI_API_KEY`。预设会按需追加 `/openai/v1`。文本调用用 `AZURE_OPENAI_DEPLOYMENT` 指定部署名称。添加其他具名连接时，使用 `driver: "openai"`、完整的 v1 URL 和 `auth: "api-key"`。

## 范围 {#scope}

当前响应提供文本、来源连接、模型以及上游实际报告的输入和输出 Token 数。Anthropic 与 Gemini 原生连接也支持普通文本、SSE 和分页模型目录。

## 模型网关 {#model-gateway}

在应用 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，并设置 `AI_GATEWAY_ENABLED=true`。运行 `./artisan vendor:publish --tag=ai-gateway-config` 后，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填写发布关系及下游 Key 的权限，再重新构建应用：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在应用环境中为 `AI_GATEWAY_SUPPORT_KEY` 设置独立的随机密钥，`DEEPSEEK_API_KEY` 仍由上游连接单独使用。外部客户端将基础 URL 设为 `https://你的应用域名/v1`，用下游 Key 作为 Bearer 凭证，模型填写 `support-chat`。`GET /v1/models` 只显示当前 Key 有权调用的已发布模型。可选的 `requests_per_minute` 通过应用的限流缓存执行。

`POST /v1/chat/completions` 支持 OpenAI 与兼容连接。`POST /v1/responses` 可原生转发至 OpenAI 连接，也可将请求转换为 Anthropic Messages。两个入口均支持 JSON 和 SSE。Responses 也可以路由到只提供 Chat Completions 的连接。这条转换支持无状态文本、系统指令、基础生成参数、JSON 对象模式和客户端函数工具。工具调用 ID 原样回传，参数增量会流式发送。附件和 JSON Schema 需选择支持相应能力的连接或转换。Responses 当前采用无状态模式，令 `store` 为 `false`。上游会话和旧 Response ID 当前不可用。服务端网页搜索需要在公开模型上明确启用。网关以结构化日志 `ai_gateway.call` 记录调用结果和上游报告的 Token 数。可用 `usage_channel` 指定日志通道，通道须允许 `info` 级别。未报告的计数记为 `null`。

使用 Anthropic 客户端时，基础 URL 指向应用域名，下游 Key 通过 `x-api-key` 提交。网关支持 `/v1/messages` 和 `/v1/messages/count_tokens`，模型可映射到 Anthropic 原生连接。Messages 请求也可以路由到已发布的 OpenAI Responses 或 Chat Completions 模型。网关保留客户端工具调用 ID，并按顺序发送每个工具的内容块。带 `x-api-key` 的 `/v1/models` 和 `/v1/models/{model}` 使用 Anthropic 格式，Bearer 模型目录使用 OpenAI 格式。

通过 `POST /v1/responses/input_tokens` 统计已发布的 OpenAI 或 Anthropic 模型输入。发送包含模型和无状态输入的 JSON，网关调用上游计数并返回 `input_tokens`。Messages 计数入口也可使用 OpenAI Responses 连接。计数请求计入下游 Key 的请求次数，计数结果不进入生成用量。

客户端需要不含 `/v1` 的路径时，在 `config/ai_gateway.rs` 将 `unversioned_paths` 设为 `true`。这会增加 `/models`、`/models/{model}`、`/chat/completions`、`/responses`、`/responses/input_tokens`、`/embeddings` 和 `/messages/count_tokens`，并共用原有授权与限流。

两个方向的转换都支持普通 JSON 和 SSE，给客户端返回公开模型名。上游报告缓存读写用量时，网关会按目标协议返回相应计数。

同一个 Anthropic 发布模型可供 Messages、Responses 和 Chat Completions 客户端调用。转换支持文本、系统指令、基本采样参数和客户端函数工具，支持 JSON 和 SSE。工具调用 ID、流式参数增量和文本结果可双向传递。结构化输出和附件使用相应的原生端点。

Gemini 客户端使用 `/v1beta` API，通过 `x-goog-api-key` 提交下游 Key。网关列出授权的 Gemini 模型，并支持 `/v1beta/models/{model}` 下的 `:generateContent`、`:streamGenerateContent?alt=sse` 和 `:countTokens`。在 SDK 或 Artisan 中选择 `gemini` 连接，模型 ID 可带 `models/` 前缀。

公开模型映射可配置 `fallbacks`，依次尝试备用连接。每项填写 `provider` 和 `model`，例如 `[{"provider":"openai","model":"backup-model"}]`。无状态请求遇到上游 429、5xx 等可重试错误时，网关会尝试备用连接。客户端仍使用同一个公开名，每次尝试用同一个请求 ID 记录。带工具、要求存储输出或已经开始返回的流不参与重放。

可在 `config/ai_gateway.rs` 中设置 `accounts.deepseek.cooldown_seconds = 30`，让具名上游连接在返回 HTTP 429、503 或 529 后进入冷却。上游有效的 `Retry-After` 优先，最长 24 小时。引用同一连接的所有公开模型共用冷却状态。可用连接都在冷却时，网关返回 HTTP 503 和 `Retry-After`。多进程部署可把 `account_cache_store` 指向共享缓存，例如 `redis`。

需要让同一客户端会话优先使用上一次成功的上游时，将 `account_session_ttl` 设为 1 至 86400 秒，并在每次请求发送 `X-Session-ID`。会话按下游 Key 与公开模型隔离。首选连接仍需通过授权、冷却和并发检查。成功响应会刷新有效期。缓存暂不可用时，网关按配置顺序选路。多副本可复用 `account_cache_store` 共享会话黏性。

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

### Responses WebSocket {#responses-websocket}

使用 HTTP 端点同一 Bearer Key 连接 `wss://your-app.example/v1/responses`。启用 `unversioned_paths` 时也可使用 `/responses`。发送 `response.create` JSON 文本帧，读取 Responses 事件直至 `response.completed` 或 `response.incomplete`。

```json
{
  "type": "response.create",
  "model": "support-chat",
  "input": "How do I return an item?"
}
```

为并行通道指定不同的 `stream_id`。每条连接最多四个活动通道，每个通道同时处理一个响应。事件携带所属通道 ID。每轮沿用 HTTP 的鉴权、路由、限额和用量记录。关闭连接会取消所有正在生成的响应。

继续文本或客户端函数对话时，将最近一次已完成响应的 ID 放入 `previous_response_id`，并在 `input` 中只提交新消息或函数结果。沿用同一通道可续接，换一个 `stream_id` 可从仍在内存中的响应分叉。省略 `model` 时沿用原别名。`instructions` 仅用于当前轮。每条连接保留最多四份历史，每份上限为 512 KiB、256 项。重连、更换模型或收到 `previous_response_not_found` 时，重新发送完整上下文。

文本帧上限为 2 MiB，连接满一小时后需重连。

### Codex 模型目录 {#codex-model-catalog}

使用 Bearer 凭证请求 `/v1/models?client_version=0.170.0` 可获得 Codex 模型目录。Codex 也可将基础 URL 设为 `https://your-app.example/backend-api/codex`。此前缀提供 `/models`、HTTP 与 WebSocket 的 `/responses`，以及 `/responses/input_tokens`。目录只列出当前 Key 能通过 Responses 调用的公开模型。普通 OpenAI 和 Anthropic 模型端点沿用各自格式。

在公开模型映射中可选填 `codex` 对象，用于配置显示名称、说明、上下文窗口、输入类型和推理选项。默认使用公开模型名作为显示名称，并声明文本输入。客户端可带上 `If-None-Match` 复用已授权的目录。

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

在 Codex 中配置 Responses 连接，填写网关基础 URL（例如 `https://your-app.example/v1`）和下游 Key，并用公开别名作为模型。原生 Responses 上游支持客户端工具命名空间。要使用上游网页搜索，在公开模型上设置 `"server_tools": ["web_search"]`。模型未启用网页搜索时，在 Codex 客户端配置中使用 `web_search = "disabled"`。

### 接入 DeepSeek Harness {#dsh-model-connection}

在 DeepSeek Harness 中打开 **设置 → 模型 → 添加模型提供商**，选择 **自定义模型 API**。填写连接 ID、网关下游 Key 和对应协议的基础 URL。

| DSH 协议 | 基础 URL |
| --- | --- |
| OpenAI Chat Completions | `https://your-app.example/v1` |
| OpenAI Responses | `https://your-app.example/v1` |
| Anthropic Messages | `https://your-app.example` |
| Gemini API | `https://your-app.example/v1beta` |

通过 **拉取可用模型** 选择模型，或使用 **添加模型** 手填网关发布的别名。保存后，在对话输入区选择该连接的模型。

### 持久化网关用量 {#gateway-usage}

在 `config/ai_gateway.rs` 设置 `usage.enabled = true` 后执行迁移。多实例共享用量数据库时，可通过 `usage.connection` 指定连接。

```shell
./artisan migrate
./artisan ai:gateway:usage:list --limit=20
./artisan ai:gateway:usage:list --request-id=REQUEST_ID
```

每次上游尝试各有一条记录。回退尝试共用 `request_id`。记录包含下游 Key 或分组、公开模型、连接、上游模型、响应状态和供应商实际返回的 Token 数。缺失用量保留为 `null`。账本省略 Key 密钥、请求正文和生成文本。

网关先登记尝试，再向上游发送请求。正常结束、出错和取消都会结算。进程在结算之前退出时，账本可能保留 `pending` 记录供核对。

使用发起模型调用的 Key，以 Bearer 或 `x-api-key` 鉴权请求 `GET /v1/usage?days=7`。返回该 Key 当日、累计以及按 UTC 日期汇总的用量。`days` 可设为 1 至 90。回退只计为一次请求，同时按实际出站次数统计尝试。缺失的 Token 数保持 `null`。

`model_stats` 返回当前 Key 最近 30 天按公开模型名汇总的用量。可用 `start_date` 和 `end_date` 指定 `YYYY-MM-DD` 格式的 UTC 日期，包含首尾两天。省略或无法解析的日期使用相应默认值，此范围仅影响 `model_stats`。

托管 Key 轮换后保留历史。替换环境 Key 后，新凭证开始累积自己的历史。运维可通过 Artisan 账本查询早期记录和上游明细。

### 分组输出 Token 配额 {#gateway-output-quota}

启用用量账本并运行迁移后，可在分组上配置输出 Token 配额。同组所有 Key 在各网关实例共享每日额度，按 UTC 零点重置。

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

网关按 `max_output_tokens` 限制每次尝试并在出站前预留额度。请求指定更小的输出上限时，预留相应减少。成功响应按供应商报告的输出 Token 结算。失败、取消或缺少最终用量时保留预留到窗口重置。每次回退都需要独立预留。

额度不足时返回 HTTP 429 和 `Retry-After`。若供应商报告的输出超出预留，该分组停止接纳新的生成请求，直到下一个窗口。配额支持 Chat Completions、Responses 和 Anthropic Messages 生成。Token 计数与 Embeddings 沿用请求次数限制。

## 持久会话 {#conversations}

给 Agent 实现 `RemembersConversations` 即可把对话存到应用数据库。先发布迁移，重新编译应用，再执行迁移。

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

继续会话时传入已认证参与者的类型和 ID。`for_user(&user)` 可直接为认证用户开启会话。`continue_last_conversation(participant)` 寻找该参与者与同一 Agent 类型的上一会话，没有历史时开启新会话。

SDK 将完成的用户消息和助手消息一起保存。下一次提示默认按时间顺序读取最近 100 条消息，可在 `config/ai.rs` 中调整 `conversations.max_messages`。数据库默认沿用应用连接，也可用 `AI_CONVERSATIONS_CONNECTION` 指定具名连接。历史页面使用 `Ai::conversation_store()?.paginate_conversation_messages(id, &participant, 15, cursor)` 从最新消息开始游标分页。流式会话使用 `stream()`，将返回的 `response` 交给 SSE 渲染器。调用开始后即可读取会话 ID。SDK 在发送最终完成事件前保存该轮消息，流中断时不会新增会话消息。声明了响应结构的 Agent 可使用 `prompt_structured()` 或 `stream_structured()`。

## 文本摘要 {#text-summarization}

导入扩展 trait 后，`Str` 和 `Stringable` 共用同一个摘要 Agent，默认请求不超过三句。`summarize_with` 可传入句数，并用常规的 `PromptOptions` 指定连接、模型和超时。

```rust
use openrust::support::Str;
use openrust_ai::{PromptOptions, Summarizes, SummarizesText};

let short = Str::summarize(article).await?;
let detailed = Str::of(article)
    .summarize_with(5, PromptOptions::default())
    .await?;
```

摘要使用所选连接的 `models.text.cheapest`。Agent 也可选择 `TextModelTier::Smartest`，使用 `models.text.smartest`。OpenAI 预设可用 `OPENAI_MODEL_CHEAPEST` 和 `OPENAI_MODEL_SMARTEST` 设置，其他预设在其模型变量上加相同后缀。未配置的档位回退到连接的默认模型，显式指定的模型优先。

## 图片附件 {#prompt-images}

```rust
use openrust_ai::{Agent, ImageAttachment, agent};

let photo = ImageAttachment::from_storage("photos/sample.png", None).await?;
let response = agent("Describe the scene.")
    .with_attachments([photo])
    .prompt("What can you see?")
    .await?;
```

上传图片用 `ImageAttachment::from_upload(&file)` 读取，本地文件用 `ImageAttachment::from_path(path).await`，字节用 `ImageAttachment::from_bytes(bytes)`。具名 Agent 通过 `Agent::attachments` 提供附件，匿名 Agent 可以用 `with_attachments`。

所选模型需支持图片输入。OpenAI Responses、Chat Completions、Anthropic Messages 和 Gemini 分别使用原生图片格式。SDK 接收 PNG、JPEG 和 WebP，单次最多 16 张，原始图片总量须小于 50 MB，同时受供应商限制。可以只发送图片，也可以附上文字。附件跟随本轮调用及工具续步，文本会话存储只记录文本。

### 文档附件 {#document-attachments}

使用 `DocumentAttachment` 读取 PDF、UTF-8 文本文档或 Markdown。支持 `from_bytes(filename, bytes)`、`from_upload(&file)`、`from_path(path).await` 和 `from_storage(path, disk).await`。混合传入图片和文档时，用 `Attachment` 统一类型，保留传入顺序。

```rust
use openrust_ai::{Agent, Attachment, DocumentAttachment, ImageAttachment, agent};

let report = DocumentAttachment::from_storage("reports/quarterly.pdf", None).await?;
let chart = ImageAttachment::from_storage("reports/chart.png", None).await?;
let response = agent("Review the quarterly report.")
    .with_attachments([Attachment::from(report), Attachment::from(chart)])
    .prompt("Summarize the results.")
    .await?;
```

请选择支持文档的模型。Responses、Anthropic Messages 和 Gemini 接收这三种格式。Chat Completions 仅接收 PDF。图片和文档共用每次提示词的附件数量及大小限额。

## Agent 工具 {#agent-tools}

实现 `Tool` 的名称、描述、JSON Schema 和异步 `handle`。用 `ToolRequest::validate` 运行框架验证规则，或通过 `decode` 将参数读为 Rust 类型。工具可以读取上游调用 ID 和本次执行的独立 ID。

```rust
let response = agent("回答前先查目录。")
    .with_tools(vec![std::sync::Arc::new(LookupInventory)])
    .prompt("还有多少库存？")
    .await?;
let answer = response.text;
```

工具调用支持 Chat Completions、Responses、Anthropic Messages 和 Gemini GenerateContent 的普通与流式请求。Claude 和 Gemini 会收到原始模型内容及工具结果，以便供应商核对签名。SDK 实时发送文本，在下一次模型调用前发送完整的工具调用及结果。默认 SSE、Vercel UI 和 AG-UI 都提供相应的工具事件。Agent 用原调用 ID 把结果送回模型，并在响应中提供 `tool_calls` 和 `tool_results`。用 `with_max_steps(n)` 限制模型步骤数。最后一个步骤提出的工具调用会记录为失败结果，工具不会执行。

## Agent 排队 {#queued-agents}

给具名 Agent 派生 `Serialize` 和 `Deserialize`，实现 `QueueableAgent` 并提供稳定的 `QUEUE_JOB` 名称。在应用服务提供者的 `boot` 方法中，从应用取得 `QueueManager`，调用 `TicketAgent::register_queue(queue.as_ref())` 注册。

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

用 `queued_then` 和 `queued_catch` 将回调名解析为类型化函数。成功回调接收还原后的 Agent 和 `AgentResponse`，失败回调接收 Agent 和任务终态错误。重试和退避沿用框架队列。任务可能重复投递，因此回调中的写入需要幂等。上游密钥由 Worker 的具名连接配置提供。

### 广播流事件 {#broadcast-agents}

`broadcast_now` 立即发布各事件，`broadcast` 通过框架排队投递事件。两者都返回流，消费流或返回 `to_response()` 后才开始发送。用户数据应使用私有频道，并由应用的频道规则授权订阅。

```rust
use openrust::broadcasting::PrivateChannel;
use openrust_ai::Agent;

let channel = PrivateChannel { name: format!("tickets.{ticket_id}") };
let stream = agent.broadcast_now("Summarize this ticket", [channel.into()]).await?;
let response = stream.to_response();
```

`broadcast_on_queue` 在 Worker 内生成和发布流。派发前可通过 `invocation_id()` 取得调用标识，用于关联客户端事件。成功回调接收汇总响应，终态失败会发送同一调用标识的 `stream_failed`。广播任务默认只尝试一次，因为订阅者可能已收到部分事件。

Agent 可覆盖 `without_broadcasting` 来排除部分广播事件，同时在 HTTP 流中保留这些事件。即时投递失败会写入日志。广播服务自身的消息大小限制仍有效。

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

导入 `Reranks` 后，可调用 `collection.rerank(|item| item.title.clone(), query).await?`，按相关度返回集合中的原始条目。需要覆盖返回条数、超时、连接或模型时，使用 `rerank_with` 和 `RerankingOptions`。

用 `Reranking::fake([])?` 生成测试排名，调用不会访问供应商。也可预设结果，或通过 `Reranking::fake_with` 编写回调。通过 `assert_reranked`、`assert_not_reranked` 和 `assert_nothing_reranked` 断言调用。替身的 `prevent_stray_rerankings(true)` 会拒绝未配置响应的调用。

## 图像生成 {#images}

选择图像连接和模型。OpenAI 沿用 `OPENAI_API_KEY`，可用 `OPENAI_URL` 覆盖服务地址。

```dotenv
AI_IMAGE_PROVIDER=openai
OPENAI_IMAGE_MODEL=gpt-image-1.5
```

```rust
use openrust_ai::Image;

let response = Image::of("A watercolor pear")?.square().quality("high").generate().await?;
let path = response.store_as("generated", "pear.png", None).await?;
```

通过 `square()`、`portrait()` 或 `landscape()` 选择标准尺寸，也可用 `size()`、`quality()` 或 `timeout()` 设置请求参数。编辑图像时，用 `ImageAttachment::from_bytes` 或 `ImageAttachment::from_storage` 创建参考图，调用 `attachments()` 后再生成。

支持 Images API 的 OpenAI 及 OpenAI 兼容连接可返回内联的 PNG、JPEG 或 WebP。兼容连接通过 `models.images` 启用图像能力。用 `first_image()` 读取 MIME 类型和字节，或用 `store` 和 `store_as` 写入磁盘。需要公开可见时，调用 `store_publicly` 或 `store_publicly_as`。

## 语音生成与转录 {#audio}

为两种能力分别选择连接和模型。OpenAI 沿用 `OPENAI_API_KEY`，可用 `OPENAI_URL` 覆盖服务地址。

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

使用 `Audio::of(text)`，或导入 `ToAudio` 在字符串上调用 `to_audio()`。用 `voice()`、`male()` 或 `female()` 选择音色，支持 MP3、WAV、Opus、AAC、FLAC 和 PCM 输出。通过 `store` 或 `store_as` 将字节写入应用的 Storage 磁盘。

已存到磁盘的录音用 `Transcription::from_storage` 加载，也可用 `from_path`、`from_upload` 或 `from_bytes` 读取本地输入。已知语言时设置 `language()`。需要说话人和分段时间时，使用 `diarize(true)` 并选择支持该功能的模型。响应包含文本和上游实际报告的用量。OpenAI 兼容连接可通过 `models.audio` 或 `models.transcription` 启用相应能力。

## 分类 {#classification}

将 `AI_CLASSIFICATION_PROVIDER` 设为连接名，并将该连接的默认模型设为支持对应 Decisions API 的模型 ID。

| 连接 | 默认模型环境变量 |
| --- | --- |
| `openai` | `OPENAI_CLASSIFICATION_MODEL` |
| `typesafe` | `TYPESAFE_CLASSIFICATION_MODEL` |
| `openrouter` | `OPENROUTER_CLASSIFICATION_MODEL` |
| `gemini` | `GEMINI_CLASSIFICATION_MODEL` |

各预设沿用自身的密钥和 URL 配置。TypeSafe 使用 `TYPESAFE_API_KEY` 和 `TYPESAFE_URL`。自定义连接可用 `system-one` 驱动接入 TypeSafe 协议，或在 OpenAI 兼容连接上设置 `models.classification.protocol = "openrouter-decisions"`，让分类请求走 OpenRouter 的 `/api/alpha/decisions`。支持 OpenAI Decisions 的兼容连接可通过 `models.classification` 设置默认模型并启用该协议。

```rust
use openrust_ai::{Classification, classification::{Boolean, Choice}};

let result = Classification::of("The item arrived damaged")?
    .question("eligible", Boolean::new("Is this a return request?"))
    .question("category", Choice::new("Choose a category", ["return", "other"]))
    .classify()
    .await?;

let category = result.answer("category");
```

分类输入可为文本、JSON 对象或 JSON 数组。为问题指定键，使用 `Boolean`、`Choice` 或 `Score` 声明问题类型，也可用 `attachments()` 添加内联图片。返回包含类型化答案、明确的拒答状态，以及上游实际报告的 Token 数。

TypeSafe 支持 PNG 和 JPEG。状态为 JSON 对象时，`images` 用于保存附件引用。OpenRouter 和 Gemini 连接也接受 WebP。实际可用性取决于所选模型和账号。

在字符串或 `Stringable` 上导入 `Decides` 即可做是非判断，也可导入 `DecidesText` 使用 `Str::decide`。导入 `DecidesBetween` 可从集合选出原始条目。`decide_by` 接受标签和描述生成函数，`decide_key` 可从键到描述的集合选出键。`DecideOptions` 可指定概率阈值、连接、模型和超时。选项概率低于阈值时返回 `None`。

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

测试时可以用 `Classification::fake([])` 自动生成类型化答案，也可提供答案映射或完整响应的序列，或使用 `fake_with` 根据请求生成答案。调用记录支持 `assert_classified`、`assert_not_classified` 和 `assert_nothing_classified`。开启 `prevent_stray_classifications(true)` 后，响应序列用尽即报错。

## 供应商文件 {#provider-files}

用 `AI_FILE_PROVIDER` 指定支持 Files API 的具名连接，也可以沿用 AI 默认连接。OpenAI、Azure OpenAI、Anthropic、Gemini 和 OpenRouter 全球端点原生支持。自定义 OpenAI 兼容连接提供同一 Files API 时可设置 `files: true`。

```rust
use openrust_ai::{FileUpload, Files};

let document = FileUpload::from_path("policy.txt").await?
    .with_mime_type("text/plain")?;
let file = Files::put(document, None).await?;
let id = file.id();
let private_path = file.store_as("ai", "policy.txt", Some("local")).await?;
```

HTTP 上传使用 `FileUpload::from_upload`，内存字节使用 `from_bytes`，已配置磁盘上的文件可使用 `Files::put_from_storage`。在 `FileUpload` 上可覆盖名称、MIME 类型和上传用途，默认用途为 `user_data`。Azure 预设使用 `assistants`。Anthropic 和 OpenRouter 在服务端管理用途，上传时使用默认设置。

`Files::get` 读取元数据，`Files::list` 按游标分页。返回的文件对象沿用原连接下载、刷新和删除。`store_as` 将内容存入私有磁盘路径。用户提供文件 ID 时，先在应用中校验其文件归属。 供应商返回 `downloadable: false` 时，读取或存盘返回 `AiError::FileNotDownloadable`。Anthropic 和 OpenRouter 对用户上传的文件使用此限制。

Gemini 返回类似 `files/policy` 的资源名。`Files::get` 和 `Files::delete` 也接受短 ID。上传后用 `refresh()` 检查状态是否为 `ACTIVE`，分页时原样传回令牌。Gemini 驱动支持上传与元数据操作，应用后续需要读取上传原件时，将原件保存在 Storage。

通过共享网关调用时，用相应协议的原生格式提交内联附件。供应商账号下的文件 ID 可在 SDK 中通过创建文件的连接使用。

测试中用 `Files::fake([])?` 在内存中上传、查询和删除文件。可以传入字节响应序列，也可以用 `fake_with` 根据请求 ID 生成内容。在替身上启用 `prevent_stray_operations(true)`，拒绝未配置的查询。可用 `assert_stored`、`assert_not_stored`、`assert_deleted` 和 `assert_not_deleted` 断言操作。`Files::fake_id("policy.txt")` 返回确定的测试 ID。

## 向量仓库 {#vector-stores}

用 `AI_STORE_PROVIDER` 选择向量仓库的具名连接，也可沿用 AI 默认连接。OpenAI、Azure OpenAI 与 Gemini File Search 原生支持。自定义 OpenAI 兼容端点可设置 `stores: true`，同一连接支持文件上传时再启用 `files: true`。

```rust
use openrust_ai::{Files, Stores};

let store = Stores::create("Support policies", None).await?;
let file = Files::get("file-policy", Some(store.provider())).await?;
let member = store.add(&file, &Default::default()).await?;
let ready = store.refresh().await?.is_ready();
```

`StoreOptions` 可指定已核实的初始文件 ID。OpenAI 还接受描述和空闲过期天数。`refresh()` 可读取索引进度和是否就绪。`Stores::list` 与 `Store::files` 支持游标分页。

仓库与文件须来自同一连接。`Store::remove(&member, false)` 移除成员并保留原文件。传入 `true` 会继续删除原文件。移除后删除失败时，`DetachedFileDeletionFailed` 会带回原文件 ID 供单独处理。使用 `add_file_id` 前先在应用中校验文件 ID 的归属。

使用 Gemini 时，原文件 ID 可用 `files/policy` 这样的资源名。导入等待索引操作完成，并返回单独的文档 ID。原文件 ID 已知时，`StoreFile::file_id()` 返回 `Some`。Gemini 文档列表省略原文件 ID，因此通过 `remove(&member, false)` 移除列表中的文档。若仓库已创建，但某个初始文件导入失败，`StorePopulationFailed` 会携带已创建的仓库 ID。

用 `Stores::fake([])?` 在内存中测试仓库创建、成员管理和删除。它也启用文件 Fake，便于上传文件、加入仓库并断言原文件是否删除。要保留当前文件连接，可用 `Stores::fake_with_files([], false)?`。可以传入查询名称序列，或用 `fake_with` 设置回调。可用 `assert_created`、`assert_not_created`、`assert_deleted` 和 `assert_not_deleted` 断言仓库操作。
