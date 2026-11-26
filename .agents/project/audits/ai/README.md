# E07 AI SDK 与模型网关契约账本

固定基线、供应商矩阵、分批目标及整体完成条件见 [开发计划](../../plans/laravel-ai-sdk.md)。Laravel AI SDK 固定在 `1d269f2359aa166aaf10b212151bf556dd86a02e`，Sub2API 固定在 `0f6671457132425d1b84f3d80536b53e02942424`。当前进展包括 SDK 文本、模型发现及自托管网关的 OpenAI 文本入口。端点清单和裁决见 [GW-00 网关端点账本](gateway.md)。后续继续按 SDK 公开成员和实际消费者核对。

## AI-01 非流式文本

| 上游位置及成员 | 公开实现及裁决 |
|---|---|
| `src/AiServiceProvider.php` 的包配置、Manager 单例、配置发布 | `AiServiceProvider` 通过已有 Cargo 包发现启用，`Ai::manager` 与容器的 `AiManager` 指向应用级单例。发布标签为 `ai`、`ai-config`。stub 与迁移发布归 AI-03／04。 |
| `src/AiManager.php` 的 `getDefaultInstance`、`setDefaultInstance`、`getInstanceConfig`、`textProvider`、`textProviderFor` 及继承的实例缓存、扩展 | `AiManager` 对应的 Rust 方法为 `get_default_instance`、`set_default_instance`、`get_instance_config`、`text_provider`、`text_provider_for`、`extend`、`forget_instance`。按具名配置惰性解析，应用缺默认连接时错误留在调用时。尚待核对基本方法完整的消费者覆盖、按需实例 `build`／`flushState`、驱动别名及其他能力和生命周期。 |
| `src/Contracts/Agent.php`、`src/Promptable.php`、`functions.php` 的 `instructions`、`prompt`、`agent()` | `Agent::instructions`、`Agent::prompt` 与 `agent()` 可用于单条文本；Rust 以 `prompt_with` 加 `PromptOptions` 实现 Provider／模型／超时覆盖。普通和显式配置验收见包测试。Trait 的 `provider`／`model` 钩子保留单连接的选择优先级。待补消息、schema、数组／按需 Provider 选择、附件、事件及其余成员。 |
| `src/Providers/OpenAiProvider.php`、`OpenAiCompatibleProvider.php` 和对应 Gateway 的非流式请求与响应 | 用 `async-openai` 的请求类型，OpenRust HTTP 工厂实际发送两种协议。由内部投影解析文本及上游用量，非成功状态返回不带上游载荷的错误。用户确认预设不指定默认模型，各上游模型需显式选择。待补高级模型选项、普通与流式终止细节、响应元数据完整契约和供应商实机。 |
| `src/Promptable.php` 的文本 Fake 及 `Gateway/FakeTextGateway.php` | 可从 `Agent::fake` 或 `Ai::fake` 在作用域内对 Agent 类型安装字符串响应序列。已提供记录、匹配、反向、计数和防止非预期请求。待补按需响应闭包、结构化输出、排队和流式等。 |

包级验收通过正常 HTTP Kernel 调用 `Agent::prompt`，实际向本地 TCP 协议服务发出请求。覆盖鉴权、保留基础路径、请求体、模型优先级、返回文本、用量、HTTP 429 错误脱敏和两个应用间 Fake 隔离。独立应用的 Artisan 验收包发现后的配置查询、发布和缓存，检查缓存无原始凭证以及清除缓存后 URL 环境变量覆盖生效。供应商实机与对外网关尚待验收。

## AI-02 共用出站流

框架 HTTP 客户端新增 `PendingRequest::stream` 与 `Response::into_stream`。正常缓冲、磁盘 sink 和 HTTP 中间件路径仍用同一 transport；网络流的读取权只转移一次，事件和记录保留元数据。实际 TCP 验收首块先于上游结束到达、消费方取消时关闭未读连接、总超时和字节上限；原 Request Options 与 Sink 测试同时回归。SDK 的 SSE 协议与网关转发仍分批验收。

## DSH 式模型目录与试调用

`Ai::models`、`artisan ai:models {provider?} {--json}` 与 `artisan ai:prompt {prompt} {--provider=} {--model=}` 已接通 OpenAI 系列兼容端点。按原始 ID 合并 `models.text.default`、`models.text.ids` 和上游目录，保留配置／发现来源和上游明确返回的输入／输出类型。目录失败时结果附带错误，手填值仍可选择。独立 Artisan 加本地协议服务验收带代理前缀的 GET 目录、Bearer 鉴权、字符串模型 ID 及实际试调用。包测试额外覆盖重复项、大小写和目录 404。待补其他原生协议的分页、交互确认保存与更完整的目录元数据。

## AI-02 两种文本 SSE 协议

`Agent::stream`、`stream_with`、`StreamableAgentResponse::into_stream` 与默认 HTTP SSE 响应已消费 OpenAI Chat Completions 与 Responses 文本事件，复用上游出站流和 `eventsource-stream` 解析。验收覆盖真实 TCP、首段文本在上游结束前读取、跨 UTF-8 边界、终止与用量，以及真实应用 HTTP Kernel 的默认 SSE 响应。待补 Vercel／AG-UI、结构化输出、工具／推理／引用事件及完整的 StreamableAgentResponse 成员。

## GW-02 OpenAI 自托管网关

| 入口 | 公开契约及验收 |
|---|---|
| `GET /v1/models` | 仅列出当前 Bearer Key 可请求的公开模型名。两个下游 Key 的授权集合实测不同，上游 Key 不能访问下游端点。 |
| `POST /v1/chat/completions` | OpenAI 和 OpenAI Compatible 同协议请求。保留业务 JSON 中的字符串原文及未知字段，以真实模型调用上游，以公开名响应客户端。支持 JSON 与 SSE。 |
| `POST /v1/responses` | OpenAI 连接原生 JSON 与命名 SSE 事件。共享上游 Key 时按无状态请求裁决，强制 `store=false`，拒绝旧 Response ID、会话、Item Reference 和上游工具。Responses 的其他子端点、WebSocket 和持久状态仍属 GW-00／03 裁决范围。 |
| 配置与错误 | `ai-gateway` 包独立启用，缺省关闭，公开模型和下游 Key 清单缺省为空。启动校验路由目标、模型授权和重复凭证。实际请求验证缺少凭证、未授权模型、未知模型、错误 JSON、不支持的协议、上游 429 和本地 Key 请求限额。响应不暴露上游错误原文、凭证或 Cookie。 |
| 调用用量 | `ai_gateway.call` 记载下游 Key 名、公开模型、上游连接和模型、协议、HTTP 状态、结果及实际报告的 Token。类型未知或未报告计数保留为空；不记录凭证和正文。业务限额、账号池及持久报表归 GW-04。 |

包测试通过正常 HTTP Kernel、本地 TCP 上游验收两种协议、两个下游 Key、JSON、SSE、请求限额与日志。独立应用进程加 Artisan 验收路由列表、配置发布及缓存无密钥，并由外部 HTTP 客户端验证普通 Responses、SSE 首帧提前到达和取消后上游断开。上游为协议模拟，真实 Sub2API 及托管服务商需另行验收。

## GW-03 Anthropic 原生协议

SDK 参考固定 Laravel AI 的 `Anthropic/Concerns/CreatesAnthropicClient.php`、`BuildsTextRequests.php`、`ParsesTextResponses.php`，沿用 `anthropic-version`、`x-api-key` 和默认 `max_tokens`。公开入口为 `Agent::prompt`、`stream` 与 `Ai::models`。本地 Anthropic 协议模拟验证普通消息、SSE 的增量文本和分别报告的输入／输出用量、目录分页和保留手填 ID。独立 Artisan 通过 `ai:models`、`ai:prompt` 和 `route:list` 验收。

网关已接入 `POST /v1/messages`、`POST /v1/messages/count_tokens` 和 `GET /v1/models/{model}`。正常 HTTP Kernel 验证原生认证头与独立上游凭证、协议错误体、授权目录与单模型、SSE 模型名转换、原生 Token 计数。计数请求返回估算值，调用日志的生成 Token 留空。上游测试为协议模拟。与 OpenAI 的跨协议转换、Gemini 和其余能力见 GW-00。

## GW-03 Responses 转 Anthropic Messages

按固定 [Sub2API Responses handler](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler_responses.go) 的跨协议入口实施。公开模型映射使用 Anthropic 原生连接时，网关接受 OpenAI Bearer 请求，检查同一 Key 的公开模型授权，再用该连接的原生认证访问 `/messages`。支持纯文本、首部系统指令、历史文本消息、基本采样和输出限制。未知转换字段、结构化输出、工具、附件及上游持久状态返回明确错误。后续补反方向和更多协议能力。

正常 HTTP Kernel 与独立应用进程加外部 HTTP 客户端，分别验证 JSON 与 SSE、上游模型映射、凭证隔离、首段在上游结束前到达、序列及用量、`max_tokens` 转为 `incomplete`。Artisan 验证对应入口存在。用量日志同时记录入站和上游协议，计数来自上游。上游为协议模拟。

## GW-03 Gemini 原生协议

SDK 的 `gemini` 连接支持 `Agent::prompt`、`stream` 与 `Ai::models`，使用 `x-goog-api-key` 和 Google 版本化相对路径。目录按 `nextPageToken` 分页，以同一个模型 ID 合并手填项和发现项。调用接受裸 ID 和 `models/` 资源名。网关通过 `/v1beta/models`、`/v1beta/models/{model}` 和三个原生动作向外部客户端提供授权模型。当前流格式为 SSE。上游路径只用已配置且通过验证的模型 ID；下游 Key 与上游应用凭证分离。

真实 HTTP Kernel 加本地协议服务覆盖 SDK 普通文本、SSE、目录分页、鉴权与网关 JSON、SSE、Token 估算和公开模型名。独立 Artisan 通过 `route:list`、`ai:models`、`ai:prompt` 验收。缓存资源、其他原生动作和托管服务商实机待后续验收。
