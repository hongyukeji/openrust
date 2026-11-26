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

用 DeepSeek Chat Completions 的思考模式时，使用自动工具选择。OpenRust 将上游推理字段保留在工具步骤中，续接同一次调用时回传。需要 DeepSeek 严格 JSON Schema 输出时，可增加一个具名连接，使用 `driver: "openai"` 和 `url: "https://api.deepseek.com/v1"` 接入 Responses API，继续使用 `DEEPSEEK_API_KEY` 环境变量。 [DeepSeek 思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode/) · [DeepSeek Responses API](https://api-docs.deepseek.com/zh-cn/api/create-response/)

AI 命令帮助使用应用当前语言。运行 `./artisan help ai:select-model` 或 `./artisan ai:prompt --help` 可查看本地化说明。可在 `lang/vendor/ai/<locale>/console.json` 中通过 `commands.<命令>.description` 覆写命令说明，通过 `commands.<命令>.arguments.<参数>` 覆写单个命令的参数说明。

`make:agent`、`make:tool` 和 `make:agent-middleware` 的交互、生成结果与名称验证文案使用应用当前语言。可在 `lang/vendor/ai/<locale>/console.json` 的 `make` 下覆写文案，`created` 中的 `:path` 会替换为生成的文件路径。

交互式连接和模型选择以及校验提示使用应用当前语言。数字选项和最终输出的模型 ID 在各语言下保持一致。可在 `lang/vendor/ai/<locale>/console.json` 的 `select_model` 下覆写交互和校验提示。

Artisan 使用应用当前语言显示连接和供应商诊断。可在 `lang/vendor/ai/<locale>/errors.json` 的 `console` 下覆写文案。`ai:models --json` 继续输出原有的模型目录数据。

通过 `agent(instructions).with_messages(history)` 带上历史对话。按顺序使用 `Message::user(text)` 和 `Message::assistant(text)` 构建历史消息，`prompt` 和 `stream` 会先发送历史，再发送本次提示词。Artisan 的 `--messages` 接受由 `role` 和 `content` 组成的 JSON 消息数组。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 返回 SDK 的默认 SSE 事件。也可以通过 `into_stream()` 消费事件。

在 `to_response()` 之前调用 `using_vercel_data_protocol(message_id)` 可返回 Vercel AI SDK UI 文本事件，调用 `using_agent_user_interaction_protocol(thread_id, run_id)` 可返回 AG-UI 文本事件。传入 `None` 时由 SDK 提供 ID。两种格式都会附上结束状态和上游报告的 Token 用量。应用可用 `using_protocol` 自定义同一批 Agent 事件的输出格式。

消费 `StreamableAgentResponse` 前可用 `.catch(|error| { /* 记录错误 */ })` 登记回调。流报告错误时，各回调按注册顺序执行一次，消费端仍收到流错误。连接与请求启动失败通过 `stream(...).await` 的结果处理。

可用 `stream.each(|event| { inspect(event); true }).await?` 逐个处理事件并保留响应。回调返回 `false` 时暂停本次遍历。之后通过 `each`、`into_stream` 或 `to_response` 消费响应，会按原 ID 重放已遍历的事件，并从同一次上游请求读取剩余事件。

可用 `stream.then(callback)` 在流消费完成时取得 `StreamedAgentResponse`。它包含合并后的文本与用量、工具调用及结果和原始事件。流暂停等待审批时，也会带上已保存的审批批次。在 `each` 完整遍历后注册回调会立即取得结果。

Vercel 和 AG-UI 流式适配器使用当前应用语言显示生成失败提示。可在 `lang/vendor/ai/<locale>/errors.json` 下覆写 `stream` 文案。

使用 `agent(instructions).with_schema(schema).prompt(text)` 请求 JSON 对象。Schema 用框架已有的 `JsonSchema` 构建。可读取 `response.structured`，或通过 `response.decode::<YourType>()` 转为 Rust 类型。所选模型须支持连接的原生结构化输出格式。结构化流将 JSON 作为文本增量发出。可用 `./artisan ai:prompt` 的 `--schema` 传入 JSON 对象 Schema 试调用。 SDK 会在出站 Schema 中禁用对象的额外属性，包括嵌套对象。如果模型执行 OpenAI 严格 Schema 校验，请将所有属性标记为必填。

当前请求或命令需要临时连接时，可用 `Ai::build(config)`，字段沿用具名连接配置。通过返回的 `name()` 在提示词调用中指定连接，或在同一作用域内通过 `Ai::text_provider` 查找。框架保留名及应用已有名称不能用作临时连接。`Ai::flush_state()` 会清除当前作用域的临时连接。

### 接入 Responses 网关 {#responses-gateway}

接入提供 OpenAI Responses 的 Sub2API 实例时，使用 `driver: "openai"` 和实例的 v1 基础地址。SDK 使用 `/models` 发现模型，通过 `/responses` 发起生成请求。设置 `store: false` 可请求上游关闭响应存储。

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

在 `SUB2API_API_KEY` 中设置 Bearer 凭证。实例需要附加头时，可在连接里配置 `headers`。名称和值均须符合 HTTP 规范，鉴权与协议所需的头由连接和 SDK 管理。可用 `./artisan ai:models sub2api --json` 确认所配置的模型也在发现结果中。

## Azure OpenAI {#azure-openai}

使用 `azure` 预设时，将 `AZURE_OPENAI_URL` 设为 Azure 资源地址，再填写 `AZURE_OPENAI_API_KEY`。预设会按需追加 `/openai/v1`。文本调用用 `AZURE_OPENAI_DEPLOYMENT` 指定部署名称。添加其他具名连接时，使用 `driver: "openai"`、完整的 v1 URL 和 `auth: "api-key"`。

## 范围 {#scope}

当前响应提供文本、来源连接、模型以及上游实际报告的输入和输出 Token 数。Anthropic 与 Gemini 原生连接也支持普通文本、SSE 和分页模型目录。

文本 `Usage` 提供提示与输出 Token 总量，并保留上游报告的缓存读取、缓存写入和推理计数。提示总量包含缓存计数，输出总量包含推理计数。可用 `total_tokens()` 和 `uncached_input_tokens()` 根据已知计数求总量与未缓存输入量，流式响应同样带上实际报告的计数。

## 模型网关 {#model-gateway}

在应用 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，并设置 `AI_GATEWAY_ENABLED=true`。运行 `./artisan vendor:publish --tag=ai-gateway-config` 后，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填写发布关系及下游 Key 的权限，再重新构建应用：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在应用环境中为 `AI_GATEWAY_SUPPORT_KEY` 设置独立的随机密钥，`DEEPSEEK_API_KEY` 仍由上游连接单独使用。外部客户端将基础 URL 设为 `https://你的应用域名/v1`，用下游 Key 作为 Bearer 凭证，模型填写 `support-chat`。`GET /v1/models` 只显示当前 Key 有权调用的已发布模型。可选的 `requests_per_minute` 通过应用的限流缓存执行。

网关鉴权失败和 SSE 终止错误文案使用应用选定的语言，可通过应用配置的语言请求头选择。可在 `lang/vendor/ai-gateway/<locale>/errors.json` 中覆写 `invalid_api_key` 和 `stream`。

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

### 网关图片生成与编辑 {#gateway-images}

在 `ai_gateway.models` 发布图片模型时，显式列出 `images/generations` 和 `images/edits`。沿用 Key 或分组授权公开模型名。上游可选 OpenAI 连接，或在 `models.images` 配置图片能力的 OpenAI 兼容连接。

```json
{
  "catalog-image": {
    "provider": "openai",
    "model": "gpt-image-1",
    "endpoints": ["images/generations", "images/edits"]
  }
}
```

`POST /v1/images/generations` 接收 JSON，`POST /v1/images/edits` 接收 JSON 或 multipart。表单可传单个 `image` 或有序 `image[]`，以及可选的 `mask`。JSON 引用可用链接或内联数据，共享网关凭证下无法引用供应商文件 ID。`stream: true` 通过 SSE 返回局部和最终图片。网关记录上游报告的用量，每个图片调用只向上游发送一次。配置了文本输出 Token 配额的分组需通过单独分组使用图片模型。

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

启用持久化用量后，网关会先保存上游报告的最终用量，再向客户端发送完成帧。若流在上游报告最终计数前结束，尝试记录保留已收到的计数。

Artisan 尝试列表的 `usage_state` 区分 `not_dispatched`（未出站）、`unknown`（未知）、`partial`（部分）、`final`（最终）和 `legacy`（历史）。明确零值保留原样。列表也会返回可获得的出站时间、上游请求 ID 和报告模型。已记录出站时间表示网关已进入发送步骤，中断的尝试仍可能需要对账。

### 文本计费 {#text-billing}

将 `billing.enabled` 和 `usage.enabled` 设为 true。发布并执行网关迁移。指定价格版本，按连接和上游模型配置成本费率，再按公开模型配置下游应收费率。

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

费率用十进制字符串表示，单位为指定币种每百万 Token。分别定价时配置 `cache_read`、`cache_write` 或 `reasoning`。每笔费用使用请求时保存的费率。缺少价格或用量证据时，金额可保留待核对。运行 `./artisan ai:gateway:usage:list --billing` 可查看成本、应收、价格版本及精确小数金额。每页在同一条数据库查询中读取用量、费用和账单证据。

在 `groups.<分组>.rate_multiplier` 设置非负十进制字符串，例如 `"0.5"`，可调整该分组的公开模型应收。倍率参与预占和结算，结算金额最后统一舍入。费率快照保留请求时使用的倍率。

每次上游尝试各有一条记录。回退尝试共用 `request_id`。记录包含下游 Key 或分组、公开模型、连接、上游模型、响应状态和供应商实际返回的 Token 数。缺失用量保留为 `null`。账本省略 Key 密钥、请求正文和生成文本。

在 `ai.providers` 中，为连接自有网关的 SDK 连接设置 `"forward_usage_context": true`。网关为每次实际文本请求保存 `consumer_invocation` 和从零开始的 `model_turn`。运行 `./artisan ai:gateway:usage:list --billing --invocation-id=<id>` 可查看相关尝试，并可用 `--invocation-scope=<scope>` 限定发起调用的 Key 范围。各模型回合有独立的网关请求 ID。关联标识不影响计费或请求去重。

网关先登记尝试，再向上游发送请求。正常结束、出错和取消都会结算。进程在结算之前退出时，账本可能保留 `pending` 记录供核对。

使用发起模型调用的 Key，以 Bearer 或 `x-api-key` 鉴权请求 `GET /v1/usage?days=7`。返回该 Key 当日、累计以及按 UTC 日期汇总的用量。`days` 可设为 1 至 90。回退只计为一次请求，同时按实际出站次数统计尝试。缺失的 Token 数保持 `null`。

`model_stats` 返回当前 Key 最近 30 天按公开模型名汇总的用量。可用 `start_date` 和 `end_date` 指定 `YYYY-MM-DD` 格式的 UTC 日期，包含首尾两天。省略或无法解析的日期使用相应默认值，此范围仅影响 `model_stats`。

托管 Key 轮换后保留历史。替换环境 Key 后，新凭证开始累积自己的历史。运维可通过 Artisan 账本查询早期记录和上游明细。

#### 导入供应商成本证据 {#import-cost-evidence}

用 `./artisan ai:gateway:billing:import invoice.json --actor=operator` 导入包含逐请求金额的供应商账单。命令按 UTC 时间窗口、命名连接和上游请求 ID 匹配。窗口包含开始时间，排除结束时间。

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

部分账单使用 `complete: false`。更正时沿用来源、账单 ID 和时间窗口并增加版本号。同版本同内容可重复导入。用量清单中的 `amount` 保留费率计算值，`actual` 展示有证据的供应商成本。币种不符或报告冲突会保留供运维核对。

### 供应商汇总账单 {#provider-cost-totals}

供应商按连接和币种给出汇总时，通过同一导入命令提交 `totals`。将供应商原始账期换算为 UTC 时间戳。Token 费用使用 `category: "tokens"`，其他费用分类保留原金额。

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

导入时，网关把每个汇总项与对应连接、币种和账期的账本比较。已确认的逐请求账单证据优先于该笔的费率核算值。结果记录供应商金额、本地记账金额、带正负号的差额以及已定价和待确认的尝试数。部分账单、待确定成本及未映射的费用分类保留供核对。汇总差额留在账单层级。

使用导入命令返回的 `report_id` 运行 `./artisan ai:gateway:billing:report <report-id>`，可查看已保存的明细、汇总差额和导入记录。

账单分成多个文件时，加入 `"page": {"number": 1, "total": 3}`，各文件的来源、账单 ID、版本、UTC 账期和 `complete` 保持一致。用同一命令导入各文件，顺序不限。可通过 `billing:report` 的 `pagination.received_pages` 找出缺页。网关在全部页收到后合并账单并记录证据。所有分页提交完毕且声明 `complete: false` 时，仍按部分账单处理。

### 借项与贷项调整 {#billing-adjustments}

运行 `./artisan ai:gateway:billing:adjust adjustment.json --actor=operator`，为已结算账目追加借项或贷项。`direction: "cost"` 用于上游成本，`"receivable"` 用于下游应收。使用原账目币种，`reference` 填支持凭据或行号。若依据是已完整导入的账单，可附上该账单的 `report_id`。

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

原始费率、金额和证据均保留。`usage:list --billing` 中的 `adjustments` 展示借项、贷项、净额和审计明细。以同一次尝试、同一方向及凭据编号提交相同内容可安全重试。原金额仍待确认时，净额保持待核对。汇总账单使用导入时可确认的净额。

### 分组每日货币预算 {#billing-budget}

在 `config/ai_gateway.rs` 启用用量和计费，并执行网关迁移。在 `groups.<分组>.budget` 设置预算。在 `billing.upstream.<连接>.<模型>` 中提供 `input_token_limit`，使用供应商确认的单次请求可计费输入 Token 上限，包含缓存 Token。下游费率使用与预算相同的币种。同组所有 Key 及共用同一数据库的网关实例共享预算。

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

网关限制出站生成的输出 Token，并按应收上界预占。成功后按记录的费率结算并释放差额。未形成下游应收的失败尝试释放应收预占。缺少最终用量或出站后进程停止时，预占留在原 UTC 账期。供应商实际回报导致应收超过声明上界时，该分组在本账期停止准入，实际费用仍可从账本查看。

运行 `./artisan ai:gateway:budget:show team` 查看当前 UTC 账期的限额、预占、已在限额内入账的金额和可用余额。余额不足返回 HTTP 429 和 `budget_exceeded`。无法确认上界的路由返回 `budget_unbounded`，也可通过 `unknown: "monitor"` 放行，并保留常规用量及计费。

进程重启后，可运行 `./artisan ai:gateway:budget:recover team` 释放已超时且未写入出站标记的预占。默认等待 300 秒，可用 `--after=秒数` 和 `--limit=条数` 调整扫描。恢复会在同一事务中关闭尝试及费用记录并释放预占。已写入出站标记的尝试继续保留供对账。

### 上游账号每日成本预算 {#upstream-cost-budget}

在 `accounts.<连接>.budget` 使用与分组预算相同的字段，币种须与上游费率一致。该账号的预算覆盖经过此连接的所有公开模型和分组。两种预算同时适用时，网关在出站前用同一事务预占上游成本和下游应收。

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

账号余额不足时，网关可选择另一个有权限的上游。所有可用账号的预算都不足时，返回 HTTP 429 和 `upstream_budget_exceeded`。失败尝试的上游成本无法确认时，仍保留成本预占。该次尝试的下游应收可独立记为零。

运行 `./artisan ai:gateway:budget:show openai --account` 查询账号余额，用 `./artisan ai:gateway:budget:recover openai --account` 按连接恢复超时未出站的尝试。一笔尝试同时预占两种预算时，恢复会一并释放。

供应商账单或借贷项确认费用后，运行 `./artisan ai:gateway:budget:reconcile <attempt-id> --direction=cost --reference=<凭据编号> --actor=operator`。核对下游应收时改用 `--direction=receivable`。命令在原预算账期内结算已确认金额。重复提交同一凭据编号会返回原记录，更正金额时使用新的凭据编号。费用超出原预占上界时，本账期停止新增调用，实际金额仍可核对。

运行 `./artisan help ai:gateway:budget:reconcile` 可按应用当前语言查看网关命令说明。应用可在 `lang/vendor/ai-gateway/<locale>/console.json` 中覆写 `commands.<命令>.description` 或 `commands.<命令>.arguments.<参数>`。命令名、参数名和选项名在各语言下相同。

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

## 文本生成选项 {#text-generation-options}

在 Agent 上设置输出 Token 上限与采样参数。匿名 Agent 用 `with_generation_options`，具名 Agent 实现 `Agent::generation_options`。这些选项也用于结构化输出和流式工具续步。普通响应的 Agent 中间件可覆盖当前步骤的选项。

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

驱动会生成供应商协议所需的字段。Anthropic 只向支持采样的 Claude 模型发送采样参数。两项都设置时，优先发送 temperature。

### 供应商扩展选项 {#provider-options}

匿名 Agent 用 `with_provider_options` 为具名连接设置扩展字段，具名 Agent 可实现 `Agent::provider_options`。仅所选连接的扩展字段会随请求发送，并保留在该连接的工具续步中。模型、消息、流模式及结构化 Schema 等字段通过 SDK 设置。Gemini 的 `generationConfig` 可以追加扩展字段。

```rust
use openrust_ai::{Agent, ProviderOptions, agent};
use serde_json::json;

let options = ProviderOptions::from_iter([("reasoning".into(), json!({"effort": "low"}))]);
let assistant = agent("Keep the answer concise.")
    .with_provider_options("openai", options);
let response = assistant.prompt("Summarize our return policy.").await?;
```

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

### 选择工具 {#tool-choice}

匿名 Agent 使用 `with_tool_choice`，具名 Agent 实现 `Agent::tool_choice`。`Auto` 让模型自行选择，`None` 禁止调用，`Required` 要求至少调用一个工具。`ToolChoice::tool(name)` 指定已注册的工具。强制调用和指定工具只作用于第一步，后续步骤可根据工具结果继续回答。

```rust
use std::sync::Arc;
use openrust_ai::{Agent, ToolChoice, agent};

let response = agent("Check inventory before answering.")
    .with_tools(vec![Arc::new(LookupInventory)])
    .with_tool_choice(ToolChoice::tool("lookup_inventory"))
    .prompt("Is this item in stock?")
    .await?;
```

### 相似度搜索工具 {#similarity-search}

`SimilaritySearch::using_model` 把工具收到的查询文字转成向量，再通过 Eloquent 查询文档并返回业务字段。需要附加应用查询条件时使用 `using_model_with`，也可以用 `using` 提供异步检索函数。`rerank` 接收从每条结果提取文本的函数和可选的返回条数，用已配置的重排连接排序。

生成 Schema 时，默认工具说明和 `query` 参数说明使用当前语言。可在 `lang/vendor/ai/<locale>/tools.json` 中覆写 `similarity_search.description` 和 `similarity_search.query`。显式调用 `with_description` 时，直接采用应用提供的说明。

```rust
use std::sync::Arc;
use openrust_ai::{Agent, SimilaritySearch, agent};

let search = SimilaritySearch::using_model::<PolicyDocument>("embedding", 0.6, 15)?;
let response = agent("Search the policy documents before answering.")
    .with_tools(vec![Arc::new(search)])
    .prompt("When can I return an order?")
    .await?;
```

### 将 Agent 作为工具 {#agent-as-tool}

用 `AgentTool::new(your_agent)` 包装子 Agent，再和普通工具一样传给父 Agent。默认使用子 Agent 的类型名，工具要求提供 `task` 字符串。实现 `CanActAsTool` 并调用 `into_tool()` 可显式指定名称和说明。子 Agent 实现了 `HasStructuredOutput` 时，可通过 `with_structured_output()` 将其 JSON 字符串作为工具结果。

子 Agent 使用自己的指令、模型和连接，父 Agent 只提交这项任务。即使父 Agent 正在流式输出，子 Agent 完成后也会一次返回工具结果。

默认工具说明和 `task` 参数说明在生成 Schema 时按当前应用语言及其回退配置解析。可在 `lang/vendor/ai/<locale>/tools.json` 中覆写 `agent.description` 或 `agent.task`。说明中的 `:agent` 会替换为工具名称。

### Agent 技能 {#agent-skills}

用 `Skill::from_directory` 打开包含 `SKILL.md` 的目录。YAML 元数据声明技能名称和描述，Markdown 正文提供说明。也可用 `Skill::new` 在 Rust 中定义技能，通过 `files` 提供内联资源。

通过 `LoadSkill::new` 将技能注册为 Agent 工具。工具描述列出技能名和描述。模型调用 `load_skill` 时，传 `name` 读取说明和文件清单，另传相对 `path` 读取不超过 256 KiB 的 UTF-8 配套文件。工具提示使用应用当前语言。

```rust
use std::sync::Arc;
use openrust_ai::{LoadSkill, Skill, agent};

let skill = Skill::from_directory("resources/skills/catalog")?;
let assistant = agent("").with_tools(vec![Arc::new(LoadSkill::new(skill))]);
```

### 工具审批策略 {#tool-approval}

实现 `Tool::should_request_approval` 可根据工具参数要求审批。当前调用没有可恢复的审批上下文时，SDK 在执行该步骤的任何工具前返回 `AiError::ApprovalNotResumable`。普通响应的错误携带待审批的调用 ID、工具名、参数和原因。

```rust
use openrust_ai::{Approval, ToolRequest};

fn should_request_approval(&self, request: &ToolRequest<'_>) -> Option<Approval> {
    let amount = request.get("amount")?.as_u64()?;
    (amount > 100).then(|| Approval::new(Some("Confirm this amount.".into())))
}
```

### 工具审批后恢复会话 {#resume-tool-approval}

用 `./artisan vendor:publish --tag=ai-migrations` 发布 AI 迁移，重新构建应用以注册迁移文件，再运行 `./artisan migrate --force`。实现 `RemembersConversations` 的 Agent 可以通过 `prompt_with_approvals` 在执行当前批次的任何工具前暂停。

```rust
use openrust_ai::{ApprovalResponse, Decision, RemembersConversations};

let result = agent
    .for_participant(participant.clone())
    .prompt_with_approvals(prompt, Default::default())
    .await?;

if let ApprovalResponse::Pending(pending) = result.response {
    let conversation_id = result.conversation_id;
    let approval_id = pending.id;
    // 已认证的参与者提交决定后，再恢复执行。
    let _result = agent
        .r#continue(conversation_id, participant)
        .resume_approval(&approval_id, Decision::approve_all())
        .await?;
}
```

保留会话 ID 和审批 ID。在同一已认证参与者和 Agent 下，用 `pending_approvals_for` 重新读取待审批项，再展示给用户。使用 `resume_approval` 提交每条请求的决定。`Decisions::new` 支持逐项批准、拒绝或编辑参数，也可为剩余项应用默认决定。

SDK 在恢复前核对参与者、Agent 和供应商连接，同一待审批批次仅能被一个请求领取。当前审批有效期为 24 小时。领取后的请求失败或中断时，工具可能已经执行，SDK 不会自动重放。当前审批快照支持文本提示词。

需要流式输出时，通过 `stream_with_approvals` 创建响应，并使用现有的 SSE 适配器。SDK 先保存快照，再发出审批事件。默认的 `tool_approval_request` 事件包含 `conversation_id` 与 `approval_id`。Vercel 在 `data-tool-approval` 中交付批次 ID，并为每个调用发送 `tool-approval-request`。AG-UI 在各中断项的元数据中交付 ID。用 `resume_approval` 提交决定，恢复会话并接收普通响应。

应用展示审批错误时，文案使用当前语言。可在 `lang/vendor/ai/<locale>/errors.json` 的 `approval` 下覆写各条消息。选择 HTTP 状态或决定是否重试时，可匹配 `AiError` 的具体变体。

监听 `openrust_ai::events::ToolApprovalRequested::NAME` 获取已保存的审批请求，监听 `ToolApprovalResolved::NAME` 获取已保存的处理结果。类型化事件携带会话、参与者、Agent 类型、调用和审批标识。处理完成事件提供该批审批的工具结果。

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

使用 Gemini 原生 API 时，设置 `AI_EMBEDDINGS_PROVIDER=gemini` 与 `GEMINI_EMBEDDING_MODEL`。SDK 通过 `batchEmbedContents` 发送文本，单条也使用同一入口。模型 ID 可带或省略 `models/` 前缀。结果按输入顺序返回，指定的维度会传给上游，Token 数量以 Gemini 实际报告为准。

发布 Gemini 原生向量模型时，将模型映射指向 Gemini 连接，并显式配置 `"endpoints": ["embedContent", "batchEmbedContents"]`。授权模型会出现在 `/v1beta/models`。通过 `x-goog-api-key` 调用 `POST /v1beta/models/{alias}:embedContent` 或 `POST /v1beta/models/{alias}:batchEmbedContents`，采用 Google 请求格式。正文中的模型名必须与发布别名一致。网关会替换为上游模型，输入可直接内联。

调用 `cache(Some(600), None)` 可将向量缓存 600 秒。在 `ai.caching.embeddings` 设置默认有效期、缓存存储以及是否为全部 SDK 调用启用缓存。逐条缓存能跨批次复用向量，只把未命中的输入发给供应商。第二个参数传 `Some(false)` 会按原顺序缓存整批。单次调用可用 `cache(Some(0), None)` 跳过缓存。部分命中时，用量只记录实际发给上游的输入。

### 使用文本查询向量 {#querying-embeddings}

导入 `ToEmbeddings` 可从字符串生成一个向量。查询构建器与 Eloquent 也可通过配置好的 SDK 连接把搜索文字转成向量。使用 PostgreSQL 时，启用 pgvector，并让向量列的维度与嵌入模型一致。

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

两条路径都使用 `ai.caching.embeddings` 的缓存设置。应用已注册自己的数据库 Embedding Provider 时，可继续使用该服务。

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
