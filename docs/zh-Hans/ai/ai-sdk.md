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

在回调中读取 `result.response.steps`，可取得每步的文本、公开推理摘要、引文、工具调用和结果。每步提供供应商报告的模型、用量和结束原因。供应商未报告模型时，沿用该步流开始事件中的模型。暂停等候审批的步骤带有工具调用和已报告的用量。响应使用最后一步的模型，并将暂停步骤中已报告的用量计入汇总。

OpenAI Responses 文本流的网页引文在原生 SSE 中使用 `citation` 事件，在 Vercel 中使用 `source-url`，在 AG-UI 中使用名为 `citation` 的 `CUSTOM` 事件。原生引文带有对应的文本消息 ID。可在完成回调中通过 `StreamedAgentResponse::citations()` 按顺序读取引文和上游报告的文本位置。 工具续轮会按事件顺序保留各次模型调用的引文。

Anthropic Messages 流通过同一套原生 SSE、Vercel 和 AG-UI 格式发送网页搜索与抓取引文。每个模型步骤发送每个 URL 的首次出现，并保留公开标题。 Gemini GenerateContent 流也通过这些格式发送选中候选的 Grounding 网页来源。同一模型步骤内，跨帧重复的来源只发送一次。

OpenAI Responses 的普通响应可通过 `response.meta()` 读取来源连接、模型和网页引文。引文沿用上游顺序与各文本分段报告的位置。结构化响应也提供该方法。自定义文本驱动可用 `AgentResponse::new` 创建响应，再填写引文。 非流式工具调用的响应元数据取自最后一个模型步骤。

内置文本驱动的非流式响应通过 `response.steps` 提供各个模型步骤。每步包含自己的模型、用量、结束原因、公开摘要和引文，以及本步的工具调用和结果。顶层用量汇总整个调用。

非流式 Anthropic Messages 提供网页搜索和抓取引文。非流式 Gemini GenerateContent 提供选中候选的 Grounding 网页来源。两者保留每个 URL 首次出现的顺序和公开标题。

Responses 模型支持公开推理摘要时，可通过 `with_provider_options` 将 `reasoning.summary` 设为 `auto`。摘要片段在原生 SSE、Vercel 和 AG-UI 中使用独立事件。可在完成回调中用 `StreamedAgentResponse::reasoning()` 读取合并后的摘要。 非流式 Responses 在 `response.reasoning` 中提供公开摘要，结构化响应同样可读。非流式工具调用提供最后一个模型步骤的公开摘要。

Vercel 和 AG-UI 流式适配器使用当前应用语言显示生成失败提示。可在 `lang/vendor/ai/<locale>/errors.json` 下覆写 `stream` 文案。

使用 `agent(instructions).with_schema(schema).prompt(text)` 请求 JSON 对象。Schema 用框架已有的 `JsonSchema` 构建。可读取 `response.structured`，或通过 `response.decode::<YourType>()` 转为 Rust 类型。所选模型须支持连接的原生结构化输出格式。结构化流将 JSON 作为文本增量发出。可用 `./artisan ai:prompt` 的 `--schema` 传入 JSON 对象 Schema 试调用。 SDK 会在出站 Schema 中禁用对象的额外属性，包括嵌套对象。如果模型执行 OpenAI 严格 Schema 校验，请将所有属性标记为必填。

当前请求或命令需要临时连接时，可用 `Ai::build(config)`，字段沿用具名连接配置。通过返回的 `name()` 在提示词调用中指定连接，或在同一作用域内通过 `Ai::text_provider` 查找。框架保留名及应用已有名称不能用作临时连接。`Ai::flush_state()` 会清除当前作用域的临时连接。

### 供应商故障转移 {#provider-failover}

普通文本、结构化输出和文本流可以按顺序尝试多个具名连接。匿名 Agent 用 `with_providers`，自定义 Agent 可实现 `Agent::providers`。每个连接可指定模型，省略时使用该连接的模型档位。

```rust
use openrust_ai::{Agent, ProviderTarget, agent};

let responder = agent("Answer briefly.").with_providers([
    ProviderTarget::new("openai").with_model("first-model-id"),
    ProviderTarget::new("deepseek"),
]);
let answer = responder.prompt("What can I do next?").await?;
```

确定建连未成功、限流、费用不足或指定的上游过载状态可触发切换。请求超时、发送后的断连和收到成功响应后首帧中断时，SDK 返回错误，避免自动重发可能已被上游处理的请求。应用可监听 `openrust_ai::events::AgentFailedOver`，用调用 ID 关联已尝试的连接。使用 `PromptOptions.provider` 显式指定连接会仅调用该连接。流在交付首个事件后固定使用当前连接。带工具的流可在启动时切换，工具结果和后续模型步骤留在选中的连接。非流式工具调用可在首个模型步骤失败时切换。首个成功的模型步骤确定连接，后续错误会返回给调用方。使用提示词中间件的调用当前仅使用首选连接。

内置文本协议遇到限流或指定的过载状态时，如果上游的 `Retry-After` 是 0 到 10 秒的整数，SDK 会等待后重试当前请求一次。上游报告额度或余额耗尽时直接按故障转移规则处理。切换事件只记录连接切换，同连接重试沿用原调用 ID。

Anthropic Messages 的 HTTP 529 可触发重试或切换。Messages 返回符合固定 Laravel 基线的额度错误时也可切换。发生这两类错误时，应用可分别匹配 `AiError::ProviderOverloaded` 和 `AiError::ProviderInsufficientCredits`，获取 HTTP 状态。原始错误正文不会进入应用错误。

### Agent 提示事件 {#agent-prompt-events}

非流式提示在每次供应商尝试前分派 `PromptingAgent`，随后分派携带响应的 `AgentPrompted` 或携带类型化错误的 `AgentFailed`。通过框架的 `Event` 门面按各事件的 `NAME` 注册监听器。每次尝试及 `AgentFailedOver` 共用调用 ID。`event.prompt.as_prompt()` 可读取当前连接在步骤中间件运行前的请求参数。

流式调用在每次供应商尝试启动前分派 `StreamingAgent`。完整消费的流分派携带结果的 `AgentStreamed`，启动或读取出错时分派 `AgentFailed`。暂停等候审批时会带上已保存的审批批次。持久会话先写入再分派成功事件，事件带会话 ID。请在发起请求前注册监听器。客户端提前停止消费时，可能收不到成功事件。

非流式与流式生成均可监听 `StartingStep`、`StepCompleted` 和 `StepFailed`，查看实际发起的模型步骤。步骤编号从零开始。起始事件携带已解析的请求及此前的工具结果。请求快照包含步骤中间件的调整。完成事件在本步工具执行前分派，携带供应商报告的模型、用量和结束原因。中间件直接返回响应时不会产生模型步骤事件。

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

## Live WebRTC 连接 {#live-webrtc}

在使用 `openai` 驱动的具名连接中加入 `"live": "openai"`，接入 OpenAI 公开 Live API，并用 `key_env` 配置 Bearer 凭证。接入已授权的 Codex Live 时，使用 `"live": "codex"`，将 URL 指向对应 Codex API 基础地址，并配置上游要求的账号请求头，其中 `x-oai-attestation` 由部署方获授权的证明服务提供。凭证和证明值通过环境变量注入配置。

用客户端的 SDP offer 和会话对象创建呼叫。将返回的 SDP answer 交给 WebRTC 对端连接。用返回的呼叫 ID 在同一个具名连接上建立 Sideband WebSocket，接收服务端事件并发送控制指令。OpenAI 公开 Live 适配也提供 `hangup_live_call`。

```rust
use openrust_ai::{Ai, AiError};
use serde_json::json;

let provider = Ai::text_provider(Some("voice"))?;
let live = provider.live_gateway().ok_or(AiError::EndpointNotSupported)?;
let session = json!({"model": model});
let call = live.create_live_call(&offer_sdp, &session).await?;
let control = live.connect_live_sideband(&call.id).await?;
```

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

两种计数入口也可使用 Bedrock 连接，前提是所选模型和区域支持运行时的 `CountTokens` 操作。要同时发布两条路径，在网关模型的 `endpoints` 中加入 `responses/input_tokens` 和 `messages/count_tokens`。两种入口均接收文本、系统指令和客户端函数历史。Messages 计数还接收 Bedrock Converse 支持的内联 Base64 附件，含文档的消息须同时包含文本块。网关记录出站尝试，生成用量、计费金额和货币预算余额保持原值。发布前可核对 AWS 的模型支持情况。[Amazon Bedrock Token 计数](https://docs.aws.amazon.com/bedrock/latest/userguide/count-tokens.html)

在 `config/ai_gateway.rs` 将 `unversioned_paths` 设为 `true` 即可启用下列别名。别名与对应的 `/v1` 路由共用授权、限流、模型别名和用量记录。修改后请重新生成路由缓存。

| 能力 | 无版本路径 |
| --- | --- |
| 目录与推理 | `GET /models`、`GET /models/{model}`、`POST /chat/completions`、`POST /responses`、`GET /responses`（WebSocket）、`POST /responses/input_tokens`、`POST /responses/compact`、`POST /embeddings`、`POST /messages/count_tokens` |
| 图片 | `POST /images/generations`、`POST /images/edits` |
| xAI 语音 | `POST /tts`、`POST /stt`、`GET /realtime`（WebSocket）、`/custom-voices` 下的[自定义音色操作](#gateway-custom-voices) |
| 搜索 | `POST /web_search`、`POST /x_search`、`POST /alpha/search` |

两个方向的转换都支持普通 JSON 和 SSE，给客户端返回公开模型名。上游报告缓存读写用量时，网关会按目标协议返回相应计数。

同一个 Anthropic 发布模型可供 Messages、Responses 和 Chat Completions 客户端调用。转换支持文本、系统指令、基本采样参数和客户端函数工具，支持 JSON 和 SSE。工具调用 ID、流式参数增量和文本结果可双向传递。结构化输出和附件使用相应的原生端点。

Gemini 客户端使用 `/v1beta` API，通过 `x-goog-api-key` 提交下游 Key。网关列出授权的 Gemini 模型，并支持 `/v1beta/models/{model}` 下的 `:generateContent`、`:streamGenerateContent` 和 `:countTokens`。在 SDK 或 Artisan 中选择 `gemini` 连接，模型 ID 可带 `models/` 前缀。

`:streamGenerateContent` 默认以 JSON 数组流式返回。可用 `?alt=json` 显式选择 JSON，或用 `?alt=sse` 选择 SSE。数组中的每一项是一个 Gemini 响应块。流开始后发生错误时，数组末尾会提供 Gemini 错误对象。

`:countTokens` 可以直接接收 `contents`，也可以在 `generateContentRequest` 中接收完整的提示配置，包括系统指令、客户端函数、生成设置和可选的网关 `cachedContent`。内层指定 `model` 时须与路径中的公开模型一致。每次请求使用其中一种形式。返回的 `totalTokens` 和可选的 `cachedContentTokenCount` 保留上游的计数，并独立于生成用量。

发布 Gemini 上下文缓存时，启用网关用量存储，运行网关迁移，并在 Gemini 生成模型上设置 `"gemini_cached_contents": true`。向 `POST /v1beta/cachedContents` 提交缓存内容，`model` 填公开模型名，例如 `models/support-chat`，在 `x-goog-api-key` 中传入网关 Key。用 `GET /v1beta/cachedContents` 列出当前 Key 的缓存，支持 `pageSize` 和 `pageToken`。单个缓存通过 `/v1beta/{name}` 的 GET、PATCH 和 DELETE 管理，PATCH 可更新 `ttl` 或 `expireTime`。

在 `:generateContent` 或 `:streamGenerateContent` 中，把网关返回的 `name` 作为 `cachedContent`，沿用同一个 Key 和公开模型。网关会把请求发给创建缓存的连接。创建时可使用内联内容和客户端函数定义。生成用量按配置的缓存输入费率结算，缓存创建和存储费用仍需供应商账单证据。分组或账号预算要求拒绝上界未知的费用时，网关会在出站前拒绝创建和修改有效期。

网关将创建、修改有效期和删除分别记为缓存生命周期操作。`ai:gateway:usage:list --billing` 通过 `resource_id` 关联生成记录，并显示上游请求 ID 与上游报告的资源用量。创建和有效期更新的上游费用在取得账单证据前保持待确认，生命周期操作不产生下游应收。账单包含请求 ID 时，可用 `ai:gateway:billing:import` 导入对应证据。只有 SKU 汇总的存储费用应单独填写 `category`。网关保留汇总金额，不将其分配给单次操作或加到生成 Token 的汇总中。

导入 Gemini 存储 SKU 汇总时，将该条的 `category` 设为 `gemini_context_cache_storage`。在模型的上游费率中设置 `cache_storage_per_million_token_hours`，单位是每百万缓存 Token 每小时的金额。对于已结束的账期，网关将已确认的缓存有效区间裁剪到报告窗口，按创建和有效期更新时的费率快照计算，汇总后只舍入一次。`storage.estimated` 是估算金额，`storage.estimated_difference` 是账单金额减去估算金额。缺少时间、费率或生命周期证据时，受影响的资源保持待确认。估算仅用于 SKU 汇总对照。

发布 Gemini 异步批任务时，启用网关用量存储，发布并运行 `ai-gateway-migrations`，再将 `batchGenerateContent` 或 `asyncBatchEmbedContent` 加入公开模型的 `endpoints`。将批任务提交到 `POST /v1beta/models/{alias}:batchGenerateContent` 或 `POST /v1beta/models/{alias}:asyncBatchEmbedContent`。使用网关返回的 `batches/…` 名称查询、取消或删除任务。`GET /v1beta/batches` 列出当前 Key 的任务，支持 `pageSize` 和 `pageToken`。各入口均使用 `x-goog-api-key`。

请按批任务价格配置具名连接和公开模型的费率。网关只登记一次提交，取得最终条目用量后结算。启用硬预算时，为上游模型设置可信的单项输入上界 `input_token_limit`，向量批次也按此上界预占。启用硬预算或输出配额的生成批次支持文本输出。网关取最严格的配置上限，按每项的候选数换算 `maxOutputTokens`，保留调用方更小的上限和原有推理设置。候选输出和推理 Token 均计入配额。用量无法确认时保留配额预占。Gemini 返回结果文件时，网关在 `response.responsesFile` 中返回当前 Key 有权访问的文件名。用 `GeminiBatches::download`，或持同一 Key 请求 `GET /download/v1beta/{responsesFile}:download?alt=media`。结果以 JSONL 流式返回，单行最大 64 MiB。下载中断后从头重试。

不超过 8 MiB 的 JSONL 可用 `Files::put`，也可通过 `POST /upload/v1beta/files` 启动会话后一次发送 `upload, finalize`。较大的文件用返回的上传 URL 逐片发送 `upload`，用 `X-Goog-Upload-Offset` 指定字节偏移。中断后发送 `query`，从 `X-Goog-Upload-Size-Received` 指示的位置继续。最后一片可用 `upload, finalize`，也可在全部上传后以文件总字节数为偏移发送空的 `finalize`。网关每个文件最多接收 32 MiB，每次请求最多 8 MiB，每个文件最多 256 片。分片保存在私有 `local` 磁盘，多进程部署时共用该磁盘。持同一 Key 将返回的 `files/input-…` 填入 `batch.inputConfig.fileName`。通过 `/v1beta/{fileName}` 可读取元数据或删除暂存文件。网关核验文件并应用配置的限额，再将 JSONL 上传到选定连接。批任务受理后即可删除暂存文件。

上传会话与暂存文件各保留 48 小时，暂存文件从上传完成起计时。用 `./artisan ai:gateway:batch-files:prune --pretend` 预览清理，去掉 `--pretend` 即可分批清理过期文件和元数据。在 `routes/console.rs` 登记同一命令可定时清理。

```rust
Schedule::configure(|schedule| {
    schedule
        .command("ai:gateway:batch-files:prune", [])
        .cron("0 * * * *")?
        .without_overlapping();
    Ok(())
})?;
```

运行[任务调度器](../digging-deeper/scheduling.md)的进程应与网关共用私有磁盘和用量数据库。

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

### Responses 会话压缩 {#responses-compact}

发布 OpenAI 协议模型时配置 `"endpoints": ["responses", "responses/compact"]`。向 `POST /v1/responses/compact` 发送公开模型名和完整的内联会话 `input`。Codex 可使用 `/backend-api/codex/responses/compact`，开启 `unversioned_paths` 后也可使用 `/responses/compact`。

保留响应的完整 `output` 数组，用作下一次 Responses 请求的输入，并在后面追加新消息。OpenRust 将加密压缩项绑定到签发时的网关 Key、公开模型和上游连接。请完整保留压缩项，负责后续请求的网关进程须配置相同的应用加密密钥。

压缩同步返回并记录上游报告的用量。该路由使用内联上下文，拒绝上游存储的响应 ID、后台调用和共享上游资源引用。

普通 Responses 请求可设置 `"context_management": [{"type": "compaction", "compact_threshold": 200000}]`，也可在输入末尾追加一个 `{"type": "compaction_trigger"}` 立即触发压缩。模型须同时发布 Responses 和 Compact。网关对 JSON、SSE 和 WebSocket 中完成的压缩项使用同一归属保护。下一次请求可内联传回这些项。

### Responses 持久响应 {#responses-state}

在使用原生 OpenAI Responses 连接的公开模型上设置 `"responses_state": true`，创建请求传入 `"store": true`。JSON、SSE 和 WebSocket 均返回不透明的 `resp_or_` 标识。使用同一网关 Key 和公开模型，把该标识作为 `previous_response_id` 即可通过 HTTP 或 WebSocket 续接。需要保存新响应时再次设置 `store`。

通过 `GET /v1/responses/{response_id}` 读取已保存的响应，通过 `DELETE /v1/responses/{response_id}` 删除。读取返回 JSON，并隐藏上游的父响应标识。各网关实例应使用同一应用加密密钥和上游凭证。响应由上游连接保存，保留期限由上游决定。此设置适用于正常生成时保存响应。流式读取及共享上游资源引用另行处理。

通过 `GET /v1/responses/{response_id}/input_items` 查看已保存的输入，支持 `limit`（1–100）、`order`（`asc` 或 `desc`）和 `include`。当 `has_more` 为真时，在同一响应的下一次查询中把返回的 `last_id` 传给 `after`。网关转发前校验游标是否属于该响应和 Key。

### Responses 后台执行 {#responses-background}

开启网关用量持久化。对已启用 `responses_state` 的模型，在 Responses JSON 请求中传入 `"background": true`。上游状态为 `queued` 或 `in_progress` 时，使用同一 Key 通过 `GET /v1/responses/{response_id}` 继续查询。通过 `POST /v1/responses/{response_id}/cancel` 取消。使用同一数据库、应用加密密钥和上游凭证的网关实例可以接续查询和取消。

网关在读到终态后结算原生成尝试，查询和取消不会另记生成尝试。观察到终态后可以删除响应。上游决定保留期限，包括 `store: false` 时的临时可用期限。后台流式传输、完成时间窗口选项及自动轮询另行处理。

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

使用 xAI 原生 Responses 时，在公开模型上启用所需工具。`code_interpreter` 使用 xAI 沙箱。`file_search` 只能检索 `file_search_collections` 授权的集合，请求中的每个集合 ID 必须精确匹配。下游 Key 还须有权访问支持全部所请求工具的原生连接。工具费用上限未知时沿用预算配置。启用硬预算时默认拒绝这类请求，设置 `unknown: "monitor"` 可放行并保留计费观察。

```rust
"grok-tools": {
    "provider": "xai",
    "model": "grok-4.7",
    "endpoints": ["responses"],
    "server_tools": ["web_search", "code_interpreter", "file_search"],
    "file_search_collections": ["collection-id"],
},
```


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

发布 Bedrock 文生图模型时，只列出 `images/generations`。响应通过 `data[].b64_json` 返回 Base64 PNG。Titan Image Generator 单次 AWS 调用最多生成五张图，竖图和横图分别映射到 Bedrock 的 `768x1152` 和 `1152x768`。Stability Stable Image 使用宽高比和模型默认质量，每次生成一张。Bedrock 生成使用普通 JSON 响应。

启用 Bedrock 图片计费前，发布并执行最新的 `ai-gateway-migrations`。在上游和下游费率中设置 `image`，用十进制字符串表示对应币种的每图单价，例如 `"image": "0.04"`。按尺寸和质量分档时，配置 `image_tiers`，例如 `{"768x1152/premium": "0.08"}`。网关以实际发给 Bedrock 的参数选档，Stability 使用 `3:2/default` 这类键。未命中时回退到 `image` 固定费率。两项都缺失时，该方向金额保留待核对，硬预算也无法按该方向预占。账本保存选中的 `image_tier` 和单价。只按图计费时将 `input` 和 `output` 设为 `"0"`。网关按请求的 `n` 预占，按实际返回且通过验证的图片数结算。已知数量会出现在用量账本及 `/v1/usage` 的 `output_images`。未知供应商费用保留待核对，可用 AWS 账单对账。

### 网关 Live 与 Sideband {#gateway-live}

配置 [Live 供应商连接](#live-webrtc)，在 `config/ai_gateway.rs` 以 `"endpoints": ["live"]` 发布模型。启用 `ai_gateway.usage.enabled` 并执行 `ai-gateway-migrations`。将公开模型别名授权给调用方的网关 Key。硬预算或输出配额当前无法预占 Live 用量，相应请求会被拦截。

用 Bearer 网关 Key 调用 `POST /v1/live`，在 JSON 或 multipart 中提交 `sdp` 和 `session`。用 `session.model` 选择公开别名。Key 只有一个可用 Live 别名时可省略该字段。响应体是 SDP answer，`Location` 是网关的 Sideband 路径。Codex 客户端可通过 `POST /backend-api/codex/realtime/calls` 调用 Codex Live 连接。

Codex 0.170.0 使用 WebRTC 时，将呼叫基础 URL 配置为网关的 `/backend-api/codex` 前缀。V3 的 Sideband 也使用该前缀，V1 则追加 `/realtime`。网关签发带 `rtc_` 前缀的本地呼叫 ID。V3 通过 `GET /backend-api/codex/{call_id}` 接入，V1 使用 `GET /backend-api/codex/realtime?call_id={call_id}`。两者都须携带创建呼叫时的网关 Key。

用同一个网关 Key 连接 Sideband 路径。每个会话同时只能有一个控制连接。原控制连接断开并释放后，可以使用同一个 Key 接回会话，包括从另一个网关进程接回。会话固定绑定原上游连接和模型，一小时后过期。`ai-gateway-live` 运行器负责观察无人控制的会话、向获授权的客户端让出控制权，并在进程重启后回收过期呼叫。至少在一个网关进程启用该运行器。每个进程最多并行观察 32 个会话。更换上游连接或凭证前，应先结束现有会话，让运行器可以使用原账号完成回收。

从 `Location` 取得呼叫 ID，通过 `./artisan ai:gateway:usage:list --request-id=<call-id>` 查询结果。网关只记一次终态，网关观察到的会话时长保存在 `resource_usage.session_duration_ms`。供应商用量和金额仍可能未知。会话时长和转录音频时长分别记录。 查询 Codex 呼叫的用量时，先去掉 ID 的 `rtc_` 前缀。

### 网关 xAI Realtime {#gateway-xai-realtime}

在 `config/ai_gateway.rs` 用 `"endpoints": ["realtime"]` 发布 `xai` 连接及上游语音模型，再向网关 Key 授权公开别名。使用 Bearer Key 升级 `GET /v1/realtime?model=<别名>` 为 WebSocket。Key 只有一个 Realtime 别名时可省略 `model`。启用 `unversioned_paths` 后也可使用 `GET /realtime`。

连接内使用 xAI 原生 JSON 事件和音频帧，同一连接可以完成多轮生成。发出 `response.cancel` 取消当前生成，收到对应的 `response.done` 后可开始下一轮。网关固定该连接的模型和上游账号，按授权校验音色和工具，连接最长一小时。SDK 直连可用 `Ai::text_provider(...).xai_realtime_gateway().connect_xai_realtime(...)` 选择具名 xAI 连接。

启用持久化用量后，`ai:gateway:usage:list` 在 `resource_usage.billable_audio_duration_ms` 显示供应商报告的最大累计计费时长。累计时长增加时即持久化，重复终帧不重复加时。这里记录观察到的用量证据。配置了硬预算或输出配额而需要预占用量时，网关会拒绝升级。

### 网关语音合成 {#gateway-speech}

要开放 `POST /v1/tts`，为 `xai` 连接发布一个模型，并向调用方的网关 Key 授权。可在 `config/ai_gateway.rs` 的 `models` 下加入以下示例。

```rust
"speech": {"provider": "xai", "model": "tts", "endpoints": ["tts"]}
```

使用 Bearer 网关 Key，提交 xAI 的 `text` 和 `language` JSON 字段。Key 恰好能访问一个语音合成别名时，网关会自动选择它。有多个可用别名时，在请求的 `model` 字段指定公开模型名。网关向 xAI 发送原生请求，流式返回 MP3、WAV、PCM、μ-law（`mulaw`）或 A-law（`alaw`）音频。结算后结束 HTTP 响应，取消下载时关闭上游连接。设置 `with_timestamps: true` 会收到带 Base64 音频及字符级 `audio_timestamps` 的 JSON 响应。可使用 xAI 内置音色或[当前 Key 名下的自定义音色](#gateway-custom-voices)。启用 `unversioned_paths` 后也可调用 `POST /tts`。

启用持久化用量后，Artisan 按尝试显示 `input_characters`。调用方的 `/v1/usage` 汇总已出站的字符数，包含失败尝试。网关按原始 `text` 的 Unicode 标量值计数，包含空格和标签。发音替换和文本规范化不改变计数。

在上游和下游费率中分别设置 `speech_per_million_characters`，单位为所选币种每百万字符，使用十进制字符串。仅供合成的费率可将 `input` 和 `output` 设为 `"0"`。硬预算按所选费率在出站前预占。失败尝试可能继续占用上游预算，直到成本对账完成。导入供应商合成汇总账单时使用 `category: "text_to_speech"`。

### 网关自定义音色 {#gateway-custom-voices}

发布 xAI [语音合成别名](#gateway-speech)，启用 `ai_gateway.usage.enabled`，发布并执行最新的 `ai-gateway-migrations`。音色管理与合成使用同一个 Bearer 网关 Key。上游连接须有 xAI Custom Voices API 权限。xAI 目前要求 Enterprise 账号开放 API 创建，并有地区限制。xAI 接收最长 120 秒的参考片段。

- `POST /v1/custom-voices` 接收 multipart 参考音频 `file` 和 `name`、`language` 等可选的 xAI 元数据。Key 可用多个合成别名时，添加 `?model=<公开别名>`。网关经私有临时文件分块上传音频，数据库只保存归属及上游引用。
- `GET /v1/custom-voices` 列出当前 Key 名下的音色。`limit` 支持 1 至 1,000，默认为 100。使用返回的 `pagination_token` 获取下一页。
- `GET /v1/custom-voices/{id}` 读取元数据，同一路径的 `PATCH` 更新指定字段。省略字段会保留原值，传 `null` 会清空该字段。GET 路径加 `/audio` 可流式下载原始样本。`DELETE` 删除音色并返回 `{"deleted":true}`。
- 将返回的 `voice_id` 和同一个合成别名传入 `POST /v1/tts`。音色固定使用创建时的连接和上游账号。更换连接凭证后，该目标无法访问原音色。其他 Key 即使共享上游凭证也无法访问这条音色。

启用 `unversioned_paths` 后，也可通过 `/custom-voices` 调用上述操作。

### 网关语音转录 {#gateway-transcription}

在 `config/ai_gateway.rs` 为 `xai` 连接配置上游转录模型和 `"endpoints": ["stt"]`，并向网关 Key 授权公开别名。`POST /v1/stt` 接收 Bearer Key 和一个 multipart `file`。音频文件最大为 500 MB（500,000,000 字节），multipart 正文总上限为 500 MB 加 256 KiB。支持带声明长度的请求和 HTTP 分块上传。录音在应用私有存储中暂存，请求成功、失败或断开后关闭临时文件。Key 只有一个转录别名时可以省略 `model`。有多个时，在 multipart 的 `model` 字段指定公开别名。启用 `unversioned_paths` 后也可调用 `POST /stt`。

网关保留重复的 `keyterm` 和 `redact` 字段，将音频文件放在最后上传。原始 PCM、μ-law 和 A-law 须指定 `audio_format` 和受支持的 `sample_rate`。响应保留上游的时间戳、说话人编号和音频时长。启用持久化用量后，Artisan 和调用方的 `/v1/usage` 可查询上游报告的 `audio_duration_ms`。拒绝未知费用的硬预算会在出站前拦截请求。

在上游和下游费率中分别设置 `audio_per_hour`，即可按上游报告的音频时长结算 xAI 转录。包含 `redact` 字段时，两侧费率还需配置 `audio_redaction_per_hour`。显式填 `"0"` 表示这项附加费用为零。费率使用十进制字符串，单位为所选币种每音频小时。仅供转录的费率可把 `input` 和 `output` 设为 `"0"`。缺少费率或时长时，对应金额保留待核对。导入供应商汇总账单时使用 `category: "speech_to_text"`，即可按转录类别单独对账。

### xAI 独立搜索 {#gateway-xai-search}

将 xAI 连接发布为网关模型，在 `endpoints` 和 `server_tools` 中分别启用要开放的搜索操作。若该模型也提供普通推理，在 `endpoints` 中加入 `responses`。

```json
{
  "provider": "xai",
  "model": "<xAI model ID>",
  "endpoints": ["web_search", "x_search"],
  "server_tools": ["web_search", "x_search"]
}
```

携带网关 Bearer Key 发送 `POST /v1/web_search` 或 `POST /v1/x_search`，正文示例为 `{"query":"Rust release notes","max_results":5}`。`input` 也可用于提供查询。若当前 Key 可访问多个支持该操作的模型，须填入公开模型名 `model`。结果数量默认 5 条，最多 20 条。无版本别名须启用 `unversioned_paths: true`。

X Search 可设置 `allowed_x_handles`、`excluded_x_handles`、`from_date`、`to_date`、`enable_image_understanding` 和 `enable_video_understanding`。两个入口均返回 `query`、`results`、`provider` 和 `max_results`。每项结果包含有来源证据的 HTTP(S) `url`、`title` 和 `snippet`。

开启网关用量后，成功的搜索记录 `resource_usage.search_requests` 及上游报告的 Token 数和费用。要求确认费用上限的分组或账号预算会在出站前拒绝此类请求。

### TypeSafe System One 网关 {#gateway-system-one}

通过现有 `typesafe` 连接发布 `systemone` 模型。在网关配置 `TYPESAFE_API_KEY`，客户端携带网关 Bearer Key 并使用公开模型名。

```json
{
  "public-jev": {
    "provider": "typesafe",
    "model": "jev-latest",
    "endpoints": ["systemone"]
  }
}
```

发送 `POST /v1/systemone`，正文包含 `model`、字符串或对象或数组类型的 `state`，以及按问题名组织的非空 `questions` 对象。网关支持同步 `noul`、`choice` 和 `score`。图片资产可通过 SDK 直连 TypeSafe。

```json
{
  "model": "public-jev",
  "state": "I would like to return my order.",
  "questions": {
    "return": {"type": "noul", "instructions": "这是退货请求吗？"}
  }
}
```

应用通过 `Classification` 调用网关时，使用 `system-one` 驱动，将连接 URL 设为以 `/v1` 结尾的网关地址，配置网关 Key 及公开分类模型名。要求确定输出 Token 或费用上限的策略会在出站前拒绝请求。

### Codex 独立 Alpha Search {#gateway-alpha-search}

使用支持原生 Alpha Search 的 OpenAI 连接，将 URL 设为上游提供的基础前缀，例如获准使用的 `/v1` 或 `/backend-api/codex`。将公开模型的 `endpoints` 配置为包含 `alpha/search`，并在 `server_tools` 中允许 `web_search`。

```json
{
  "provider": "openai",
  "model": "<upstream model ID>",
  "endpoints": ["alpha/search"],
  "server_tools": ["web_search"]
}
```

携带网关 Bearer Key 访问 `POST /v1/alpha/search` 或 `POST /backend-api/codex/alpha/search`。启用 `unversioned_paths` 后可访问 `POST /alpha/search`。在 Codex 搜索请求中填入公开模型名。

网关保留上游搜索结果及加密输出。启用用量持久化后，记录成功的独立搜索请求及上游报告的 Token。此模式要求上游凭证具有 Alpha Search 权限。

已获准通过 Responses 使用托管网页搜索的 Codex PAT，可在该连接的 `driver`、`url`、`key_env` 旁配置 `"alpha_search": "responses"`。客户端仍使用原有 Alpha Search 路径和公开模型名。网关访问连接的 `responses` 路径，确认收到成功完成事件后将文本和引用转换成 Alpha Search JSON。连接模式默认为 `native`。

Codex 0.170.0 使用自定义 Responses 提供商时，将基础 URL 设为以 `/backend-api/codex` 结尾，配置 `supports_standalone_web_search = true` 和引用网关 Key 的 `env_key`。启用 `features.standalone_web_search`，将 `web_search` 设为 `cached` 或 `live`。Codex 使用同一连接生成模型响应和执行 `web.run`，公开模型的 `endpoints` 也须包含 `responses`。

### Antigravity 网关路径 {#gateway-antigravity}

配置获准访问 Antigravity 兼容上游的 Anthropic 和 Gemini 协议连接，在 `config/ai_gateway.rs` 的 `accounts` 中为对应连接设定 `"platform": "antigravity"`，再发布使用这些连接的模型别名。

专用路径仅调度已标记的账号。账号设置 `"mixed_scheduling": true` 后，也可供普通网关路径使用。两种路径都遵循 Key 和分组的模型及连接授权。

使用 `GET /antigravity/models` 或 `GET /antigravity/v1/models` 查询获准模型。Anthropic 调用使用 `POST /antigravity/v1/messages` 和 `POST /antigravity/v1/messages/count_tokens`。`GET /antigravity/v1/usage` 返回当前网关 Key 的用量。这些路径使用 Anthropic 鉴权头。

Google 客户端使用 `x-goog-api-key` 访问 `/antigravity/v1beta/models` 及单模型路径。支持 `generateContent`、`streamGenerateContent` 及 `countTokens`。启用专用入口后重新生成路由缓存。

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

### Token 计费 {#text-billing}

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

`/v1/embeddings` 和 Gemini 的 `embedContent`、`batchEmbedContents` 只按输入 Token 计算成本和应收。向量模型的费率可将 `output` 设为 `"0"`。上游明确回报零 Token 时金额为零，缺少输入用量时金额保留待核对。

在 `groups.<分组>.rate_multiplier` 设置非负十进制字符串，例如 `"0.5"`，可调整该分组的公开模型应收。倍率参与预占和结算，结算金额最后统一舍入。费率快照保留请求时使用的倍率。

每次上游尝试各有一条记录。回退尝试共用 `request_id`。记录包含下游 Key 或分组、公开模型、连接、上游模型、响应状态和供应商实际返回的 Token 数。缺失用量保留为 `null`。账本省略 Key 密钥、请求正文和生成文本。

在 `ai.providers` 中，为连接自有网关的 SDK 连接设置 `"forward_usage_context": true`。网关为每次实际文本请求保存 `consumer_invocation` 和从零开始的 `model_turn`。运行 `./artisan ai:gateway:usage:list --billing --invocation-id=<id>` 可查看相关尝试，并可用 `--invocation-scope=<scope>` 限定发起调用的 Key 范围。各模型回合有独立的网关请求 ID。关联标识不影响计费或请求去重。

SDK Agent 通过自有网关使用 xAI 原生工具时，在 `ai.providers` 新增独立连接并设为 `driver: "xai"`。URL 填以 `/v1` 结尾的网关地址，凭证使用下游网关 Key，`models.text.default` 填公开模型别名。设置 `store: false`，启用 `forward_usage_context` 以关联各模型回合。将 `ProviderTool::options` 传给该 SDK 连接名。需要在回合间回放供应商工具结果或加密推理时，把公开模型固定到能够读取这些状态的上游连接。

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

对账 DeepSeek 时，从开放平台导出用量。用成本 CSV 核对供应商金额，用用量 CSV 核对请求次数、缓存命中、未命中和输出 Token，并将原始账期转换为 UTC。未结束的账期以 `complete: false` 导入，账期结束后重新导出。汇总项应与相同账号、相同账期的本地连接账目比较。CSV 无请求 ID 时，保留报表原有汇总粒度。

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

向量请求按整次上游调用的可计费输入 Token 上限预占，不占用输出额度。发布批量向量入口时，需核对整批输入的上限。

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

进程重启后，可运行 `./artisan ai:gateway:budget:recover team` 释放已超时且未写入出站标记的预占。默认等待 300 秒，可用 `--after=秒数` 和 `--limit=条数` 调整扫描。恢复会在同一事务中关闭尝试及费用记录并释放预占。普通的已出站尝试继续保留供对账。

对于最终结算前中断的 Titan 批次，恢复命令在同一事务处理整批，即使条数超过 `--limit`。恢复释放下游应收预占，按已保存的费率结算已确认的输入用量，并保留已出站但用量未知的账号预占。超时检查包含最近的出站时间。请在确认原请求已停止后执行恢复。

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

SDK 将完成的用户消息和助手消息一起保存。下一次提示默认按时间顺序读取最近 100 条消息，可在 `config/ai.rs` 中调整 `conversations.max_messages`。数据库默认沿用应用连接，也可用 `AI_CONVERSATIONS_CONNECTION` 指定具名连接。历史页面使用 `Ai::conversation_store()?.paginate_conversation_messages(id, &participant, 15, cursor)` 从最新消息开始游标分页。流式会话使用 `stream()`，将返回的 `response` 交给 SSE 渲染器。调用开始后即可读取会话 ID。SDK 在发送最终完成事件前保存该轮消息。工具步骤完成后如果上游请求出错，SDK 会保留已完成步骤并将助手消息标为 `failed`。首个工具步骤完成前的上游错误不会写入新消息。最终结构化输出无效时，也按同样规则保存已完成的工具步骤。声明了响应结构的 Agent 可使用 `prompt_structured()` 或 `stream_structured()`。

默认数据库存储为新会话生成标题，使用成功连接的经济型文本模型。标题 Agent 最多读取首条提示词的 500 个显示列。可在 `config/ai.rs` 将 `conversations.generate_title` 设为 `false`，或设置 `AI_CONVERSATIONS_GENERATE_TITLE=false`，直接采用 50 个显示列的预览。标题生成失败时使用 100 个显示列的预览。Agent Fake 可通过 `ConversationTitleAgent::fake` 指定标题。

在运行已发布的迁移前，可在 `config/ai.rs` 中设置 `conversations.tables.conversations` 和 `conversations.tables.messages`，也可用环境变量 `AI_CONVERSATIONS_TABLE` 和 `AI_CONVERSATION_MESSAGES_TABLE`。默认表名为 `agent_conversations` 和 `agent_conversation_messages`。使用两个不同的 SQL 标识符，可含字母、数字和下划线，也可使用数据库支持的 Schema 前缀。迁移、应用运行和回滚须使用同一套配置。已有部署要更换表名时，先迁移现有表及数据，再切换配置。

已完成的 SDK 助手消息会保存模型、汇总用量，以及 `steps` 中每步的实际模型、用量、结束原因和工具结果。既有历史可继续读取其原有步骤数据。继续会话时，SDK 按顺序把助手的工具调用及其已记录结果交给模型。`conversations.max_messages` 在展开前限制读取的存储行数，历史分页仍逐行返回消息及其 `steps`。手工构造历史时，先调用 `Message::assistant_with_tool_calls`，再用 `Message::tool_results` 添加结果。

连接、鉴权和请求模型相同时，工具步骤可重放 Chat Completions 私有推理、Responses 加密推理、Anthropic 签名块或 Gemini 思考签名。供应商报告具体模型版本时，仍可使用同一个请求别名重放。变更连接或模型时使用已记录的工具调用与结果。公开历史、回调和流事件沿用原响应结构。

会话消息和待审批快照在数据库中保存文件引用。附件默认写到 `local` 磁盘，文件可见性为私有。在 `config/ai.rs` 设置 `conversations.attachments.disk`，或使用 `AI_CONVERSATION_ATTACHMENTS_DISK`，可为多个应用进程配置共享的私有磁盘。更换配置时，仍须保留既有记录引用的磁盘。

已有 Base64 内联附件的应用可运行 `./artisan ai:migrate-attachments`。命令写入并核验文件后才更新数据库引用。迁移失败的记录可继续按旧格式读取。命令可以重复运行，JSON 输出提供迁移的消息和审批数量。

需要其他会话后端时，实现 `ConversationStore`，并在应用启动时通过 `Ai::set_conversation_store(Arc::new(store))` 注册。`Ai::conversation_store()` 返回当前存储。实现负责校验参与者归属，并以原子操作保存用户和助手消息。需要持久工具审批时使用 `DatabaseConversationStore`。自定义存储可以实现 `supports_attachments` 和 `store_turn_with_message` 来支持附件。

导入历史记录时，可通过 `Ai::conversation_store()` 依次调用 `store_conversation`、`store_user_message` 和 `store_assistant_message`。创建时可传入已有 ID，传 `None` 会生成 UUIDv7。每次写入都需要参与者，追加消息时会核对会话归属。用户附件由默认数据库后端写入配置的私有磁盘。保存助手消息时可传入 `Some(&error)`，记录会标记为 `failed` 并保存错误摘要。每次调用单独提交，普通 Agent 会话仍以整轮为单位原子保存。

## 文本生成选项 {#text-generation-options}

在 Agent 上设置输出 Token 上限与采样参数。匿名 Agent 用 `with_generation_options`，具名 Agent 实现 `Agent::generation_options`。这些选项也用于结构化输出和流式工具续步。Agent 中间件可覆盖当前步骤的选项。

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

所选模型需支持图片输入。OpenAI Responses、Chat Completions、Anthropic Messages 和 Gemini 分别使用原生图片格式。SDK 接收 PNG、JPEG 和 WebP，单次最多 16 张，原始图片总量须小于 50 MB，同时受供应商限制。可以只发送图片，也可以附上文字。附件跟随本轮调用及工具续步。

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

通过 `Message::user_with_attachments(text, attachments)` 和 `with_messages` 可为指定的历史用户消息添加附件。历史和本轮合计最多 16 个文件，原始数据须小于 50 MB。在 Rust 中通过 `Message::attachments` 读取文件。普通消息的 JSON 表示包含角色和文本。

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

### 工具执行事件 {#tool-execution-events}

通过 `Event` 门面监听 `InvokingTool`、`ToolInvoked` 和 `ToolFailed`。事件共用 Agent 调用 ID 和本次处理器执行的工具调用 ID。`event.invocation.call` 包含工具名、供应商调用 ID 和实际参数，其中包括审批时的编辑。终态事件提供结果或类型化错误及处理器耗时。等待审批、被拒绝或被跳过的调用不会运行处理器或分派执行事件。

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

### MCP 客户端工具 {#mcp-tools}

应用通过已连接的 `rmcp` 客户端选择要提供给 Agent 的工具，再把连接对端和工具定义传给 `McpTool::new`。MCP 的连接、鉴权和传输由应用管理。模型看到带 `mcp_tools_` 前缀的工具名，OpenRust 调用 MCP 时发送原始名称和参数。创建适配器时遇到不支持的参数 Schema 会返回 `AiError::InvalidSchema`。

```rust
use std::sync::Arc;
use openrust_ai::{McpTool, agent};

let tool = McpTool::new(client.peer().clone(), definition)?;
let assistant = agent("").with_tools(vec![Arc::new(tool)]);
```

可用 `with_approval` 为工具调用指定应用审批策略。`annotations()` 返回 MCP 服务公布的提示。

使用 Streamable HTTP 时，在应用的 `rmcp` 依赖启用 `transport-streamable-http-client-reqwest`，通过 `StreamableHttpClientTransport` 建立连接。共用同一个 MCP 会话的 Agent 调用可以复用该连接对端。

要把应用自身的 MCP 服务工具直接提供给 Agent，可将工具定义和异步处理函数传给 `McpServerTool::new`。处理函数接收 `CallToolRequestParams` 并返回 `AiResult<CallToolResult>`。Agent 使用工具原名，并可沿用应用审批策略。

### Agent 技能 {#agent-skills}

用 `Skill::from_directory` 打开包含 `SKILL.md` 的目录。YAML 元数据声明技能名称和描述，Markdown 正文提供说明。也可用 `Skill::new` 在 Rust 中定义技能，通过 `files` 提供内联资源。

匿名 Agent 可用 `with_skills` 声明技能，具名 Agent 可实现 `Agent::skills`。来源可为 `Skill`、包含多个技能的目录或解析函数。实际调用准备工具时执行解析函数。Agent 同时注册了 `LoadSkill` 时，OpenRust 会把声明的技能合并到该工具。

模型调用 `load_skill` 时，传 `name` 读取说明和文件清单，另传相对 `path` 读取不超过 256 KiB 的 UTF-8 配套文件。工具提示使用应用当前语言。

使用 `LoadSkill::new` 可直接注册已解析的技能。`LoadSkill::from_sources([])` 会从应用的 `resources/skills` 目录发现技能。

```rust
use openrust_ai::{Skill, agent};

let skill = Skill::from_directory("resources/skills/catalog")?;
let assistant = agent("").with_skills(skill);
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

审批完成后，`ApprovalResponse::Completed(response)` 包含 `Box<AgentResponse>`，可直接通过 `response.steps` 读取模型步骤。

保留会话 ID 和审批 ID。在同一已认证参与者和 Agent 下，用 `pending_approvals_for` 重新读取待审批项，再展示给用户。使用 `resume_approval` 提交每条请求的决定。`Decisions::new` 支持逐项批准、拒绝或编辑参数，也可为剩余项应用默认决定。

使用 `Decision::reject(None)`、布尔值 `false` 或空白结果直接拒绝时，本轮在保存审批决定和已执行的工具结果后结束，后续模型用量为零。若希望模型根据拒绝结果继续，请传入非空结果，例如 `Decision::reject(Some("用户要求保留原金额".into()))`。同一批次里已明确批准的工具仍会执行。

`ToolResult.denied` 表示审批拒绝，`ToolResult.failed` 表示执行失败。`successful()` 会同时检查两者，`error()` 返回未成功的结果文本。Vercel 流使用 `tool-output-denied` 表示拒绝，AG-UI 在工具结果的 `metadata.denied` 报告拒绝。手工构造结果时可使用 `ToolResult::new(id, name, result)`，再按需设置标志。

`ToolResult.arguments` 保存调用使用的参数，包括审批通过的编辑。拒绝时保留原拟调用参数。旧记录缺少该字段时按空映射读取。

Responses 工具调用有独立的条目 ID 与关联 ID 时，`ToolCall.id` 保存条目 ID，`result_id` 保存协议关联 ID。提交审批决定时使用审批请求返回的 ID。上游结果和历史回放优先使用 `result_id`。

审批暂停时，会话历史会立即显示本轮用户消息和状态为 `paused` 的助手消息。恢复后，工具结果写回同一条助手消息。再次暂停或完成仍沿用这条消息的 ID，完成或失败时更新状态。附件引用随原用户消息保留。

审批恢复会在下一次模型请求前保存本步工具结果。应用可按同一参与者、Agent、会话和审批 ID 调用 `approval_results_for`，读取已保存且尚在处理中的结果。完成后从会话历史读取结果。如果恢复进程在续跑途中退出，已保存结果仍可查询，重复提交同一审批会被拒绝，以防工具重复执行。

使用 `Decision::edit` 时，工具使用审批后的参数执行。审批快照会将改动后的参数与结果一起保存，完成后的响应和会话历史也会保留实际执行参数。回放到需要原生推理块的同一连接与模型时，SDK 按供应商协议保留原始块，并附上实际工具结果。通用协议回放使用编辑后的参数。

SDK 在恢复前核对参与者、Agent 和供应商连接，同一待审批批次仅能被一个请求领取。当前审批有效期为 24 小时。领取后的请求失败或中断时，工具可能已经执行，SDK 不会自动重放。工具执行后如果下一次模型请求失败，SDK 将已完成结果保存到失败的会话轮次，并结清审批。审批快照通过私有文件引用保留本轮和历史中的图片及文档。恢复时先核对参与者、Agent 和连接，再读取文件。

需要流式输出时，通过 `stream_with_approvals` 创建响应，并使用现有的 SSE 适配器。SDK 先保存快照，再发出审批事件。默认的 `tool_approval_request` 事件包含 `conversation_id` 与 `approval_id`。Vercel 在 `data-tool-approval` 中交付批次 ID，并为每个调用发送 `tool-approval-request`。AG-UI 在各中断项的元数据中交付 ID。用 `resume_approval` 提交决定，恢复会话并接收普通响应。

如需流式续跑，调用 `resume_approval_stream(&approval_id, decisions)` 并返回现有 SSE 适配器生成的响应。工具结果落库后会先发出，随后交付模型增量。下一步再次需要审批时，继续使用同一个审批 ID。最终结束帧在会话写入成功后交付，历史保留暂停前与续跑后的步骤及累计用量。

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

`broadcast_on_queue` 在 Worker 内生成和发布流。派发前可通过 `invocation_id()` 取得调用标识，用于关联客户端事件。成功回调接收合并文本、所有模型步骤的汇总用量、工具结果和逐步元数据，终态失败会发送同一调用标识的 `stream_failed`。广播任务默认只尝试一次，因为订阅者可能已收到部分事件。

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

在 Agent 上通过 `Agent::middleware` 配置中间件，匿名 Agent 则使用 `with_middleware`。生成的每一步都会收到 `PendingStep`，其中包含当前模型、步骤编号、已完成步骤和用量。调用 `next.run(step)` 继续执行。用 `with_model`、`with_instructions` 和 `only_tools` 修改当前步骤。

非流式调用实现 `handle`，流式调用实现 `handle_stream`。生成的中间件模板包含两个入口。在 `handle_stream` 中，`NextStreamStep::run` 返回持续读取上游的 `StepResult`。用 `StepResult::then` 读取终帧响应，也可用 `StepResult::from_response` 由中间件直接返回结果。只实现 `handle` 的中间件用于流式调用时会返回 `StreamingMiddlewareNotSupported`。

可在返回前用 `result.response().await` 读取完整步骤。这会缓冲该步并将事件保留给调用方。要结束流，在调用 `next.run` 前返回 `StepResult::stop()`，流会以取消状态结束，响应保留已经产生的步骤和用量。

使用 `with_middleware(vec![Arc::new(Stop::when(|step: &PendingStep<'_>| !step.is_first_step()))])` 可以让 Agent 在第一次模型请求后停止。通过 `openrust_ai` 导入 `Stop` 和 `PendingStep`。`Stop::new`、`Stop::when` 和 `Stop::unless` 都接受布尔值或步骤谓词。`unless` 在条件为假时停止。

停止发生在模型请求发出前。首步停止时没有步骤和用量，也不会创建会话记录。后续步骤停止会保留已完成的工具结果和用量。流式终帧的原因为 `cancelled`。如果结构化生成因停止而没有最终对象，非流式结果中的 `structured` 是空映射，已完成步骤仍可读取。自定义非流式中间件可以在调用 `next.run` 前返回 `StepResponse::stop()`。

### 测试 Agent {#agent-fake}

使用 `SupportAgent::fake` 提供有序字符串，也可用 `SupportAgent::fake_with` 根据 `RecordedPrompt` 决定结果。回调可用 `.into()` 返回文本、JSON 对象或完整的 `AgentResponse`。Fake 为每次调用记录一条提示，包括回调返回错误的调用，可通过 `assert_prompted` 和 `assert_prompted_times` 断言。回调在每个模型步骤运行，可看到中间件选定的模型及 `prompt.steps` 中此前的工具结果。模拟模型响应请求已注册的工具时，会经过正常工具处理器。流式响应保留所提供的模型、用量、公开推理摘要、引文和工具结果。

使用 `SupportAgent::fake_responses` 可按序混合文本、JSON 对象、完整响应和 `ToolCall`。序列为空或耗尽时，结构化提示会按属性、枚举、默认值、长度及数值范围生成样本。启用 `prevent_stray_prompts(true)` 可要求每个模型步骤都有显式结果。

用 `SupportAgent::assert_queued`、`assert_not_queued` 和 `assert_never_queued` 核对派发的提示。排队提示和已执行提示分别记录。独立启动的 Worker 使用所属应用中注册的 Fake。

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

已发布的 Bedrock 向量模型也可通过 `/v1/embeddings` 调用。Cohere Embed 接受单条或一批文本，使用模型原生维数。Titan Text Embeddings V2 接受单条或最多 2048 条文本，支持 256、512 或 1024 维。网关在出站前预占整批预算，每条输入对应一次 AWS 请求，按输入顺序返回向量。批内请求失败时返回错误，保留已确认的上游成本，整批不产生下游应收。两者都可返回浮点数组，或将小端 float32 向量编码为 Base64。所有调用均回报输入 Token 时，网关返回合计 `usage`。

将 `AI_EMBEDDINGS_PROVIDER` 设为 `cohere`、`jina` 或 `voyageai`，即可使用对应连接生成文本向量。每条连接与重排序共用密钥和基础 URL。

| 连接 | 向量模型 |
| --- | --- |
| `cohere` | `COHERE_EMBEDDING_MODEL` |
| `jina` | `JINA_EMBEDDING_MODEL` |
| `voyageai` | `VOYAGE_EMBEDDING_MODEL` |

Cohere 读取原生浮点向量和计费输入 Token，Jina 使用 `retrieval.passage`。Voyage 默认走原生文本接口，选择 `voyage-multimodal-3` 或 `voyage-multimodal-3.5` 时由多模态接口接收文本。原版 `voyage-multimodal-3` 固定返回 1024 维。Cohere 的请求维度用于校验模型输出。

使用 Gemini 原生 API 时，设置 `AI_EMBEDDINGS_PROVIDER=gemini` 与 `GEMINI_EMBEDDING_MODEL`。SDK 通过 `batchEmbedContents` 发送文本，单条也使用同一入口。模型 ID 可带或省略 `models/` 前缀。结果按输入顺序返回，指定的维度会传给上游，Token 数量以 Gemini 实际报告为准。

使用 Groq 时，设置 `GROQ_API_KEY`、`AI_PROVIDER=groq` 和 `GROQ_MODEL`，可用 `GROQ_URL` 覆盖默认地址 `https://api.groq.com/openai/v1`。驱动支持 Chat Completions、SSE、本地工具续步及支持模型上的图片输入。无工具的非流式结构化请求使用原生 JSON Schema。使用工具或流时，SDK 将 Schema 放入提示词，并检查最终结果能否解析为 JSON 对象。转录设置 `AI_TRANSCRIPTION_PROVIDER=groq` 和 `GROQ_TRANSCRIPTION_MODEL`，返回文本、时间戳和上游报告的时长。需要区分说话人时，可选用其他转录连接。

使用 Amazon Bedrock Converse 时，设置 `AI_PROVIDER=bedrock` 和 `BEDROCK_MODEL`，并用 `AWS_REGION` 或 `AWS_DEFAULT_REGION` 指定 AWS 区域。可用 `BEDROCK_URL` 覆盖运行时地址。凭证可使用 `AWS_BEARER_TOKEN_BEDROCK`、显式 AWS 凭证及可选的 `AWS_SESSION_TOKEN`，或 AWS 凭证链。SDK 支持普通文本、会话历史及内联 PNG、JPEG、WebP、PDF、纯文本和 Markdown。用 `BEDROCK_MODEL_CHEAPEST` 或 `BEDROCK_MODEL_SMARTEST` 配置独立模型档位。 如果需要代入 IAM 角色，在具名连接配置 `assume_role.arn`。可在 `assume_role` 中设置 `session_name`、`duration_seconds` 和 `external_id`，临时凭证由 AWS SDK 获取和刷新。

用 `Agent::stream` 调用 Bedrock ConverseStream。SDK 读取 AWS 原生事件流，并通过 SSE 返回增量文本和可见推理。结束原因和 Token 用量随最终元数据返回。流截断或 AWS 事件无效会产生流式错误，请求超时也适用于读取事件。

要通过网关发布 Bedrock 模型，在 `config/ai_gateway.rs` 的模型 `endpoints` 中加入 `chat/completions`。客户端用公开模型名调用 `POST /v1/chat/completions`。普通 JSON 和 SSE 都支持前置 system 或 developer 指令、文本历史、最大输出 Token、temperature、top-p 和最多四个停止字符串。设置 `stream_options.include_usage` 为 `true` 可收到末尾用量帧。省略该帧时网关仍记录用量。

此入口支持文本和客户端函数工具。提交 `tools` 与可选的 `tool_choice`，下一轮保留工具定义，并用带原始 `tool_call_id` 的 `tool` 消息回送每个结果。并行结果会合并为一条 Bedrock 消息。若 `parallel_tool_calls` 为 `false`，网关会拒绝上游一次返回多个调用的响应。流式工具调用在 Bedrock 发出完整参数和最终用量后交付。

Bedrock 的已完成轮次含签名推理时，Chat Completions 会在助手消息或一条 SSE 增量里返回 `openrust_reasoning_replay`。续轮将该字段保留在重建的助手消息中，原样提交此前消息，再追加工具结果或新的用户消息。OpenRust SDK 在工具运行中自动处理该字段。回放使用同一网关 Key、公开模型和 Bedrock 连接。显式 Bearer 或 SigV4 凭证沿用原有绑定。使用 AWS 默认凭证链时，网关通过 STS `GetCallerIdentity` 核对调用者。同一 IAM 角色刷新临时凭证并切换角色会话后，可以继续原会话。加密状态有效期一小时，可由共享应用密钥的进程恢复。

使用 Responses 时，设置 `store: false`，按顺序提交原始输入和上轮的全部输出项，再追加新的用户消息或 `function_call_output`。原样保留推理项中的 `reasoning.encrypted_content`。普通 JSON 和 SSE 均返回该推理项，也可通过 `include: ["reasoning.encrypted_content"]` 明确请求。

用户消息可附带内联 PNG、JPEG、WebP 图片及 PDF、UTF-8 文本或 Markdown 文档。Chat Completions 在 `image_url.url` 传 Base64 data URL，文档用 `file` 传 `filename` 和 Base64 `file_data`。Responses 使用 `input_image` 和 `input_file`。带文档的消息需同时提供文本块，图片的 `detail` 支持 `auto`。单次请求最多 16 个附件，其中最多 5 份文档。单张图片上限 3.75 MB，每边最多 8,000 像素。单份文档上限 4.5 MB。这两个入口只接受内联数据。

需要结构化输出时，将 `response_format.type` 设为 `json_schema`，并在 `response_format.json_schema` 中提供 `name`、对象 `schema` 和可选的 `description`。同时发布 `responses` 时，可将这些字段与 `type: "json_schema"` 一起放在 `text.format`。普通 JSON 和 SSE 均使用 Bedrock 原生约束。请选择支持结构化输出的模型，并在 Schema 的每个对象上明确设置 `additionalProperties: false`。该入口需要明确的 Schema，Bedrock 会拒绝不支持的定义。网关为此模式提供 120 秒，包含首次编译 Schema 的时间。

Bedrock 的 Converse 和 ConverseStream 均可通过 `with_tools` 调用本地函数。SDK 保留原始工具 ID，在下一回合随结果回送供应商签名推理。仅在模型支持相应 Bedrock 工具选择时使用 `ToolChoice::Required` 或 `ToolChoice::tool`。流式工具等到消息结束和用量事件完整到达后才执行。

使用 AWS 默认凭证链时，SDK 通过 STS `GetCallerIdentity` 确认调用者。另一个应用进程可以用同一 Bedrock 连接、模型和 IAM 角色批准待执行的工具，也可以在继续会话时回放已保存的签名工具步骤。同一角色刷新临时凭证后仍可继续。恢复待审批工具时，身份变更或无法确认会在执行工具前拒绝请求。签名和脱敏推理保留在内部回放状态中。显式 Bearer 或 SigV4 凭证继续使用原有回放规则。

Bedrock 的普通和流式调用均支持 `with_schema`。SDK 通过专用工具定义发送 Schema，并将该工具的参数作为最终 JSON 响应。与本地函数混用时，前面的回合可以调用本地函数，最后一轮选择结构化工具。对于固定 Laravel 基线中拒绝强制选工具的模型，SDK 把 Schema 加入系统指令，并检查最终文本能否解码为 JSON 对象。

使用 Bedrock 文本向量时，设置 `AI_EMBEDDINGS_PROVIDER=bedrock` 和 `BEDROCK_EMBEDDING_MODEL=amazon.titan-embed-text-v2:0`，然后调用 `Embeddings::r#for`。Titan 每次请求发送一条输入，默认 1024 维。向量索引需要其他维数时，Titan V2 支持 `.dimensions(256)` 和 `.dimensions(512)`。Cohere Embed 模型用 `search_document` 一次发送整批输入，维数由模型决定。

使用 Bedrock 重排序时，设置 `AI_RERANKING_PROVIDER=bedrock` 和 `BEDROCK_RERANKING_MODEL=cohere.rerank-v3-5:0`，再调用 `Reranking::of(documents)?.limit(3).rerank(query).await?`。结果按上游返回的顺序提供从零开始的原文索引和相关性分数。需要按排名返回原始条目时，在 `Collection` 上使用 `Reranks`。

使用 Bedrock 文生图时，设置 `AI_IMAGE_PROVIDER=bedrock` 和 `BEDROCK_IMAGE_MODEL=stability.stable-image-core-v1:1`，再调用 `Image::of(prompt)?.portrait().generate().await?`。驱动支持 Titan Image Generator，以及使用 Bedrock `images` 响应的 Stability Stable Image 模型。AWS 已于 2026 年 9 月 30 日退役 Nova Canvas，驱动保留其兼容后端的请求映射。通过 `square`、`portrait` 或 `landscape` 选择宽高比，用 `store` 或 `store_as` 保存到配置好的磁盘。需要基于参考图进行编辑时，请选择支持图像附件的驱动。

运行 `./artisan ai:models bedrock` 可发现当前区域的基础模型与有效推理配置。目录使用已配置的 Bedrock Bearer Key 或 AWS IAM 凭证链。凭证需允许 `bedrock:ListFoundationModels` 和 `bedrock:ListInferenceProfiles`。需要覆盖控制面基础地址时，设置 `BEDROCK_CATALOG_URL`。其中一个列表调用失败时，命令仍显示另一个列表的结果、已配置的文本模型 ID 和发现错误。默认模型仍由应用选择。

使用 xAI 文本模型时，设置 `AI_PROVIDER=xai`、`XAI_API_KEY` 和 `XAI_MODEL`。用 `XAI_URL` 覆盖默认地址 `https://api.x.ai/v1`，用 `XAI_MODEL_CHEAPEST` 和 `XAI_MODEL_SMARTEST` 配置模型档位。连接使用原生 Responses，支持 SSE、结构化输出、本地函数工具，以及受支持模型的图像输入。上游返回加密推理时，本地工具续步会把它随请求发回。运行 `./artisan ai:models xai` 查看发现的模型及已配置的模型。

使用 xAI 原生连接时，SDK 用量和网关账本将推理 Token 计入输出总量一次，并单独记录推理子项。上游同时报告总量时，按总量判断输出字段是否已经包含推理。网关原生转发保留上游响应中的用量字段。

xAI 原生响应按美元 ticks 报告实际费用。文本读取 `response.usage.cost_in_usd_ticks`，图像读取 `response.usage.tokens.cost_in_usd_ticks`，1 美元等于 10¹⁰ ticks。普通多步 Agent 响应在每一步都报告费用时返回合计。启用网关计费后，`./artisan ai:gateway:usage:list --billing` 的 `reported_cost_usd` 显示原生 Responses 最终回报的费用，费率估算和下游应收可分别查询。账单金额与回报不一致时，`actual.state` 为 `conflicting_evidence`。已有安装需应用最新的 `ai-gateway-migrations`。

在 xAI 使用供应商工具时，将 `ProviderTool::options` 的结果通过 `with_provider_options` 传给对应连接。可选 `WebSearch`（最多指定五个允许的域名）、`CodeExecution`（供应商沙箱）和 `FileSearch`（搜索已有 xAI 集合）。`vector_store_ids` 填写 xAI 集合 ID。同一个 Agent 可通过 `with_tools` 添加本地函数。

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


使用 Mistral 原生 API 时，设置 `MISTRAL_API_KEY`、`AI_PROVIDER=mistral` 和 `MISTRAL_MODEL`。基础 URL 默认为 `https://api.mistral.ai/v1`，可用 `MISTRAL_URL` 覆盖。运行 `./artisan ai:models mistral` 查看模型。原生聊天支持普通与 SSE 响应、结构化输出、工具续步、图片和文档。SDK 分别交付推理块和答案文本。生成向量时设置 `AI_EMBEDDINGS_PROVIDER=mistral` 和 `MISTRAL_EMBEDDING_MODEL`。SDK 使用模型原生维度，并校验调用者显式指定的维度。

选择 Ollama 原生 SDK 连接时，设置 `AI_PROVIDER=ollama` 和 `OLLAMA_MODEL`。默认连接 `http://127.0.0.1:11434`，可用 `OLLAMA_URL` 覆盖，带鉴权的代理可配置 `OLLAMA_API_KEY`。模型目录使用 `/api/tags`，普通聊天、结构化输出和工具续步使用 `/api/chat`，支持非流式响应和原生 NDJSON 流。SDK 增量交付文本和推理内容，并在收到上游步骤终帧后才执行该步骤的工具。向量入口使用 `AI_EMBEDDINGS_PROVIDER=ollama` 和 `OLLAMA_EMBEDDING_MODEL` 调用 `/api/embed`。

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

用 Gemini 生成和编辑图像时，设置 `AI_IMAGE_PROVIDER=gemini`，并用 `GEMINI_IMAGE_MODEL` 指定支持图像的模型。连接沿用 `GEMINI_API_KEY` 和可覆盖的 `GEMINI_URL`。SDK 调用 Interactions API。可用 `size("16:9")`、`square()`、`portrait()` 或 `landscape()` 指定宽高比。画质 `low`、`medium` 和 `high` 分别映射为 `1K`、`2K` 和 `4K`，以所选模型支持的规格为准。响应包含返回的所有图像及上游报告的图像 Token。

使用 xAI 时，设置 `AI_IMAGE_PROVIDER=xai`、`XAI_API_KEY` 和 `XAI_IMAGE_MODEL`。可通过 `XAI_URL` 覆盖连接地址。生成和编辑共用 `Image` 入口，一次编辑最多可提供五张参考图。用 `size("16:9")` 设置宽高比。画质 `low`、`medium` 和 `high` 分别请求 `1k`、`2k` 和 `2k` 分辨率。

用 `Image::fake([])?` 返回占位字节。测试需要读取或保存有效图像时，可传入含有效图片的 `ImageResponse`。用 `Image::fake_with` 根据已记录的提示词、参考图、尺寸、质量和连接构造响应。用 `assert_generated` 检查生成，用 `assert_queued` 检查派发。调用 `prevent_stray_images(true)` 后，响应序列用尽会报错。

图像生成或编辑可通过 `queue()` 或 `queue_with(provider, model)` 排队。配置任务后调用 `dispatch().await`。通过 `ImageAttachment::from_storage` 或 `from_path` 加载排队任务的参考图，确保 Worker 能重新读取。队列按附件顺序保存文件引用。在应用启动时用 `Image::register_queue_then` 和 `Image::register_queue_catch` 注册具名回调。

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

Mistral 沿用 `Audio` 和 `Transcription` 入口。设置 `MISTRAL_API_KEY`，需要替换服务地址时设置 `MISTRAL_URL`。

```dotenv
AI_AUDIO_PROVIDER=mistral
MISTRAL_AUDIO_MODEL=voxtral-mini-tts-3-0
AI_TRANSCRIPTION_PROVIDER=mistral
MISTRAL_TRANSCRIPTION_MODEL=voxtral-mini-transcribe-3-0
```

语音默认返回 MP3，也支持 WAV、Opus、FLAC 和 PCM。`male()` 使用 `en_paul_neutral`，默认音色和 `female()` 使用 `gb_jane_neutral`。可通过 `voice()` 指定 Mistral 音色 ID。此驱动沿用 Laravel 的映射，朗读风格通过输入文本表达。`diarize(true)` 请求带说话人标记的分段，优先于 `language()`。

Gemini 通过原生 Interactions API 合成语音和转录音频。合成设置 `AI_AUDIO_PROVIDER=gemini` 和 `GEMINI_AUDIO_MODEL`，转录设置 `AI_TRANSCRIPTION_PROVIDER=gemini` 和 `GEMINI_TRANSCRIPTION_MODEL`。两者均使用 `GEMINI_API_KEY`，可用 `GEMINI_URL` 覆盖连接地址。语音默认返回 WAV，也可显式选择 PCM。上游旧版返回裸 PCM 时，SDK 可封装为 WAV。专用转录模型按词级时间戳归并说话人轮次。通用 Gemini 模型通过同一入口返回文本或带时间戳的分段。

使用 ElevenLabs 时，设置 `ELEVENLABS_API_KEY`，通过 `AI_AUDIO_PROVIDER` 或 `AI_TRANSCRIPTION_PROVIDER` 选用 `eleven`，再用 `ELEVENLABS_AUDIO_MODEL` 或 `ELEVENLABS_TRANSCRIPTION_MODEL` 选择模型。默认地址是 `https://api.elevenlabs.io/v1`，可用 `ELEVENLABS_URL` 覆盖。运行 `./artisan ai:models eleven` 查看模型。合成支持 MP3、WAV、Opus 和 PCM，可用 `voice()` 指定 ElevenLabs 音色 ID。启用说话人区分后，转录返回带时间戳的词及上游报告的说话人 ID。

使用 xAI 语音合成时，设置 `AI_AUDIO_PROVIDER=xai` 和 `XAI_API_KEY`，然后调用 `Audio::of(text)?.generate().await?`。原生 `/v1/tts` 接口没有模型参数，SDK 在 `AudioResponse::model` 中用 `tts` 标识这个操作。默认输出 MP3，也支持 WAV 和 PCM。默认音色为 `eve`，`male()` 使用 `rex`，`female()` 使用 `ara`，可用 `voice()` 指定音色 ID。语言由上游自动检测。表达风格可在正文中使用 `<whisper>…</whisper>` 等受支持的标签。[xAI 语音合成](https://docs.x.ai/developers/model-capabilities/audio/text-to-speech)

使用 xAI 语音转录时，设置 `AI_TRANSCRIPTION_PROVIDER=xai` 和 `XAI_API_KEY`。SDK 调用原生 `/v1/stt`。设置 `XAI_TRANSCRIPTION_MODEL=grok-voice-transcribe-2.0`，或通过 `generate_with` 指定本次调用的模型。可用 `XAI_URL` 覆盖基础地址。词级时间戳位于 `segments`。启用 `diarize(true)` 后，数字说话人编号以字符串返回，例如 `"0"`。供应商报告的音频时长位于 `usage.seconds`。[xAI 语音转录](https://docs.x.ai/developers/model-capabilities/audio/speech-to-text)

在应用测试作用域内调用 `Audio::fake([])?` 或 `Transcription::fake(["测试转录".into()])?`。用 `fake_with` 根据请求生成替身响应。默认语音替身返回占位字节。测试需要播放或转录该音频时，通过 `AudioResponse::from_bytes` 提供有效的音频样本。用 `assert_generated`、`assert_not_generated` 和 `assert_nothing_generated` 检查调用。对返回的替身调用 `prevent_stray_audio(true)` 或 `prevent_stray_transcriptions(true)`，可在响应序列耗尽时报告错误。

### 排队处理媒体请求 {#queued-audio}

两种 Builder 都提供 `queue()` 和 `queue_with(provider, model)`。设置队列和重试选项后调用 `dispatch().await`。转录排队时，用 `Transcription::from_storage` 或 `from_path` 选择已有文件，确保 Worker 能访问具名磁盘上的对象或同一本地路径。队列载荷保存文件引用，Worker 执行时解析供应商凭证。

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

在应用启动时用 `Audio::register_queue_then`、`Audio::register_queue_catch` 或 `Transcription` 的同名方法注册具名回调，生产端和 Worker 使用相同注册。重试用尽后执行失败回调。测试用 `assert_queued`、`assert_not_queued` 和 `assert_nothing_queued` 检查派发记录，可独立验证后续是否生成。

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

## Gemini 上下文缓存 {#gemini-caches}

用 `GeminiCaches::for_connection("gemini")` 创建和管理 Gemini 显式缓存。向 `create` 传入模型和原生缓存字段。示例中的 `reference` 是达到上游模型最低可缓存输入大小的文本。

```rust
use openrust_ai::{Agent, GeminiCaches, PromptOptions, agent};
use serde_json::json;

let cache = GeminiCaches::for_connection("gemini")?;
let entry = cache.create("your-model", &json!({
    "displayName": "reference",
    "ttl": "1800s",
    "contents": [{"role": "user", "parts": [{"text": reference}]}]
})).await?;
let options = json!({"cachedContent": entry.name});
let response = agent("")
    .with_provider_options("gemini", options.as_object().unwrap().clone())
    .prompt_with("Summarize the reference.", PromptOptions {
        provider: Some("gemini"),
        model: Some(&entry.model),
        ..Default::default()
    }).await?;
```

用 `get` 读取元数据，用 `list` 和返回的分页令牌浏览，用 `delete` 删除。更新过期时间时，向 `update` 传入 `GeminiCacheExpiration::Ttl("3600s")` 或 `GeminiCacheExpiration::ExpireTime(...)`。生成时沿用相同的具名连接和模型。将缓存前缀的系统指令和工具定义放在缓存里。普通和流式响应的缓存输入量由 `response.usage.cache_read_input_tokens` 返回。缓存操作使用 Gemini SDK 直连。

## Gemini 批任务 {#gemini-batches}

使用 `GeminiBatches::for_connection("gemini")` 选择具名 Gemini Developer API 连接。将模型 ID 和 Google 的 `batch` 对象内容传给 `generate(model, &batch)` 或 `embed(model, &batch)`。批任务支持内联请求和 Gemini 输入文件。

```rust
use openrust_ai::GeminiBatches;
use serde_json::json;

let batches = GeminiBatches::for_connection("gemini")?;
let job = batches.generate("your-model", &json!({
    "displayName": "inventory",
    "inputConfig": { "requests": { "requests": [{
        "request": { "contents": [{ "parts": [{ "text": "汇总库存。" }] }] },
        "metadata": { "key": "inventory-1" }
    }] } }
})).await?;
let latest = batches.get(&job.name).await?;
```

保存返回的 `batches/…` 名称，后续可在 Worker 中查询。`state()`、`done`、`response` 和 `error` 提供 Google 返回的状态、结果和单项失败。使用 `download(&job.name)` 流式读取已完成任务的 JSONL 结果文件。分页时把 `list(limit, cursor)` 返回的 `next_page_token` 原样传回。`cancel` 与 `delete` 采用所选 Gemini 账号的权限。以上 SDK 调用直连 Gemini。[Google Batch API](https://ai.google.dev/gemini-api/docs/batch-api)

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
