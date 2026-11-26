# E07 AI SDK 与模型网关契约账本

固定基线、供应商矩阵、分批目标及整体完成条件见 [开发计划](../../plans/laravel-ai-sdk.md)。Laravel AI SDK 固定在 `1d269f2359aa166aaf10b212151bf556dd86a02e`，Sub2API 固定在 `0f6671457132425d1b84f3d80536b53e02942424`。目前仅推进 AI-00 的首批裁决与 AI-01 的非流式文本。网关端点级账本和 SDK 其余成员继续核对。

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
