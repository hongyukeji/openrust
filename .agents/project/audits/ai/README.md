# E07 AI SDK 与模型网关契约账本

固定基线、供应商矩阵、分批目标及整体完成条件见 [开发计划](../../plans/laravel-ai-sdk.md)。Laravel AI SDK 固定在 `1d269f2359aa166aaf10b212151bf556dd86a02e`，Sub2API 固定在 `0f6671457132425d1b84f3d80536b53e02942424`。端点清单和裁决见 [GW-00 网关端点账本](gateway.md)。下方分批记录公开成员与真实消费者证据。

## 当前验收范围

SDK 已验证四种原生文本协议、结构化输出、流式工具与回调、具名连接故障转移、Responses／Messages／Gemini 网页引文、Responses 公开推理摘要、数据库会话、持久化审批、Skills 与 MCP 工具桥。模型发现及 Artisan 选择、Embeddings、Reranking、分类、OpenAI 图像与音频、部分供应商文件和向量仓库均已有入口证据。

网关已验证原生文本入口、跨协议工具、下游授权、托管 Key、账号回退、冷却、并发、会话黏性，以及预算预占、用量、计费与账单对账。各条记录标注应用进程、隔离数据库、模拟上游或授权实机。剩余供应商能力、完整的 SDK 类型与网关端点矩阵、应用发布验收继续按计划逐项核对。

## AI-01 非流式文本

| 上游位置及成员 | 公开实现及裁决 |
|---|---|
| `src/AiServiceProvider.php` 的包配置、Manager 单例、配置发布 | `AiServiceProvider` 通过已有 Cargo 包发现启用，`Ai::manager` 与容器的 `AiManager` 指向应用级单例。发布标签为 `ai`、`ai-config`。stub 与迁移发布归 AI-03／04。 |
| `src/AiManager.php` 的 `getDefaultInstance`、`setDefaultInstance`、`getInstanceConfig`、`textProvider`、`textProviderFor` 及继承的实例缓存、扩展 | `AiManager` 对应的 Rust 方法为 `get_default_instance`、`set_default_instance`、`get_instance_config`、`text_provider`、`text_provider_for`、`extend`、`forget_instance`。按具名配置惰性解析，应用缺默认连接时错误留在调用时。尚待核对基本方法完整的消费者覆盖、驱动别名及其他能力和生命周期。按需实例在下方单独记录。 |
| `src/Contracts/Agent.php`、`src/Promptable.php`、`functions.php` 的 `instructions`、`prompt`、`agent()` | `Agent::instructions`、`Agent::prompt` 与 `agent()` 可用于单条文本；Rust 以 `prompt_with` 加 `PromptOptions` 实现 Provider／模型／超时覆盖。普通和显式配置验收见包测试。Trait 的 `provider`／`model` 钩子保留单连接的选择优先级。普通用户和助手历史消息在 AI-02 实现。待补 schema、数组／按需 Provider 选择、附件、事件及其余成员。 |
| `src/Providers/OpenAiProvider.php`、`OpenAiCompatibleProvider.php` 和对应 Gateway 的非流式请求与响应 | 用 `async-openai` 的请求类型，OpenRust HTTP 工厂实际发送两种协议。由内部投影解析文本及上游用量，非成功状态返回不带上游载荷的错误。用户确认预设不指定默认模型，各上游模型需显式选择。待补高级模型选项、普通与流式终止细节、响应元数据完整契约和供应商实机。 |
| `src/Promptable.php` 的文本 Fake 及 `Gateway/FakeTextGateway.php` | 应用范围内支持字符串序列、`fake_responses` 类型化序列与 `fake_with` 步骤闭包。闭包可读取此前工具结果。结构化提示在耗尽后按 Schema 生成样本。复用真实中间件、工具执行和流，按调用记录一次。静态提示及排队断言经真实应用、HTTP、Artisan 与 Worker 验收。 |

包级验收通过正常 HTTP Kernel 调用 `Agent::prompt`，实际向本地 TCP 协议服务发出请求。覆盖鉴权、保留基础路径、请求体、模型优先级、返回文本、用量、HTTP 429 错误脱敏和两个应用间 Fake 隔离。独立应用的 Artisan 验收包发现后的配置查询、发布和缓存，检查缓存无原始凭证以及清除缓存后 URL 环境变量覆盖生效。供应商实机与对外网关尚待验收。

## AI-02 共用出站流

框架 HTTP 客户端新增 `PendingRequest::stream` 与 `Response::into_stream`。正常缓冲、磁盘 sink 和 HTTP 中间件路径仍用同一 transport；网络流的读取权只转移一次，事件和记录保留元数据。实际 TCP 验收首块先于上游结束到达、消费方取消时关闭未读连接、总超时和字节上限；原 Request Options 与 Sink 测试同时回归。SDK 的 SSE 协议与网关转发仍分批验收。

## DSH 式模型目录与试调用

`Ai::models`、`artisan ai:models {provider?} {--json}` 与 `artisan ai:prompt {prompt} {--provider=} {--model=}` 已接通 OpenAI 系列兼容端点。按原始 ID 合并 `models.text.default`、`models.text.ids` 和上游目录，保留配置／发现来源和上游明确返回的输入／输出类型。目录失败时结果附带错误，手填值仍可选择。独立 Artisan 加本地协议服务验收带代理前缀的 GET 目录、Bearer 鉴权、字符串模型 ID 及实际试调用。包测试额外覆盖重复项、大小写和目录 404。Anthropic 与 Gemini 分页及交互保存已分别在后续批次验收。更完整的能力目录元数据仍待核对。

## AI-02 两种文本 SSE 协议

`Agent::stream`、`stream_with`、`StreamableAgentResponse::into_stream` 与默认 HTTP SSE 响应已消费 OpenAI Chat Completions 与 Responses 文本事件，复用上游出站流和 `eventsource-stream` 解析。验收覆盖真实 TCP、首段文本在上游结束前读取、跨 UTF-8 边界、终止与用量，以及真实应用 HTTP Kernel 的默认 SSE 响应。Vercel／AG-UI 文本和结构化流分别在后续批次验收。待补工具／推理／引用事件及其余 StreamableAgentResponse 成员。

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

## AI-05 与 GW-03 OpenAI Embeddings

`Embeddings::r#for`、`PendingEmbeddingsGeneration`、`EmbeddingProvider`、`AiManager::embedding_provider` 和 `EmbeddingsResponse` 已支持批量文本。默认连接由 `ai.default_for_embeddings` 选择，默认模型使用命名连接的 `models.embeddings.default`。显式 Provider、模型、维数和超时可逐次覆盖。OpenAI 原生连接支持此能力，兼容驱动须由应用配置该连接的 Embeddings 能力。用固定 `async-openai` 的请求类型和项目 HTTP 工厂调用，以真实索引恢复输入顺序，拒绝重复索引、数量与维数不一致及非有限向量值。

网关 `POST /v1/embeddings` 支持独立下游 Key、公开模型映射、请求用量和同协议 JSON。模型可配置 `endpoints`，从而只发布对应端点。正常 HTTP Kernel 与本地协议服务验证 SDK 批量向量、维数、重复索引反例，网关验证授权、端点限制、同协议请求和真实用量。独立应用进程及 Artisan 验证配置、路由和外部 HTTP。缓存、Fake、排队、其他驱动、向量数据库桥，以及排序与分类仍是 AI-05 的开放项。

## DSH 式选择与确认保存

`artisan ai:select-model [provider] [--model=...] [--save] [--default]` 复用当前默认连接、已有凭证清单、模型目录与 Artisan 交互。用户可选择已发现或手填的模型。无 `--save` 不写入。保存调用框架 `EnvironmentFile`，可一并保存模型和 `AI_PROVIDER`，保留已有条目与注释、拒绝覆盖生效的进程环境，并提示刷新配置缓存。已存在的文件使用快照冲突检测。首次创建使用框架的安全创建及编码路径。

独立 Artisan 进程加伪终端与本地 HTTP 模型服务验收选择、拒绝、确认、进程覆盖、全新应用的环境文件，以及新进程默认模型生效后的 HTTP 上游调用。应用内测试覆盖 `.env.testing` 的落点。通过这一命令保存的是当前连接的模型设置，供应商 Key 及 URL 沿用应用配置，后续仍需 DSH 客户端连接验收。

## AI-02 多轮文本

按固定 `Messages/Message.php`、`MessageRole.php` 和 `Prompts/AgentPrompt.php` 增加 User／Assistant 文本历史。应用可用 `AnonymousAgent::with_messages`，或实现 `Agent::messages`。历史与本次提示一起进入正常 `prompt` 和 `stream`，Fake 可断言历史。扩展 Provider 继续支持既有纯文本接口，接收历史时需实现新的 `AgentPrompt` 入口。

OpenAI Chat Completions、Responses、Anthropic Messages 和 Gemini 的 Rust 消费者均通过 HTTP Kernel 和本地 TCP 协议服务核对历史顺序与角色。独立 Artisan 用 `ai:prompt --messages=...` 验证历史进入 Anthropic 上游。会话持久化、工具结果角色、附件和结构化输出仍待对应批次。

## AI-02 结构化输出

`HasStructuredOutput`、匿名 Agent 的 `with_schema`、`StructuredAgentResponse` 与 `decode` 复用框架的 `JsonSchema`。Rust 普通及结构化调用分别拥有静态返回类型。OpenAI Chat Completions 和 Responses 使用 JSON Schema 请求，Anthropic 使用原生 `output_config.format`，Gemini GenerateContent 使用 `generationConfig.responseJsonSchema`。返回 JSON 对象，同时保留原始文本、模型和用量。SDK 流目前发出文本增量。接入方可解码为自己的类型；输出 Schema 的执行由原生模型协议处理。

正常 HTTP Kernel 加本地协议服务验收四个适配器、深层对象和数组、严格标记、拒答、非 JSON 结果和 Responses 的结构化 SSE。独立 Artisan 用 `ai:prompt --schema=...` 验证实际 HTTP 请求及 JSON 输出。模型支持差异、Anthropic 合成工具降级、Gemini Interactions、流的结构化专用事件及其余高级选项仍待对应批次。

## GW-03 Messages 转 OpenAI Responses

原生端点优先。模型发布到 OpenAI Responses 连接时，可通过 Anthropic Messages 调用纯文本；规则仍使用同一 Key 和模型端点约束。对照固定 Sub2API 的 `apicompat/anthropic_to_responses.go`、`responses_to_anthropic.go`，处理指令、消息角色、采样与 Token 上限、普通和 SSE 生命周期、公开模型及拒答／截断。双向转换保留上游报告的输入、输出、缓存读写计数，并按各协议的包含关系投影。网关日志保留原上游生成用量，起始 SSE 帧的协议初始值不参与计量。

包测试及独立 HTTP 进程通过两个协议的本地模拟验收反向转换，覆盖首段到达后上游才继续发送、最终事件和用量。Artisan 验证同一发布配置下的 Messages 路由。工具、文件、结构化和持久状态跨协议仍按能力表分批实施。

## GW-03 有序上游回退

已发布模型可通过 `fallbacks` 追加备用 Provider 和真实模型。每次尝试按同一公开模型、独立下游 Key 权限、端点约束及目标协议重新解析。请求限额按下游请求扣一次。无状态生成在连接失败、超时、上游 408／429／5xx 等条件下可发给下一目标。常规 4xx、要求持久化或携带工具的请求、或收到成功响应后的 SSE 错误不会重放。每个上游尝试记录自己的连接、模型、状态和实际用量，并共享请求 ID。

HTTP Kernel 与多个实际 TCP 模拟上游验证非流式和流式回退、400 短路、已有输出的流中断、工具请求不重放及限额只扣一次。独立 Artisan 加外部 HTTP 验证发布配置、上游凭证隔离及回退顺序。账号池冷却、组权限和计量归 GW-04。

## GW-01 官方 Sub2API 进程当前证据

已下载并按 GitHub 发布记录中的 SHA-256 校验 Sub2API `v0.2.25` Darwin ARM64 发行包。二进制自报编译修订 `348997267d756eab5729bb6b78a275a15d18796a`。在回环隔离环境使用 PostgreSQL 18.7、Redis 8、临时管理员及模拟协议上游，已启动并登录。创建分组返回 HTTP `423 ADMIN_COMPLIANCE_ACK_REQUIRED`，要求管理员确认 `v2026.06.10` 的合规承诺。此隔离进程验收到登录。SDK 到既有部署的结果见[授权 Sub2API 实例实测](#gw-01-授权-sub2api-实例实测)。

## AI-01 按需实例生命周期

固定 `AiManager.php::build` 与 `flushState` 已对应为 `Ai::build`、`AiManager::build`、`flush_state`，沿用 `ondemand_` 名称规则和固定 `Lab` 保留名。状态置于框架容器当前请求／命令作用域。按需配置可用名字解析并刷新实例，构造失败保留原先有效实例。清理只针对当前作用域的按需连接。已有常驻配置走应用级实例缓存。

HTTP Kernel 及本地协议上游覆盖具名与自动命名、同名重建、失败保持、普通实例保留、实际提示和后续请求查找失败。独立 Artisan 通过应用注册的命令验证按需查找与清理。Worker 等生命周期的独立实测归 AI-04。

## AI-02 Vercel 与 AG-UI 文本格式

固定 `StreamableAgentResponse.php` 与 `Streaming/Protocols` 的 `usingProtocol`、`usingVercelDataProtocol`、`usingAgentUserInteractionProtocol` 分别由对应 Rust 方法及公开 `StreamProtocol` 实现。Vercel 使用 UI Stream `v1` 响应头、步进帧、文本增量、结束用量及 `[DONE]`。AG-UI 映射 run、step、assistant message 与结束／错误；支持线程和运行 ID。序列化流中错误按协议返回脱敏终止帧。未报告计数保持缺失。

HTTP Kernel 及独立应用进程经实际 TCP 模拟服务验收格式、首段提前、用量与终止。Artisan 验证两个正常路由。工具、审批、引用、推理、浏览器 SDK 集成及其余高级事件仍待对应批次。

## AI-03 非流式应用工具

`Tool`、`ToolRequest`、`ToolCall`、`ToolResult` 和 `StepTextGateway::generate_text_step` 进入正常 Agent 调用。Rust 的工具列表由 `Agent::tools` 或匿名 Builder 装配。复用框架 JSON Schema、Validator 和 ULID。每一步按调用 ID 记录工具结果，限定应用声明的工具集合。重复 ID、非法参数、保密的错误摘要和最后一步停止策略避免重复或未授权执行。累计各步上游报告的 Token。原生 OpenAI Chat Completions 和 Responses 的工具输出按协议回送。

实际 HTTP Kernel 与本地 TCP 测试验证两个协议、参数校验、调用关联、下一步、终止与用量。独立 Artisan 应用命令验收工具执行与第二次真实 HTTP 请求。流式工具、正常／失败／延迟工具的事件、单步中间件、审批与恢复、会话、Provider 工具和其他协议归后续批次。

Anthropic Messages 与 Gemini GenerateContent 已接入同一个原生非流式工具循环。回送完整模型内容以保留签名和顺序。Gemini 有上游 ID 时按原值回送，缺省时生成 SDK 内部关联 ID。HTTP Kernel 及独立 Artisan 经本地协议上游验收声明、原始签名、调用关联、结果及再次生成。Gemini Interactions 和四协议的流式工具循环仍待实现。

## AI-04 Agent 与工具生成器

固定 `MakeAgentCommand` 和 `MakeToolCommand` 对应 `make:agent` 与 `make:tool`。默认目录映射到 `app/ai/agents` 和 `app/ai/tools`。支持嵌套 PascalCase 类型、显式 `App/` 根路径、`--force/-f` 与 Agent 的 `--structured/-s`。缺少名称时复用 Artisan 交互，未传生成选项时再询问是否使用结构化模板。生成只写 Rust 源文件，模块声明由应用维护。

`vendor:publish --tag=ai-stubs` 发布当前三个模板，生成时优先读取应用模板。`ai` 与 `ai-config` 继续发布配置。独立 Artisan 验证缺参、命名、交互、路径、覆盖与发布，生成的普通 Agent、结构化 Agent 和工具通过独立进程编译并经 HTTP 与本地协议上游运行。`make:agent-middleware`、中间件模板、会话迁移发布及 Worker 验收仍待后续批次。

## AI-03 单步 Agent 中间件与 AI-04 模板

固定 `HasMiddleware::middleware` 在 Rust 普通 Agent 契约上由 `Agent::middleware` 装配，匿名 Agent 提供 `with_middleware`。复用框架异步 Pipeline；中间件可以读取 `PendingStep` 的编号、模型、历史、累积用量和调用 ID，在本步修改模型、指令、历史或筛选工具，再调用 `NextStep::run` 继续并处理结果。实际执行核对本步启用的应用工具，即使上游仍返回被禁用的工具，也只记录失败。HTTP Kernel 加本地上游验收三步修改、上下文与工具约束。

`make:agent-middleware` 生成到 `app/ai/middleware`，Agent 中间件模板加到 `ai-stubs`。独立 Artisan 验证生成、发布及生成文件编译后的真实调用。

流式中间件通过 `AgentMiddleware::handle_stream` 复用 `PendingStep` 和异步 Pipeline。可继续实时上游，使用 `StepResult::then` 检查终帧，也可直接返回应用生成的响应。真实 Artisan 和 HTTP 检查三步模型及指令改写、历史传入、实时首段、上下文恢复、禁用工具后的返回结果、非流式原请求契约、应用响应的引文和摘要，以及中间件参与时 503 后只调用首选连接。原生、Vercel、AG-UI 通过。SDK 回归 14 项、严格 Clippy 和三语文档通过。新生成模板提供两种入口，Artisan 实际生成文件通过编译，18 语生成回归再次通过。只提供非流式实现的中间件继续获得明确的流式不支持错误。

流式 `StepResult::response().await` 显式缓冲当前步骤，后续重放原有事件。Artisan 与 HTTP 验证重复读取、读取后登记完成回调、文本只交付一次、上游断开时重复获得同一类型化错误。中间件可在调用下一处理器前返回 `StepResult::stop()`。首步停止用量为零，续步停止保留此前的工具和用量，本轮只到达一次上游。调用下一处理器后返回停止结果会报错，防止将已经发出的尝试计为零。原生、Vercel、AG-UI 验收终态和用量。SQLite、PostgreSQL 18.7、MySQL 8.4.13 验证首步停止不会创建空会话；SDK 回归 14 项、严格 Clippy 和三语站点通过。

- [固定 Laravel StepResult](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/StepResult.php)

`Middleware\Stop` 的构造、`when`、`unless` 和步骤执行前判定映射到 `openrust_ai::Stop`，布尔值和 `&PendingStep` 谓词均可用。非流式中间件沿用现有返回类型，用 `StepResponse::stop()` 声明取消。调用过模型后丢弃响应并尝试返回取消会报错。首步取消返回空响应与零用量，已有步骤则返回累计结果和用量。结构化取消返回空对象与已完成步骤，流式对话跳过最终 JSON 解析。原生流仍按步骤提供增量用量，`cancelled` 终帧自身没有新增用量。

独立 Artisan 和 HTTP 使用真实 SQLite（非流式及流式）、PostgreSQL 18.7（非流式）、MySQL 8.4.13（流式）及本地 Chat Completions 模拟验证首步停止零上游请求、无空会话，工具续步停止只发出一次模型请求，保留一次工具调用和 11／4 Token，跨进程 HTTP 继续会话时重放工具结果。SDK 回归 16 项，包含模型请求发出后拒绝取消响应。独立 SQLite 复验审批跨进程恢复、私有附件、迁移及原生和两种 UI 流。严格 Clippy 和三语站点通过。

- [固定 Laravel Stop](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Middleware/Stop.php)
- [固定 Laravel TextGenerationLoop](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/TextGenerationLoop.php)

### AI-03 Agent 文本替身

`Agent::fake_with` 及 `Ai::fake_with` 在应用范围内根据 `RecordedPrompt` 解析文本、JSON 对象或完整的 `AgentResponse`。真实 Artisan 和 HTTP 确认闭包收到已选定的供应商与模型、结构化 Schema 可读、对象按通用入口解码，Rust 类型不匹配时由 `decode` 返回错误。完整响应的实际模型、7 输入／3 输出 Token、摘要、引文与预设工具结果通过原生／Vercel／AG-UI 输出，测试上游收到零请求。SDK 回归 14 项、严格 Clippy 和三语站点通过。

Fake 作为模型步骤网关接入正常的 Agent 运行链。Artisan 和 HTTP 实测非流式与流式中间件各自改写模型、两步脚本真实执行一次应用工具、下一步在 `prompt.steps` 中读到结果、顶层汇总 9 输入／6 输出 Token。Fake 按固定上游在每次调用进入时记录一次提示，包含后续失败的调用。三种 SSE 与零模型出站复验通过。SDK 回归 14 项、严格 Clippy 和三语站点通过。

`fake_responses` 可按顺序提供 `ToolCall`、文本、JSON 对象和完整响应。真实 Artisan 和 HTTP 消费工具调用与结束文本的序列，每次只执行一次工具，通过 `prevent_stray_prompts(true)` 核对耗尽错误。默认结构化样本覆盖嵌套对象、数组长度、枚举、默认值、邮件格式、整数倍数和小数边界。流程未向测试上游发出请求。既有流和回调场景、14 项回归、严格 Clippy 与三语站点通过。

- [固定 Laravel FakeTextGateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/FakeTextGateway.php)

## AI-05 Embeddings 缓存

固定 `PendingEmbeddingsGeneration::cache` 的禁用、启用、有效期、指定 Cache Store 和整批／逐条两种模式均进入正常 SDK 入口。逐条命中只请求缺失输入并恢复原顺序，重复输入保持原位置。全命中没有上游用量，部分命中只记录实际调用量。缓存键覆盖具名连接、端点、凭证摘要、模型与维度，按需连接重建时不会复用其他连接的结果；自定义驱动可提供缓存隔离标识。

HTTP Kernel 和 TCP 上游覆盖全命中、部分命中、顺序、重复输入、按调用关闭、整批与按需连接重建。独立 Artisan 与 HTTP 进程共享 File、Redis 8、SQLite、MySQL 8.4.13 及 PostgreSQL 18.7 的真实缓存，验证同一个模型只收到初始批次和缺失输入。Embeddings Fake、队列、附件输入与数据库向量检索仍待各自批次。

## AI-05 Embeddings Fake

固定 `Embeddings::fake`、`isFaked`、生成断言与 `fakeEmbedding` 进入 SDK 公开调用；Rust 的闭包配置入口为 `fake_with`。替身支持脚本批次、回调、正维度自动单位向量和未配置响应时的拦截。记录输入顺序、维度、超时、连接及模型。替身在缓存前执行并按应用隔离，保留正常响应的向量数量和类型校验。

包测试验证响应、元数据、断言、耗尽、归一化及跨应用隔离。独立 Artisan 与 HTTP 各走正常 SDK 入口，隔离监听器确认没有出站请求。排队替身与排队断言归生成队列批次。

## AI-05 Reranking 原生文本入口

固定 `Reranking::of` 和 `PendingReranking` 对应 Rust 的文档输入、限制、超时、默认或显式连接及模型。使用同一框架 HTTP 客户端。Cohere、Jina 和 OpenRouter 共用 `top_n/results`，Voyage 使用 `top_k/data`。结果保留供应商顺序、原文档索引和可用用量；重复、越界或非有限分值显式失败。四个预设有标准 URL 和环境覆盖，模型 ID 由开发者选择。OpenRouter 沿用 OpenAI Compatible 配置。

HTTP Kernel 和独立 Artisan／HTTP 进程经真实 TCP 上游模拟验证四种连接、认证、线协议、用量及索引。Bedrock、Collection 便利入口、Reranking Fake 及真实供应商验收仍待对应批次。

- [Cohere v2](https://docs.cohere.com/v2/reference/rerank) · [Jina AI](https://jina.ai/reranker/) · [Voyage AI](https://docs.voyageai.com/reference/reranker-api) · [OpenRouter](https://openrouter.ai/docs/api/api-reference/rerank/create-rerank)

## AI-03 OpenAI Agent 流式工具循环

OpenAI Responses 和兼容 Chat Completions 的 `StepTextGateway::stream_text_step` 复用已有协议请求、SSE 传输及具名工具。第一个 HTTP 请求在 SDK 返回流前建立。流式文本先到达消费者；工具须等上游结束帧及完整对象参数后执行。复用非流式工具的参数校验、重复 ID 检查、工具白名单和最大步骤限制。结果按原 ID 带回模型。SDK SSE、Vercel 和 AG-UI 输出工具事件，后两种汇总实际报告的步骤用量。

HTTP 响应体在请求作用域结束后才消费。流式 Tool 通过框架 `Application::scope` 在所属应用作用域内执行，可正常使用 Validator。请求需要的业务身份由应用在构造工具时显式携带。包测试覆盖真实 TCP 两协议、无结束帧不执行和 UI 事件；独立 Artisan 与 HTTP 验证真实校验、首段文本提前、工具回送和最终用量。Claude／Gemini 流式工具及流式中间件留在相应后续批次。

## AI-03 Anthropic 与 Gemini Agent 流式工具循环

Anthropic Messages 与 Gemini GenerateContent 的 `Agent::stream` 接入现有 `StepTextGateway` 工具循环。Claude 组装分片对象参数及 thinking 签名，等 `message_stop` 才执行工具。Gemini 保留原始 functionCall 和 thoughtSignature，读取末尾用量帧后确认本轮终止。工具结果和原始模型内容按供应商协议一起回传，执行时沿用同一参数校验和步骤限制。

独立 HTTP 进程通过 Vercel UI 协议及时收到 Claude 首段文本，并验证分片参数、签名、调用 ID、下一轮结果和合计用量。独立 Artisan 验证 Gemini 的原始签名及工具结果回送。包测试覆盖 Gemini 独立用量帧及缺少结束标志时不执行工具。两路上游均使用真实 TCP 连接的协议模拟。Gemini 分片函数参数、Interactions 和流式中间件待后续批次。

- [Anthropic Messages Streaming](https://platform.claude.com/docs/en/build-with-claude/streaming) · [Gemini GenerateContent](https://aistudio.google.com/api-reference/generate-content)

## AI-03／AI-04 数据库会话与迁移发布

普通文本 Agent 实现 `RemembersConversations`，可给参加者或认证用户开启对话，按 ID 继续，也可查询参与者与当前 Agent 类型的最近会话。继续前按参与者类型和 ID 精确验证归属，未通过即终止上游调用。历史以 UUIDv7 排序和裁剪，读取最近消息时恢复成时间正序，分页用框架 Cursor。会话与成功的用户和助手消息按同一事务保存，助手行记录模型、供应商、用量与工具调用结果。

`ai.conversations.connection` 沿用框架 Database 连接，可通过 `AI_CONVERSATIONS_CONNECTION` 选择。应用通过 `vendor:publish --tag=ai-migrations` 发布迁移，重新编译后迁移和回滚。无状态 SDK 启动不查询会话表。独立 Artisan 创建、独立 HTTP 恢复对话，使用协议模拟上游和真实 SQLite、MySQL 8.4.13 与 PostgreSQL 18.7 验证持久化、大小写不同参与者拒绝访问、游标、用量与回滚。包测试另外覆盖历史裁剪与上一会话。

完成的一轮原子持久化。标题与协议块的验收见后续批次，已完成工具步骤后的失败记录仍按固定 Laravel 基线核对。

`ConversationStore` 接入创建、继续、历史和原子写入。应用使用 `Ai::set_conversation_store` 注册实现。真实 Artisan 与 HTTP 在数据库连接未配置时使用独立内存实现，核对按 Agent 查找最近会话、参与者不符时 404 且无模型请求、历史分页、流式落库及工具需审批时的安全返回。默认数据库实现仍提供持久审批。SQLite、PostgreSQL 18.7、MySQL 8.4.13 实测默认具名连接、落库、截断和迁移回滚，另复验隔离进程审批恢复。SDK 回归 14 项、严格 Clippy 和三语站点通过。

| 固定 Laravel 写入契约 | OpenRust 入口与裁决 |
| --- | --- |
| `storeConversation` | `ConversationStore::store_conversation` 接收参与者、标题及可选 ID，省略 ID 时生成 UUIDv7。按 OpenRust 已有会话归属契约，参与者必填。 |
| `storeUserMessage` | `store_user_message` 检查参与者与消息角色，保存用户消息并更新会话时间。附件继续使用私有 Storage。 |
| `storeAssistantMessage` | `store_assistant_message` 保存助手消息、用量、模型与步骤，接受可选的 `AiError`，失败时保留 `meta.error`。普通会话仍用原子整轮写入并复用相同的助手记录结构。审批后的续写仍经专用审批入口，暂停记录的逐项对齐待验。 |
| `storeApprovalResults` | `ConversationStore::store_approval_results` 对已领取的会话及审批写入同一步的工具结果。SDK 在下一次模型请求前完成写入，归属、领取者及快照版本一同约束更新。保存于既有审批快照，最终转入会话历史。 |

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过独立 Artisan 与 HTTP 验证明确定义和生成 ID、重复 ID 拒绝、大小写不同参与者拒绝且不落附件、时间更新、私有附件逐字节回读、助手完成和失败状态。SQLite 另验证普通、流式结构化失败留下已有工具步骤和错误摘要，并复验跨进程审批及附件迁移。SDK 回归 16 项，默认及自定义表、具名连接、严格 Clippy 和三语站点通过。

- [固定 Laravel ConversationStore](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Contracts/ConversationStore.php)

默认存储、发布迁移的建表与回滚读取 `ai.conversations.tables.conversations` 和 `ai.conversations.tables.messages`，沿用固定 Laravel 的键和默认值。Rust 发布源码从所属应用的 `MigrationContext::configuration` 读取配置，编译时无需 AI 功能，默认表的索引与外键命名维持兼容，自定义表使用 Schema 的命名规则。SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测配置缓存、迁移预演的表名和外键、独立 Artisan／HTTP 顺序写入与读取、归属检查、流式落库和回滚后重建。相同表名在执行迁移前被拒绝。默认路径及自定义表名回归、迁移回归、严格 Clippy 和三语文档站通过。

- [固定 Laravel 会话迁移](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/database/migrations/2026_01_11_000001_create_agent_conversations_table.php)

新会话默认通过 `ConversationTitleAgent` 选择成功连接的经济型模型，首条提示最多 500 个显示列。禁用 `ai.conversations.generate_title` 时保留 50 列预览，标题失败用 100 列预览。普通会话 Fake 使用本地回退，应用可另外安装标题 Fake。已存在的会话继续时沿用原标题。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 实测独立 Artisan 和 HTTP。主连接失败后用成功连接生成中文标题，验证经济型模型、流式完成、长提示裁剪、禁用、失败回退、普通及标题 Fake 零出站、续聊无重复标题。SQLite 另复验旧及新审批快照，核对只有审批最终完成时生成标题，标题输入只含本轮提示文本。SDK 15 项、严格 Clippy 与三语站点通过。

- [固定 Laravel ConversationTitleAgent](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Agents/ConversationTitleAgent.php) · [固定 Laravel RememberConversation](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Middleware/RememberConversation.php)

## AI-03 流式与结构化持久会话

沿用 `for_participant` 和 `r#continue` 的归属校验与历史构造。持久会话的 `stream` 与 `stream_structured` 在返回流时提供会话 ID，采用任一已有 SSE 渲染器。流式正文照常交付。最后的完成事件在成功落库后发送，截断或客户端提前丢弃的流不会生成半轮记录。工具流的各步用量按已报告的值合并，结构化调用使用现有 Schema 和 JSON 对象解析后再写入。

包测试验证 Fake 的会话输入、结构化输出、流内可见的持久化边界和取消。独立 Artisan 从结构化入口创建会话，独立 HTTP 进程恢复历史、实时发送首段文本、提供存储后的最终事件，并对截断流返回错误。三种真实数据库 SQLite、MySQL 8.4.13 和 PostgreSQL 18.7 均通过 HTTP／Artisan 与迁移回滚。附件及结构化工具步骤重放见对应批次，原始协议块重放继续核对。

完成的助手消息保存公开的逐步响应。流使用与回调、Worker 一致的汇总，持久化之后才交付终帧。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过 Artisan 生成两步工具流，独立 HTTP 进程读到两步模型、结束原因及工具结果，汇总 16 输入／6 输出 Token。旧 SQLite 记录维持原 `steps` 形状，另复验审批快照恢复、结构化入口、截断后无新增消息与迁移回滚。历史记录使用公开元数据和步骤，私有协议重放保留在原审批快照。SDK 回归 13 项、严格 Clippy 和三语站点通过。

- [固定 Laravel DatabaseConversationStore](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Storage/DatabaseConversationStore.php)

继续会话按固定 Laravel 的存储行限制读取历史，然后逐步展开助手内容、工具调用及按调用 ID 配对的已记录结果。旧记录保留原末轮文本，跳过无法配对的工具调用。历史分页仍返回存储行。应用可用 `Message::assistant_with_tool_calls` 和 `Message::tool_results` 手工构造等价历史。四种连接按原生协议传递公开调用与结果。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 中，独立 Artisan 实际发起两步调用并保存。独立 HTTP 进程恢复时对 OpenAI Chat Completions、Responses、Anthropic Messages 和 Gemini 核对工具 ID、参数、结果和后续用户输入顺序，跨参与者没有出站请求。旧记录及部分结果回归、SDK 15 项、严格 Clippy、三语站点通过。原供应商签名和私有推理块按协议分别验收。

- [固定 Laravel DatabaseConversationStore](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Storage/DatabaseConversationStore.php)

Chat Completions 的工具步骤保留 `reasoning_content` 供同连接、同鉴权、同记录模型重放。连接标识沿用审批的请求命名空间摘要，记录中不写入凭证值。旧步骤和不匹配的请求继续发送公开工具调用及结果。私有块不进入公开响应、历史分页、调试输出及 SSE。

SQLite 同时通过普通和流式 Artisan 生成会话，PostgreSQL 18.7 实测普通路径，MySQL 8.4.13 实测流式路径。独立 HTTP 进程复用历史时核对同连接完整回放、更换凭证与模型后只发送公开历史、跨参与者拒绝和公开序列化。旧纯文本及附件审批快照、SDK 15 项、严格 Clippy 和三语站点复验通过。Anthropic 与 Gemini 见下文，Responses 私有块见下文。

- [固定 Laravel Chat 兼容消息映射](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenAiCompatible/Concerns/MapsChatCompletionMessages.php)

Anthropic Messages 复用同连接、同鉴权、同记录模型的有签名工具消息。Gemini 保留模型思考签名及原始工具调用 ID 的可选形状，使后续 functionResponse 对应同一步。切换凭证或模型时改用公开调用与结果。允许回放的是已识别的协议块，包含原生媒体的块使用通用历史，附件继续由私有 Storage 管理。

SQLite 分别通过普通和流式 Artisan 记录两种协议的签名，PostgreSQL 18.7 实测普通调用，MySQL 8.4.13 实测原生流。独立 HTTP 进程逐一核对完整签名、Gemini 原始无 ID 的函数响应、切换模型和凭证后的回退、公开历史和调试隐藏签名，以及跨参与者无出站请求。原生媒体排除用例、SDK 15 项、严格 Clippy 与三语站点通过。

- [固定 Laravel Anthropic 消息映射](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Anthropic/Concerns/MapsMessages.php) · [现有 OpenRust Gemini 原生工具流](../../../vendor/openrust-ai/src/native_tool_stream.rs)

历史私有块按中间件处理后的实际请求模型识别，公开步骤仍报告供应商返回的模型版本。旧审批快照保留读取兼容，缺少请求模型时采用原有流程；流式与普通请求均使用有效请求模型。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 实测中间件从初始模型重写到请求别名，供应商报告不同的具体版本。独立 Artisan 记录后，HTTP 通过同一别名重放私有块，更换模型或凭证时回退公开历史。旧审批快照移除新字段后仍可恢复，SDK 15 项、严格 Clippy 及三语站点复验通过。

Responses 按固定 Laravel 的模型族请求 `reasoning.encrypted_content`，合并应用配置的 `include`。模型显式配置推理时也请求该字段。收到可独立重放的加密块后，保存受支持的原始项及顺序，包括供应商的 function_call 项 ID。存储态文件检索引用不进入重放。改变连接、凭证或请求模型即使用公开工具历史。

SQLite 普通与流式、PostgreSQL 18.7 普通、MySQL 8.4.13 流式均通过独立 Artisan 与 HTTP。在 `store=false` 下核对初始请求的 `include` 去重、工具续步、跨进程历史、加密块和调用 ID 顺序、公开响应隐藏、跨参与者早拒及凭证和模型变化后的回退。SDK 15 项、严格 Clippy 和三语站点通过。样本来自 TCP 协议模拟，已知真实上游验收见 GW-01。

- [固定 Laravel Responses 请求规则](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenAi/Concerns/BuildsTextRequests.php) · [固定 Laravel Responses 历史映射](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenAi/Concerns/MapsMessages.php)

默认数据库存储在流式出错时保留已完成的工具步骤，助手行标为 `failed`。历史分页返回状态，继续会话发送原调用和已保存结果。未完成的步骤、断流后收到的文本及工具前截断都不新增步骤。失败轮使用本地标题预览，客户端继续得到原来的错误。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 实测独立 Artisan 流式失败及 HTTP 读取和继续。首次截断无记录，已执行工具后截断保留一项工具结果和 11 输入／4 输出 Token。后续请求收到保存的结果，实际工具只执行一次。无权参与者没有上游请求。SDK 15 项、严格 Clippy 和三语站点通过。缓冲和审批恢复失败见下文。无人消费的流按固定响应消费路径单列。

- [固定 Laravel RememberConversation 失败处理](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Middleware/RememberConversation.php)

普通缓冲调用也保留上游失败前执行完的工具步骤。SDK 内部传递原错误和已完成响应以保存失败行。公开调用仍返回原错误，已执行工具的尝试不会转移到另一供应商。首次上游失败继续不写入会话。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 用 Artisan 触发首个请求 503 及第二步 503，再用独立 HTTP 校对归属、状态、原调用和结果，以及只计入完成步骤的 11／4 Token。续聊复用已记录结果，未重新执行工具。SQLite 复验旧纯文本与带私有附件的审批路径。SDK 15 项、严格 Clippy 及三语站点通过。

审批恢复执行工具后如果后续模型失败，失败轮、已完成结果、用量与审批结算使用同一数据库事务。归档附件复制到私有会话文件，确认提交后清理已结算审批的临时归档。处理结果事件在失败历史可读后触发，客户端收到原模型错误。未能保存失败轮时，已领取审批仍拒绝重复执行。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 实测独立 Artisan 暂停、HTTP 恢复执行与上游 503。检查批准的工具只执行一次、失败历史读取 31／20 Token、私有文件保留、审批再次读取和提交均被拒绝、已结算事件可读失败历史。旧快照、重复暂停、SSE、自定义表默认路径回归、SDK 15 项、严格 Clippy 和三语站点通过。

结构化会话在最后输出无法解析为对象时仍返回原解析错误。若此前完成了工具步骤，失败历史只保留截至最后一个工具结果的完整步骤。无效输出和对应步骤的用量不会进入会话历史。

SQLite 普通及流式、PostgreSQL 18.7 普通、MySQL 8.4.13 流式以独立 Artisan 和 HTTP 核对损坏输出。工具前的无效结果不写入新会话。工具后的失败保留 11／4 Token 和原工具结果，排除最终无效文本、23／12 Token 和错误步骤的模型。后续请求使用保存的结果，工具调用始终只有一次。SDK 回归 15 项、严格 Clippy 和三语站点通过。

## AI-05 Reranking Fake 与 Collection

`Reranking::fake` 可接收结果列表或完整响应，`fake_with` 接收闭包。默认使用框架 Collection 打乱输入索引并按限制返回，支持阻止意外调用及 Laravel 风格断言。Fake 使用应用内状态，在 SDK 的正常入口拦截并通过与供应商相同的索引和数量校验。包测试覆盖元数据、回调、随机结果、耗尽和无出站请求。

`Reranks` 为框架 Collection 提供 `rerank` 与 `rerank_with`，应用用闭包选择条目中的文本，返回原始条目和上游排名顺序。独立 HTTP 通过 TCP 模拟 Voyage 验证条目关联，独立 Artisan 验证 Fake 拦截、记录和耗尽。固定 Laravel 的字段路径及多字段提取、Bedrock 和供应商专用选项继续保留为待核对项。

- [Laravel 固定 Collection 与 Reranking 注册逻辑](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/AiServiceProvider.php)

## AI-06 OpenAI 原生图像生成、编辑与存储

`Image::of`、`PendingImageGeneration` 和 `ImageProvider` 接入默认或显式连接及模型。OpenAI Images API 经框架 HTTP 客户端发送 JSON 生图，带参考图时发送 multipart 编辑请求。支持标准比例、显式尺寸、质量和超时。兼容连接由 `models.images` 显式启用。输入可来自本地字节或已配置 Storage 磁盘。只接收受支持的内联位图，返回原始字节、MIME、模型、修订提示和上游报告的用量。

包测试验证 JSON、多个 multipart 参考图、损坏 Base64、实际 PNG 解码与磁盘读写。独立 Artisan 生成并写入私有磁盘，独立 HTTP 进程读取同一图片发起编辑并写入命名公开磁盘。两条链路经 TCP 协议模拟，校对认证、字段和原始 PNG 字节。本批覆盖 OpenAI 和实现同一 Images API 的连接。Gemini、xAI、Bedrock、图像 Fake、队列、广播及供应商实机仍留在后续验收范围。

- [OpenAI Images 生图](https://developers.openai.com/api/reference/resources/images/methods/generate) · [OpenAI Images 编辑](https://developers.openai.com/api/reference/resources/images/methods/edit)

## AI-06 OpenAI 文本转语音与录音转录

`Audio::of`、字符串 `ToAudio`、`PendingAudioGeneration` 与 `AudioProvider` 提供默认或显式连接、模型、音色、指令、格式及超时。生成结果持有缓冲音频字节并使用共用 Storage 写入。转录支持本地路径、Storage 磁盘、框架上传及字节输入，经受限读取和内容格式判定后发送 multipart 请求。支持语种、模型选择、说话人分段、时间戳、实际报告的 Token 用量及计费时长。图像也复用受限读取和框架媒体类型检测。OpenAI 兼容连接按能力显式配置。

包测试覆盖标准转录及说话人区分、真实 WAV、用量两种分支、磁盘读取、上传类型探测和异常分段。独立 Artisan 通过本地协议上游生成 WAV 并落盘，独立 HTTP 进程上传同一文件，验证字节、认证、语言、说话人与计费秒数。实测格式为 WAV。其他媒体格式、非 OpenAI 供应商、媒体 Fake、排队、流式转录及正式供应商凭证验收仍待对应批次。

- [OpenAI Speech](https://developers.openai.com/api/reference/resources/audio/subresources/speech/methods/create) · [OpenAI Transcription](https://developers.openai.com/api/reference/resources/audio/subresources/transcriptions/methods/create)

## AI-05 OpenAI Decisions 分类

`Classification::of` 接收文本、JSON 对象或数组。提供具名 `Boolean`、`Choice` 和 `Score` 问题、单次覆盖连接和模型、超时及内联图片。使用固定 Laravel 的 OpenAI Decisions 请求形状，布尔标准折入谓词说明。结果保留类型、概率、置信度、分级说明、拒答、来源模型和上游报告的输入、缓存及推理 Token。未声明的答案、重复键和无效概率返回错误。

包测试覆盖全部问题类型、内联 PNG、模型覆盖与异常响应，独立 Artisan 和 HTTP 以默认或显式模型调用正常 SDK 入口。上游为真实 TCP 的 OpenAI 协议模拟，兼容端点可通过连接配置选择。OpenAI 供应商实机验收待正式凭证。

- [Laravel 固定 OpenAI Classification Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenAi/OpenAiClassificationGateway.php) · [OpenAI Decisions API](https://developers.openai.com/api/reference/resources/decisions/methods/create)

## AI-05 TypeSafe、OpenRouter 和 Gemini 分类

同一 `Classification` 入口复用问题和答案类型。TypeSafe System One 使用 Bearer、具名问题和图片资产引用，保留实际路由模型。OpenRouter 走连接地址的 `/alpha/decisions`，同一连接保留 Chat、Embeddings 和 Reranking 配置。Gemini Decisions 使用 `x-goog-api-key`、文本或图片内容块及其原生用量字段。三家预设复用 URL 和密钥配置，模型由开发者指定。

独立 Artisan 和 HTTP 通过三个本地 TCP 协议服务逐个验证认证、路径、默认及显式模型、图片和结构化状态、用量、越界分级响应。TypeSafe 一并验证状态键冲突。现阶段图片输入支持 PNG、JPEG，OpenRouter 和 Gemini 另接受 WebP。命名附件、Gemini 远端图片、分类故障转移和真实账户验收继续单列。

- [固定 Laravel TypeSafe Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/TypeSafeGateway.php) · [固定 Laravel Gemini Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Gemini/GeminiClassificationGateway.php) · [OpenRouter Decisions API](https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-request)

## AI-05 分类 Fake 与便捷入口

`Classification::fake` 支持答案映射序列或完整响应，`fake_with` 支持按请求返回结果，无显式答案时生成确定性的类型化答案。Fake 记录输入、问题、图片、连接和模型覆盖，可断言有无调用并开启序列耗尽保护。Rust 通过扩展 trait 提供 `Str::decide`、`Stringable::decide` 和 `Collection::decide`。集合可映射原条目的标签和描述，也可对键到描述的集合返回原键。

包测试覆盖三种答案、默认及覆盖模型、图片记录、序列耗尽、标签重复、阈值和原条目映射。独立 Artisan 执行 Fake 请求并确认上游无流量。独立 HTTP 应用通过协议服务运行 Stringable 和 Collection 真实分类逻辑，验证阈值和回到原条目。三语文档同步。

- [固定 Laravel 分类 Fake](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/FakeClassificationGateway.php) · [固定 Laravel 便捷入口](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/AiServiceProvider.php)

## AI-04 具名 Agent 排队与类型化回调

`Agent::queue` 和 `queue_with` 使用框架 Payload、队列连接、重试及终态失败处理。具名 Agent 通过 `QueueableAgent` 指定稳定任务名，在应用服务提供者注册同一个 Worker 处理器。用 Serde 恢复 Agent 状态，调用正常 SDK 入口。Rust 的 `then` 与 `catch` 传注册名，由 Worker 解析为已编译的类型化函数。调用 `dispatch` 时提交，可在此前指定连接、队列名、延迟及任务策略。上游密钥由执行进程解析。

独立 HTTP 与 Artisan 分别派发任务，独立 Artisan SQLite Worker 实测在同一进程里消费、503 重试后成功与耗尽后失败。核对任务表、终态失败表、成功回调原始模型和用量、失效回调名拒绝及上游认证来自 Worker。包测试、严格 Clippy 和三语站点及浏览器检查通过。该批实测具名文本 Agent 与 SQLite 队列。匿名与结构化 Agent、排队 Fake、其他能力排队及更多队列后端继续核对。

- [固定 Laravel Promptable](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Promptable.php) · [固定 Laravel InvokeAgent](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Jobs/InvokeAgent.php)

## AI-04 Agent 流事件广播

`broadcast` 使用框架广播队列，`broadcast_now` 随消费流直接投递，`broadcast_on_queue` 在 Worker 内生成并即时投递。匿名广播沿用框架的频道、授权及广播连接。HTTP 正文读取发生在路由返回之后，广播在所属应用作用域执行。`without_broadcasting` 可按事件类型排除广播，HTTP 流仍保留对应事件。队列模式返回稳定调用标识，终态发出同一标识的 `stream_failed`，默认只尝试一次。成功回调接收收集后的文本、工具和用量。

独立 HTTP、Artisan、SQLite Worker、模型协议服务和真实 Reverb WebSocket 联测三种方式。通过框架频道授权接口得到订阅签名，未授权请求收到 403，未订阅客户端无事件。超过 Reverb 10 KB 入口限制的中文文本事件仍完整到达 HTTP SSE，前后的正常广播保留，拒绝投递写日志。Worker 对同一调用标识发出完成和终态失败事件，实际用量交给回调。三语构建、AI 页面在两种部署前缀的桌面与移动检查以及全站导航页渲染通过。全站浏览器脚本的安装页语言切换保留查询和锚点断言在该轮超时，独立记录于验收日志。

Worker 复用流完成回调的响应汇总，发布期间逐帧处理。新增独立 Reverb 实测中，两步分别报告 11 输入／3 输出和 7 输入／4 输出 Token，Worker 完成回调汇总为 18／7，保留工具结果、合并文本和各步终帧实际模型。订阅者仍按步骤收到原始用量。原三种广播、403、进程隔离、终态失败和大小限制场景同步复验。SDK 回归 14 项、严格 Clippy 与三语站点通过。

Agent 类型可直接断言提示与排队请求。排队断言记录校验后的普通任务和队列广播，任务仍交给配置的队列后端执行。SQLite、PostgreSQL 18.7、MySQL 8.4.13 各由 Artisan 与 HTTP 派发四个任务，验证非法提示和未知回调未记入，排队和已执行记录相互独立。单独启动的 Worker 从自身应用安装 Fake，消费四个任务并调用正常完成回调，模型模拟上游收到零请求。SDK 回归 14 项、严格 Clippy 与三语站点通过。

- [固定 Laravel Promptable](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Promptable.php) · [固定 Laravel BroadcastAgent](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Jobs/BroadcastAgent.php)

## AI-06 OpenAI Files API

`Files::put`、`put_from_path`、`put_from_storage`、`get`、`list` 和 `delete` 复用命名连接和框架 HTTP 客户端。上传支持本地路径、Storage、HTTP 上传和内存数据，使用合法文件名、显式 MIME 及默认用途 `user_data`。文件对象保留创建它的连接，后续读取、刷新、删除均在同一连接执行。文件 ID 校验在 HTTP 请求前执行，下载有大小上限，可写入私有 Storage。

独立 Artisan 把本地文本上传到协议服务。独立 HTTP 进程随后验证元数据、私有存盘回读、真实字节、游标分页、删除及删除后 404。验证拒绝非法路径式 ID，检查认证与实际 multipart。`openai` 原生支持，自定义兼容连接通过 `files: true` 开启。真实供应商凭证、其他 Files 驱动、Fake 及向量仓库继续单列。三语构建、AI 页面十二组浏览器及完整导航检查通过。

- [固定 Laravel OpenAI File Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenAi/OpenAiFileGateway.php) · [OpenAI Files API](https://developers.openai.com/api/reference/resources/files/methods/create)

## AI-06 OpenAI 向量仓库

`Stores` 支持创建、获取、分页和删除。`Store` 支持索引状态刷新、增加文件、成员分页与移除。仓库成员 ID 和源文件 ID 分别保留。命名连接的实例包含资源归属标记，阻止不同连接的文件、不同仓库的成员误用。移除后可选择继续删除原文件。若第二步失败，错误携带原文件 ID，便于单独处理。兼容连接可显式配置 `stores: true`。

独立 Artisan 建立仓库，独立 HTTP 通过 TCP 协议服务验证描述、空闲过期、索引计数、属性、双重分页和归属拒绝。实测保留源文件、删除源文件、移除成功但删除上游 503、仓库删除后 404。包测试、严格 Clippy、三语构建、AI 页面十二组浏览器和全站导航渲染通过。正式服务商凭证、文件与仓库 Fake、Gemini 和 Azure 驱动及其他资源路径继续验收。

- [固定 Laravel Store Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenAi/OpenAiStoreGateway.php) · [OpenAI Vector Stores](https://developers.openai.com/api/reference/resources/vector_stores/methods/create)

## GW-04 持久化上游用量

开启 `ai_gateway.usage.enabled` 后，每个上游尝试在出站前写入数据库。静态或托管 Key、分组、公开模型、实际连接与模型、入站和上游端点一并记录，回退用同一请求 ID 关联。用量只取上游报告值，另保留缓存读写和推理 Token，各协议计数含义按原字段。账本省略请求数据和密钥。可用 `ai:gateway:usage:list` 按请求或托管 Key 分页检查。登记失败返回 503。正常响应和失败在请求内结算，取消后尝试异步收尾，进程退出留下的 `pending` 可以查明。

SQLite、MySQL 8.4.13 和 PostgreSQL 18.7 均经独立 HTTP、协议服务及 Artisan 验收。核对迁移发布、登记不可用时零出站、503 后回退的两条记录、正常 JSON、缺失用量、SSE 正常终止、截断和下游取消。验证无凭证及请求正文、缓存和推理用量、按请求或托管 Key 分页。包测试、严格 Clippy、三语构建、AI 页面十二组浏览器及全站导航渲染通过。Token 或金额硬限额及对未结算记录的核对继续单列。

## GW-04 分组输出 Token 配额

分组 `quota` 可配置 UTC 日窗口和单次生成上限，静态与托管 Key 共享分组额度。Chat Completions、Responses 和 Messages 在完成协议转换后按最终上游请求确定上限。数据库事务原子预留并登记用量，成功结算按确实报告的输出 Token 返还差额。缺失最终用量、失败和取消保留剩余窗口内的预留。回退需要新预留，超过额度返回 429 与 `Retry-After`。上游超报保留实际用量并关闭当日该分组的生成准入。计数和 Embeddings 不消耗生成配额。

SQLite、MySQL 8.4.13、PostgreSQL 18.7 均实测两个独立 HTTP 实例竞争。五个请求争用两个额度时只有两个到达上游。验证静态与数据库 Key 共享、取消、超报、回退、协议字段、计数及独立 Artisan 查询。SQLite 注入结算表暂时不可用，响应保留实际流结果，结算记录保留 `pending`，新尝试无出站；恢复数据库后保留原预留。三库迁移回退及重装、包测试、严格 Clippy、三语构建和浏览器通过。原生 Gemini 生成的 Token 算法、金额账本和未结算记录的手工核对仍待后续。

- [OpenAI Responses 上限](https://developers.openai.com/api/docs/guides/reasoning) · [OpenAI Chat 上限](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) · [Anthropic Thinking 上限](https://platform.claude.com/docs/en/about-claude/models/extended-thinking-models)

## GW-05 Codex 模型目录

OpenAI Bearer 列表收到 `client_version` 时返回 Codex 清单，并提供 `/backend-api/codex/models`。启用无版本路径时 `/models` 也支持同一查询。清单使用公开模型名，按下游 Key 的分组和 Responses 路由能力筛选。可为模型配置显示名称、上下文窗口、输入类型和推理选项。默认公布文本能力，避免由模型 ID 推断未验证能力。ETag 由筛选后的内容生成，鉴权先于 304 判断。

独立 HTTP 验证普通 OpenAI 和 Anthropic 目录、单模型形状、两类别名、不同 Key、未认证条件头、空版本及无效版本参数。独立 Artisan 走标准目录，外部 HTTP 用清单中的别名成功调用实际 Responses 线路。包测试、严格 Clippy、三语构建、AI 页面十二组浏览器与全站导航通过。官方 Codex 0.170.0 Linux ARM64 发布资产已校验。隔离客户端实测原生模型目录及使用授权别名的流式生成，详见[网关验收账本](gateway.md#gw-05-官方-codex-客户端)。固定 Sub2API 的上游账号目录拉取仍单列。

- [固定 Sub2API Codex 模型处理器](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/openai_codex_models_handler.go) · [Codex 0.170.0 模型结构](https://github.com/openai/codex/blob/rust-v0.170.0/codex-rs/protocol/src/openai_models.rs)

## GW-03 Responses WebSocket

`GET /v1/responses` 接受 `response.create`，复用 HTTP 生成、鉴权、限流及用量。覆盖独立客户端、Artisan 撤销、Chat 回退、首段、关闭码、截断和持久用量。实现边界与后续缺口见[网关验收账本](gateway.md#gw-03-responses-websocket)。

## AI-06 Anthropic 与 OpenRouter 文件

`Files` 复用现有公开入口和私有 Storage。Anthropic 使用固定基线的 Files beta 请求头，OpenRouter 全球预设使用 Bearer 和独立 Files 协议。上传沿用 multipart，元数据统一为 `FileInfo`，列表使用上游 `after_id`。上游返回 `downloadable: false` 时，读取或存盘在出站前返回 `FileNotDownloadable`。

分别用独立 Artisan 上传，再用独立 HTTP 应用连接本地 TCP 协议服务，验证原生认证、文件元数据和时间戳、上传限制、两页游标、生成文件的私有存盘与回读、上传文件拒绝下载、删除后 404。OpenAI Files 重跑独立 HTTP 与 Artisan 通过。两个包测试、严格 Clippy、三语构建、AI 页面十二组浏览器及完整导航通过。真实供应商凭证、区域 OpenRouter 端点、其他资源驱动和 Fake 继续单列。

- [固定 Laravel Anthropic File Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Anthropic/AnthropicFileGateway.php) · [固定 Laravel OpenRouter File Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenRouter/OpenRouterFileGateway.php) · [Anthropic Files API](https://platform.claude.com/docs/en/build-with-claude/files) · [OpenRouter Files API](https://openrouter.ai/docs/api/api-reference/files/list-files)

## AI-06 Gemini 文件

`Files` 支持 Gemini 原生上传、元数据、状态刷新、分页和删除。上传依据 Google 的 resumable 协议创建会话，然后在同源的上传端点发送文件字节。拒绝其他来源、路径重写及跳转。资源支持 `files/` 前缀及短 ID，列表使用不透明令牌。上传文件的内容读取返回 `FileNotDownloadable`。供应商自定义认证头标记为敏感，由框架 HTTP 诊断快照过滤。

独立 Artisan 和 HTTP 通过本地 TCP 验证标头、JSON、二进制上传、异源会话拒绝、诊断移除 Key／会话查询／正文、`PROCESSING` 刷新为 `ACTIVE`、过期时间、带符号的分页令牌和删除后 404。两个包测试、严格 Clippy、三语构建、AI 页面十二组浏览器及完整导航通过。Gemini 向量仓库、提示词文件引用、供应商生成文件下载和授权 Gemini 实机仍待独立验收。

- [固定 Laravel Gemini File Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Gemini/GeminiFileGateway.php) · [Google Files API](https://ai.google.dev/gemini-api/docs/files)

## AI-06 Gemini 向量仓库

同一 `Stores` 门面接入 Gemini File Search，支持创建、获取、分页、删除和仓库文档管理。导入使用原生异步操作，限时轮询同一仓库的操作名。`StoreOptions` 可初始化文件，失败时返回已创建的仓库 ID。Gemini 对描述与闲置过期设置返回明确错误。原始文件 ID 与文档 ID 分开保留。由于供应商文档清单不含原文件 ID，`StoreFile::file_id()` 返回 `Option<&str>`。若原文件身份未知，同时删除原文件的请求在移除成员之前被拒绝。

分别以独立 Artisan 和 HTTP 验证原生认证、预填文件、轮询、字符串／数字／字符串列表元数据、分页、跨仓库和跨操作拒绝、只移除成员、同时删除原文件、软失败后处理以及删除后 404。OpenAI 仓库以同样入口重跑通过。两个包测试、严格 Clippy、三语构建、AI 页面十二组浏览器及完整导航通过。真实服务商凭证、Gemini File Search 工具及向量仓库 Fake 继续单列。

- [固定 Laravel Gemini Store Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Gemini/GeminiStoreGateway.php) · [Google File Search Stores](https://ai.google.dev/api/file-search/file-search-stores) · [Google File Search Documents](https://ai.google.dev/api/file-search/documents)

## AI-06 Azure OpenAI 文件与向量仓库

Azure 预设沿用 `openai` 协议驱动，按需追加 `/openai/v1`，使用敏感的 `api-key` 请求头、用户配置的部署名和默认文件用途 `assistants`。其他 OpenAI 连接沿用默认 Bearer 与 `user_data`。具名连接可显式配置 `auth: "api-key"`。

本地 TCP 上游与独立 Artisan、HTTP 应用验收资源地址和完整 v1 URL、默认及显式上传用途、认证头诊断脱敏、Responses 文本部署、创建仓库、成员分页、移除以及同时删除原文件。独立 OpenAI 文件回归、两个包测试、严格 Clippy、三语构建、AI 页面十二组浏览器及完整导航通过。Azure 实机、Azure 特有的图像及 Embeddings 有待各自的协议与部署验收。

- [固定 Laravel Azure OpenAI 客户端](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/AzureOpenAi/Concerns/CreatesAzureOpenAiClient.php) · [Microsoft Files](https://learn.microsoft.com/en-us/rest/api/microsoft-foundry/azureopenai/files) · [Microsoft Vector Stores](https://learn.microsoft.com/en-us/rest/api/aifoundry/azureopenai/vector_stores)

## GW-04 调用方用量

`GET /v1/usage` 按当前凭证返回当日、累计及 UTC 日序列。已通过 SQLite、MySQL 和 PostgreSQL 独立 HTTP 与 Artisan 验收。见[网关验收账本](gateway.md#gw-04-调用方用量)。

## AI-06 文件 Fake

`Files::fake`、`fake_with`、`fake_id`、`is_faked` 和上传／删除断言接入应用容器。实际文件连接继续可由原有 `Ai::file_provider` 解析。Fake 在未配置真实供应商时也可使用，按具名连接隔离。查询支持字节序列、回调和严格模式，上传可回读并通过游标分页。

独立 Artisan 和 HTTP 验证无真实供应商配置、确定的 ID、严格响应序列、回调、断言、同名文件的连接隔离、应用隔离与私有 Storage 回读。两个包测试、严格 Clippy、三语文档构建、十二组浏览器及导航通过。仓库 Fake、文件提示词及其他媒体的 Fake 继续分列。

- [固定 Laravel Files 入口](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Files.php) · [固定 Laravel FakeFileGateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/FakeFileGateway.php)

## GW-05 固定 DSH 客户端验收

固定 DSH 0.2.3-alpha.2 真实进程已通过四种外部流式协议。真实 DSH Web 验证模型发现、手填、保存、选择及生成。协议和验收条件见[网关验收账本](gateway.md#gw-05-固定-dsh-客户端)。

## AI-06 向量仓库 Fake

`Stores::fake` 连同文件 Fake 装配，覆盖创建、获取、模拟响应回调／序列、成员关系、分页、删除与断言。可选择保留已有文件连接。公开名称和所有权都跟随具名上下文，同一文件可通过同一连接加入和删除。不同仓库、不同连接和不同 Fake 实例之间的成员隔离。

独立 Artisan 验证固定 ID、严格序列、选项和断言。独立 HTTP 验证回调、文件 Fake 联动、成员分页、只操作当前连接与 Fake、删除成员和原文件。OpenAI 向量仓库回归、两个包测试与严格 Clippy、三语构建、十二组浏览器及导航通过。

- [固定 Laravel Stores 入口](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Stores.php) · [固定 Laravel FakeStoreGateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/FakeStoreGateway.php)

## AI-02／AI-03 Agent 图片附件

匿名 Agent 用 `with_attachments`，具名 Agent 实现 `attachments`。复用图像生成的 `ImageAttachment`，支持字节、应用磁盘、本地路径与 HTTP 上传。允许图片作为唯一输入。OpenAI Chat Completions／Responses、Anthropic Messages 与 Gemini 各自以原生格式接收图片。流式工具续步保留本轮图片，文本 Fake 记录图片并支持断言。此前批次覆盖本轮调用。会话历史与落库见下文。

独立 Artisan 验证本地、私有磁盘、Fake、图片数量上限和真实 Responses 请求。真实 HTTP Kernel 解析 multipart，四个本地 TCP 协议入口逐一核对鉴权、图片字节、普通响应、SSE 及仅图片输入。Chat 工具流验证第二步仍保留同一张图片与工具结果。文档、供应商存储 ID、远端附件、单次调用及历史附件、真实供应商视觉模型仍列为后续验收。

- [固定 Laravel Promptable](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Promptable.php) · [固定 Laravel Image](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Files/Image.php)

手动历史通过 `Message::user_with_attachments` 与 `with_messages` 传入。四种协议复用各自的用户附件内容构造，保留消息角色、附件顺序以及其后的无附件提示结构。普通消息 JSON 继续只有角色和文本，Fake 可直接断言历史附件。历史与本轮共用 16 个文件和小于 50 MB 的原始数据上限。独立 Artisan 发送图片和 PDF 并验证 Fake、超限拒绝。真实 HTTP 通过 multipart 接收 PDF 和 Markdown 并验证四路原生协议的普通响应与 PDF SSE，Chat 的 Markdown 在发送前拒绝。OpenAI 请求结构回归、SDK 15 项、严格 Clippy、三语站点通过。会话落库、历史回填和审批恢复见下文。

- [固定 Laravel UserMessage](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Messages/UserMessage.php)

默认 `ConversationStore` 先写私有 Storage 文件，再在同一数据库事务提交用户消息、附件引用和助手响应。审批快照以同一机制保存本轮及历史附件。两者按所属消息或审批 ID 校验路径，按记录的磁盘、类型、字节数和 SHA-256 回读。同一审批再次暂停复用原归档，完成事务后清理审批归档。新增记录使用引用格式。普通消息 JSON 保留角色和内容。

`bddf1fb04` 的旧会话 Base64 记录和早期审批快照仍可读取。实际运行 `ai:migrate-attachments` 会在文件核验通过后条件更新原记录。数据库确认未更新时删除本次文件，结果不明时保留文件。移除只为内联写入添加的包扩容迁移。应用已运行的列宽和数据可继续使用。自定义存储可通过 `supports_attachments` 和 `store_turn_with_message` 启用。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 用独立 Artisan／HTTP 实测文件私有权限、SHA-256、数据库仅保留引用、旧会话回放、旧审批直接恢复、两种旧记录迁移、失败保留及重复迁移。通过默认 SSE、Vercel 和 AG-UI 验证暂停，跨进程恢复准确回送本轮和历史附件。跨参与者及变更连接的请求在恢复前被拒绝，并发审批只执行一次。三库复验再次暂停复用同一归档，最终落库后清理。另复验旧纯文本快照、默认和自定义表的 SDK 回归、严格 Clippy、Artisan 18 语帮助及三语站点。本批实际文件后端为私有本地磁盘。

- [固定 Laravel StoredImage](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Files/StoredImage.php) · [固定 Laravel StoredDocument](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Files/StoredDocument.php)

## AI-02 模型档位与文本摘要

`Agent::model_tier` 及匿名 Agent 的 `with_model_tier` 对应固定 Laravel 的 `UseCheapestModel` 和 `UseSmartestModel`。连接在 `models.text.cheapest`、`models.text.smartest` 配置模型 ID，显式模型优先，未设置档位时使用已配置的默认模型。模型发现合并默认模型、档位模型和上游模型。内置连接预设通过既有模型环境变量加 `_CHEAPEST`、`_SMARTEST` 后缀读取对应配置。

`SummarizeAgent` 默认提示最多三句，句数至少为一，使用经济型模型。通过扩展 trait 提供 `Str::summarize`、`Stringable::summarize` 和单次覆盖入口。真实 Artisan 验证 Responses 经济档、显式模型切换到 Anthropic、目录合并和 Fake，真实 HTTP 验证高能力档、Gemini 的默认模型回退和经济档 SSE。上游均使用本地 TCP 协议服务。

- [固定 Laravel Promptable](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Promptable.php) · [固定 Laravel 摘要宏](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/AiServiceProvider.php#L102) · [固定 Laravel SummarizeAgent](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Agents/SummarizeAgent.php)

## AI-02 Agent 文档附件

`Attachment` 统一图片与文档的本轮提示输入，保留传入顺序。`DocumentAttachment` 从字节、私有 Storage、本地路径或上传接收 PDF、UTF-8 文本和 Markdown。OpenAI Responses 发送 `input_file`，Chat Completions 发送 PDF `file`，Anthropic Messages 使用原生 `document`，Gemini 使用 `inlineData`。Chat Completions 对纯文本和 Markdown 附件在出站前返回不支持。附件共用数量和原始字节限制。

独立 Artisan 验证本地文件、命名磁盘、混合顺序、错误格式和 Fake 记录。真实 HTTP 逐协议发送 multipart，核对四个本地 TCP 上游收到的 PDF 原始字节、文件名、普通响应和 SSE，以及三种支持协议的 Markdown 映射。远端 URL、供应商文件 ID、会话历史及排队的文件恢复仍按独立契约验收。

- [固定 Laravel Document](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Files/Document.php) · [OpenAI File inputs](https://developers.openai.com/api/docs/guides/file-inputs) · [Anthropic PDF support](https://platform.claude.com/docs/en/build-with-claude/pdf-support) · [Gemini Document understanding](https://ai.google.dev/gemini-api/docs/document-processing)

## AI-05 Gemini 原生文本 Embeddings

同一 `Embeddings::r#for` 支持 Gemini 原生 `batchEmbedContents`。使用连接的 `models.embeddings.default` 或显式模型，带 `models/` 的写法先规范为同一个模型标识，复用连接鉴权和独立协议缓存。请求保留文本顺序和维度。响应检查向量数量、长度和有限浮点值，只返回已报告的 Token 数。

独立 Artisan 写入真实 File Cache，独立 HTTP 回读相同向量和模型标识，证明缓存命中没有出站。还验证原生 API Key、模型路径、批量顺序、显式覆盖、未报告用量、非法路径、超大维度及响应维度不符。本批覆盖文本 Embeddings，对外 Gemini 原生网关及多模态向量另行验收。

- [固定 Laravel Gemini Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Gemini/GeminiGateway.php#L179) · [Gemini Embeddings API](https://ai.google.dev/api/embeddings)

## AI-02 文本生成选项

`TextGenerationOptions` 通过具名 Agent 的 `generation_options` 或匿名 Agent 的 `with_generation_options` 配置输出 Token 上限、temperature 与 top-p。协议分别使用 Responses 的 `max_output_tokens`、Chat 兼容与 Anthropic 的 `max_tokens`、Gemini 的 `generationConfig.maxOutputTokens`。Anthropic 沿用固定 Laravel 的模型采样判定，并优先采用 temperature。Fake 记录选项。中间件可在普通响应的当前步骤覆盖配置，流式工具续步会保留原始 Agent 配置。

独立 Artisan 验证具名 Agent、选项记录与非有限数拒绝。真实 HTTP 通过四个本地 TCP 上游核对字段、步骤限额覆盖、流式工具续步、Anthropic 采样取舍，以及 Gemini 同时发送结构化 Schema 和参数。特定供应商扩展选项、工具选择与提示缓存另按对应契约验收。

- [固定 Laravel TextGenerationOptions](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/TextGenerationOptions.php) · [固定 Laravel Anthropic 请求映射](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Anthropic/Concerns/BuildsTextRequests.php)

## AI-03 Agent 工具选择

`ToolChoice` 提供自动、禁止、要求及指定工具四种选择，由具名 Agent 或匿名 Agent 配置。对应四种原生协议的工具选择字段。要求及指定只作用于第一步，与固定 Laravel 的续步规则一致。普通中间件可以覆盖当前步骤的选择。无可用工具及未声明名称在出站前报错。模型返回与要求冲突的工具调用时，先报错再结束，不执行该调用。

独立 Artisan 验证 Responses 的指定工具、首步后释放及 Fake 记录。实测 HTTP 通过 Chat、Anthropic 和 Gemini 本地 TCP 上游覆盖流式续步、自动、禁止、指定与必选，以及已注册的其他工具被拒绝且未触发其处理器。包测试、严格 Clippy 和三语文档验收通过。

- [固定 Laravel ToolChoice](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/ToolChoice.php) · [固定 Laravel 续步选项](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/TextGenerationOptions.php)

## AI-02 按连接的供应商扩展选项

固定 Laravel 的 `HasProviderOptions::providerOptions` 对应具名 Agent 的 `provider_options`，匿名 Agent 用 `with_provider_options` 登记具名连接字段。SDK 传递当前连接的扩展参数，工具续步沿用本轮选项，普通中间件可覆盖当前步骤。模型、消息、工具选择、结构化输出及流式控制等保留字段由 SDK 管理。Gemini 允许合并 `generationConfig` 的额外字段。Anthropic 扩展思考与强制工具选择互斥。

独立 Artisan 核对具名连接、Responses 推理选项和 Fake 记录。真实 HTTP 通过四个本地 TCP 上游验证普通调用、Chat 工具流续步、Gemini 结构化合并和保留字段拒绝，以及 Anthropic 扩展思考与强制工具互斥。包测试、严格 Clippy 和三语文档验收通过。

- [固定 Laravel HasProviderOptions](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Contracts/HasProviderOptions.php) · [固定 Laravel 生成选项合并](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/TextGenerationOptions.php)

## AI-05 查询构建器与 SDK 嵌入

安装 AI 后，应用容器为数据库的文本向量查询提供 SDK 默认实现。既有自定义数据库 Embedding Provider 保留优先权。Query Builder 与 Eloquent 的 `*_text` 入口复用当前 SDK 连接、模型、维度和缓存策略。`ToEmbeddings` 为字符串和 `Stringable` 提供单向量入口。数据库桥关闭旧的外层缓存，应用继续从 `ai.caching.embeddings` 管理 SDK 缓存。

独立 Artisan 依次调用 `Stringable` 和 Query Builder，从真实 PostgreSQL 18.7 与 pgvector 0.8.2 查询过滤、顺序和投影距离。独立 HTTP 的 Eloquent 查询共用同一文件缓存。关闭 SDK 缓存后两次文本转换均实际出站。应用手动替换旧数据库绑定后可在 SDK 无凭证时查询，全程不会调用 SDK。保留框架数据库嵌入回归测试，包测试、严格 Clippy 与三语文档通过。

- [Laravel Query Builder 向量相似度查询](https://laravel.com/docs/13.x/queries#vector-similarity-clauses) · [pgvector 0.8.2](https://github.com/pgvector/pgvector/tree/v0.8.2)

## AI-03 相似度搜索工具

`SimilaritySearch` 复用模型查询、SDK 文本嵌入及集合重排。`using_model` 接收向量列、相似度阈值和条数。`using_model_with` 允许应用附加约束，`using` 支持自定义异步检索。结果保留业务字段并移除向量列。支持自定义工具说明、空结果和按候选重排。工具名沿用固定 Laravel 的 `SimilaritySearch`。

独立 Artisan 通过真实 Responses 工具循环检索 PostgreSQL 18.7 上的 pgvector 0.8.2。独立 HTTP 验证模型查询过滤、结果数量、SDK 缓存复用、Cohere 协议重排、注入自定义查询、空结果和无效参数。协议上游使用本地 TCP 服务。相关包测试与严格 Clippy 均通过。

- [固定 Laravel SimilaritySearch](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/SimilaritySearch.php) · [固定 Laravel 工具名称解析](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/ToolNameResolver.php)

## AI-03 工具审批声明与执行前门控

工具通过 `Tool::should_request_approval` 针对当前参数声明审批要求。待审批项包含工具调用 ID、名称、参数与可选原因。`Decision` 和 `Decisions` 支持批准、拒绝、编辑参数及批准或拒绝剩余项，并检查空集合、重复调用 ID 与通配编辑。需要审批且没有恢复上下文时，普通请求返回 `ApprovalNotResumable`，流式入口交付错误。SDK 在执行当前批次的任一工具前检查整个批次，包括无须审批的并列工具。

独立 Artisan 核对待审批项和决定集合。实际 HTTP 验证普通请求、Vercel 流式错误、并列工具无副作用，以及无须审批的普通调用。相关包测试、严格 Clippy 与三语文档验收通过。持久化审批、恢复执行和独立审批事件仍待实施。

- [固定 Laravel Approvable](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Contracts/Approvable.php) · [固定 Laravel 审批类型](https://github.com/laravel/ai/tree/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Approvals)

## GW-01 授权 Sub2API 实例实测

2026-11-26，使用现有调用方 Bearer Key 和连接级附加头，经 OpenRust SDK 连接授权的 Sub2API 实例，协议为 OpenAI Responses，模型为 `gpt-6.1-sol`。部署版本、分组和实际选中的上游账号由下方后台核对补证。实例的账号、分组与管理配置保持原样。

| 实测入口 | 结果 |
|---|---|
| Artisan `ai:models sub2api --json` → `GET /v1/models` | HTTP 200，发现 12 个模型，`gpt-6.1-sol` 同时标记为已配置和已发现。目录未声明用于本次验收的嵌入、音频和原生图片模型。 |
| Artisan Agent 与应用 HTTP 路由 → `POST /v1/responses` | 普通文本成功，输入／输出 Token 为 26／8。Artisan 和 HTTP 路由分别收到 5 个 SSE 文本增量及结束事件，HTTP 路由返回 200；该路由此次报告 27／9 Token。 |
| Artisan Agent 客户端函数 → `POST /v1/responses` | 强制选择库存工具、核对参数、执行本地工具、提交工具结果并收到模型总结，累计 168／24 Token。 |
| Artisan 严格 JSON Schema → `POST /v1/responses` | 初次返回 400。递归关闭出站对象的 `additionalProperties` 后成功返回布尔字段，报告 44／12 Token。 |
| Artisan Embeddings → `POST /v1/embeddings` | 使用已授权文本模型试请求，返回 503。向量实机仍需该实例提供可用向量模型。 |
| Artisan Responses `image_generation` 工具 | 返回 403。直连 Images 与音频未取得该实例的可用模型证据。当前实例的媒体实机验收仍未完成。 |

连接级 `headers` 沿用固定 Laravel 的配置名，在 SDK 共享 HTTP 入口合并并标记为敏感值。实际 Artisan 配置入口验证保留鉴权头、重复名称及非法头值被拒绝。回归测试通过 Chat Completions 与 Responses 核对嵌套 Schema；Anthropic、Gemini 同样使用对象转换。此记录对应 SDK 消费既有实例，OpenRust 自托管网关及实际客户端入站证据见[网关端点账本](gateway.md)。

- [固定 Laravel Provider](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Providers/Provider.php) · [固定 Laravel ObjectSchema](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/ObjectSchema.php)

### Sub2API 后台只读核对

使用已登录的 Chrome 查看「使用记录」和「账号管理」，按时间、端点和 Token 数匹配 SDK 实测记录。后台显示版本 `v0.2.23`。六条成功调用均命中 `OpenAI` 分组下的同一个 OpenAI OAuth Pro 账号，入站和上游端点均为 `/v1/responses`，记录的响应模型为 `gpt-6.1-sol`。

| 北京时间（2026-11-26） | 实测入口 | 输入 Token | 输出 Token | 网关记录费用（USD） |
|---|---|---:|---:|---:|
| 20:52:48 | Artisan 普通文本 | 26 | 8 | 0.000132 |
| 20:52:50 | Artisan 流式文本 | 27 | 9 | 0.000144 |
| 20:52:52 | 工具首轮 | 69 | 17 | 0.000308 |
| 20:52:53 | 工具续轮 | 99 | 7 | 0.000268 |
| 20:52:56 | 应用 HTTP 流式文本 | 27 | 9 | 0.000144 |
| 20:54:49 | 修复后的结构化输出 | 44 | 12 | 0.000208 |
| 合计 | 6 次上游调用 | 292 | 62 | 0.001204 |

工具两轮合计 168 输入、24 输出 Token，网关费用为 0.000576 USD。两条调用明细与 SDK 汇总一致。

| 北京时间（2026-11-26） | 失败请求 | 后台依据 |
|---|---|---|
| 20:52:50 | 严格结构化输出 400 | 已到达 OpenAI 上游，错误指出响应 Schema 必须提供 `additionalProperties: false`。修复后复测成功。 |
| 20:52:54 | Embeddings 503 | `/v1/embeddings` 路由暂不可用，错误记录未关联上游账号。向量实机仍待成功验收。 |
| 20:52:55 | Responses 图片工具 403 | 当前 `OpenAI` 分组未启用图片生成，错误记录未关联上游账号。媒体实机仍待成功验收。 |

**供应商实际结算待核对。** 0.001204 USD 是 Sub2API 后台六条成功记录的计费合计。OpenAI OAuth Pro 账号的供应商实际结算需要账单或获授权的报表作为凭据。三次失败均未匹配到成功用量记录，费用仍未知。

## AI-03 持久化工具审批与恢复

具名会话 Agent 通过 `prompt_with_approvals` 返回已完成响应或待审批批次。应用用会话 ID、审批 ID 和同一参与者读取待审批项，使用 `resume_approval` 提交决定。SQLite 或生产会话数据库保存恢复快照，绑定参与者与 Agent 类型。协议、基础地址、凭证和连接头的摘要保护上游回放，同一连接在其他作用域或进程可以恢复。快照记录调用 ID、提示词、已完成的步骤、用量及原始协议状态，不保存连接凭证。审批窗口为 24 小时。

缺项、未知项、失效的连接和其他参与者均在工具执行前拒绝。批准执行原参数，编辑用批准的参数执行，拒绝将可选原因作为失败结果交给模型。数据库先原子领取，再执行工具。完成后在同一数据库事务写入会话消息并清空快照。如果执行期间中断，旧领取仍阻挡再次执行。当前批次覆盖普通会话文本，流式审批事件、附件和审批 Fake 另行验收。

独立 Artisan 发布并安装迁移，在隔离应用创建待审批。SQLite 和原生 PostgreSQL 18.7 的实际 HTTP 进程核对参与者、连接凭证变更、过期、缺失与额外决定，跨两个进程并发提交仅一个调用完成。验证编辑、拒绝、上游仅续接一次、会话历史和累计用量。包测试、严格 Clippy 和三语文档均通过。

- [固定 Laravel ResolvesPendingApprovals](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Contracts/ResolvesPendingApprovals.php) · [固定 Laravel ResumesToolApprovals](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Providers/Concerns/ResumesToolApprovals.php)

审批领取后将该步骤的完整结果按工具调用 ID、名称和顺序核对，写入私有快照的 `resolved_results`。同结果可重复提交，变更结果会被拒绝。旧快照原 JSON 的附件内容或引用随原记录保留，新快照继续只使用附件引用。独立进程通过 `PendingConversation::approval_results_for` 可在模型续跑期间读取结果。正常完成、失败历史提交或再次暂停继续走原事务边界。进程中断后仍可读取结果，领取状态防止再次执行工具。审批恢复后的单行暂停历史合并仍单独核对。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 实测独立 Artisan 创建审批、HTTP 领取、本地 Responses 上游阻塞、独立 HTTP／Artisan 读取与身份隔离。强制终止续跑进程后保留结果并拒绝重复执行。SQLite 注入快照写入失败时，上游收到零次后续请求，独立 HTTP 读取含原工具结果的失败历史。三库复验旧附件、重复审批、SSE 三种格式、并发领取及私有附件归档；SDK 16 项、严格 Clippy 和三语文档构建通过。

- [固定 Laravel DatabaseConversationStore::storeApprovalResults](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Storage/DatabaseConversationStore.php#L635)

编辑审批使用同一份 `ExecutionPlan` 的参数执行工具、记录私有快照的 `edited_arguments` 和完成后写入公开工具步骤。调用 ID、顺序与原生协议块保持原样。OpenAI Chat 的普通工具重放直接使用执行参数。OpenAI Responses、Anthropic、Gemini 的同连接同模型原生块沿用各自回放规则，切换凭证后使用通用历史中的实际参数。保留未编辑调用的参数及原结果类型。

独立 Artisan 创建审批和检查运行中快照，真实 HTTP 不同进程执行审批、查看历史、同连接重放和更换凭证重放。SQLite 覆盖 Chat Completions、Responses、Anthropic Messages 和 Gemini，PostgreSQL 18.7 额外验收 Responses，MySQL 8.4.13 额外验收 Gemini。本地模拟检查原金额 80、执行金额 40、拒绝项保留金额 30、只执行一次及签名块完整。另复验 SQLite 旧附件快照、重复审批、中断和结果写入失败。SDK 16 项、严格 Clippy 及三语站点通过。

- [固定 Laravel TextGenerationLoop::resolveApprovalResults](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/TextGenerationLoop.php#L978) · [Claude 工具推理回放](https://platform.claude.com/docs/en/build-with-claude/thinking-tool-workflows) · [Gemini GenerateContent 思考签名](https://aistudio.google.com/docs/thought-signatures?generate-content=true)

## AI-03 流式工具审批事件

具名会话的 `stream_with_approvals` 与普通审批复用数据库快照、领取和连接范围。当前批次的任一工具需要审批时，先保存快照，再交付工具声明及 `tool_approval_request`。流在审批处结束，会话历史等恢复完成后再持久化。默认 SSE 携带会话 ID 和审批批次 ID，Vercel 发送标准的逐工具审批帧及批次数据，AG-UI 用中断结果及元数据交付。后续可从另一进程用 `resume_approval` 提交决定并取得普通响应。流式继续输出、框架级审批事件以及审批输入适配仍待验收。

SQLite 和原生 PostgreSQL 18.7 上，以独立 Artisan 消费 SDK 流，检查审批项已保存。真实 HTTP 分别通过默认 SSE、Vercel 与 AG-UI 读取审批，在另一进程恢复一次，再验证旧审批无法重用。OpenAI Responses 上游使用本地 TCP 服务，相关包测试、严格 Clippy 和三语文档通过。

- [固定 Laravel ToolApprovalRequest](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Streaming/Events/ToolApprovalRequest.php) · [Vercel 数据协议](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Streaming/Protocols/VercelDataProtocol.php) · [AG-UI 协议](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Streaming/Protocols/AgentUserInteractionProtocol.php)

## AI-02 文本用量明细

`Usage` 保留已报告的缓存读取、缓存写入与推理 Token。按固定 Laravel `TextUsage` 语义，提示总量包含缓存，输出总量包含推理。Anthropic 增加原始输入计数之外的两类缓存。Gemini 增加候选输出之外的推理量。OpenAI 两种协议保持上游总量并保留可用明细。流中的部分计数更新按同一步骤合并，跨工具步骤累计。生成或存储旧用量时缺少明细仍可读取，序列化会省略未报告的明细。

独立 Artisan 通过四种本地 TCP 协议验证总量、缓存和推理字段。真实 HTTP 验证 Vercel 流及拆帧字段合并，Chat 与 Responses 还实际执行两步工具回合并核对累计用量。SDK 与网关包测试、严格 Clippy 和三语文档验收通过。

- [固定 Laravel TextUsage](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Responses/Data/TextUsage.php) · [固定 Laravel Anthropic 用量](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Anthropic/Concerns/ParsesTextResponses.php)

## AI-03 框架级工具审批事件

`ToolApprovalRequested` 在审批快照写入后经框架事件分发器发送，覆盖普通与流式请求。监听器可用会话和审批 ID 读取同一份待审批项。`ToolApprovalResolved` 在成功恢复完成或新审批快照保存后发送，带上原调用 ID 和该批审批项的工具结果。事件包含参与者和 Agent 类型，可按会话关联；监听器失败只记无载荷的诊断，已保存的审批状态继续有效。无效审批决定、过期请求和重复提交不发处理完成事件。

独立 Artisan 及真实 HTTP 在 SQLite 和原生 PostgreSQL 18.7 上注册类型化监听器。监听器实际读回待审批和已保存会话状态，核对流式／普通请求、审批结果和并发只发一次。SDK 与网关包测试、严格 Clippy 和三语文档验收通过。

- [固定 Laravel ToolApprovalRequested](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Events/ToolApprovalRequested.php) · [固定 Laravel ToolApprovalResolved](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Events/ToolApprovalResolved.php)

## AI-03 子 Agent 工具

`AgentTool` 将具名子 Agent 注册为普通工具，默认使用类型名。实现固定 Laravel `CanActAsTool` 对应的方法后，通过 `into_tool()` 指定名称与说明。`task` 按框架 JSON Schema 声明为必填字符串。子 Agent 使用自己的连接、模型、指令和消息，父 Agent 只提交独立任务。结构化子 Agent 可用 `with_structured_output()` 返回 JSON 文本。当前工具结果在子 Agent 完成后交付，子 Agent 流式活动与父子调用追踪仍待验收。

独立 Artisan 验证显式工具名、普通子 Agent 与父工具续接。真实 HTTP 验证默认名、结构化结果和非法参数，断言非法调用没有向子连接出站。父应用用 Responses 协议，子 Agent 用独立 Chat 兼容连接，握手凭证、模型和父历史隔离由实际 TCP 请求验证。SDK 与网关包测试、严格 Clippy 和三语文档验收通过。

- [固定 Laravel CanActAsTool](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Contracts/CanActAsTool.php) · [固定 Laravel AgentTool](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/AgentTool.php)

## AI-03 子 Agent 工具 Schema 国际化

AI 服务提供者将 18 种 BCP 47 语言的默认工具文案作为包资源嵌入。框架沿用 `Lang`、包命名空间、应用覆写及回退机制，运行产物可直接加载译文。生成工具 Schema 时解析默认委托说明及 `task` 参数说明。委托说明使用 `:agent` 替换工具名，应用自定义说明直接使用。工具名、`task`、类型和必填约束沿用既有契约。

独立 Artisan 核对简中、繁中、英文、日文及未知语言回退。真实 HTTP 在同一应用、同一个 Agent 上并发验证全部 18 语，实测应用逐项覆写、法语回退、日本语流式工具续步及自定义说明。消费进程的应用根只提供覆写文件，默认译文来自运行产物。框架语言路径与语言选择回归、AI 与网关包测试、严格 Clippy 和三语文档均通过。其他内置工具说明、Artisan 和错误文案按国际化计划继续审计。

- [固定 Laravel 工具说明与参数契约](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/AgentTool.php)

## AI-03 相似度工具 Schema 国际化

`SimilaritySearch` 的默认工具说明和 `query` 参数说明通过 AI 包语言资源解析，包含 18 种语言。`with_description` 保留应用文字，字段名、类型与必填约束沿用原契约。目前已检查 SDK 内的 `AgentTool` 与 `SimilaritySearch` 两个内置工具的 Schema 文案。框架新增工具时继续沿用同一语言资源。

独立 Artisan 使用五种语言和回退场景。真实 HTTP 在同一应用进程并发检查 18 语出站 Schema，覆盖流式调用、自定义说明和子 Agent 现有翻译回归。包测试、严格 Clippy 与三语文档通过。

## AI-01 模型选择交互国际化

`ai:select-model` 的连接选择、模型选择、手填、保存确认和配置缓存提示从 AI 包语言资源读取，覆盖 18 种 BCP 47 语言。用原生 Artisan 关联选择返回数字键，再从目录取得原始模型 ID。翻译后的标签无需参与解析，数字菜单、环境变量、模型 ID 和非交互输出维持原契约。

实际 Artisan 通过本地 HTTP 模型目录验证 18 种语言、覆写、未知语言回退、连接选择、手填、保存及非交互输出。真实应用 HTTP 在同一进程并发读取各语言交互资源。包测试、严格 Clippy 和三语文档通过。

模型选择自身的缺参、模型 ID 校验、连接选择和环境变量写入限制使用同一语言资源。实际 Artisan 逐语言核对退出码、原样模型 ID、环境变量名、错误路径无写入及成功保存。真实 HTTP 并发通过 `Artisan::call` 实测缺参的译文、法语覆写和未知语言回退。供应商及框架底层错误文案随各自入口继续核对。

### AI-07 Artisan 命令帮助

`ai:models`、`ai:prompt`、`ai:select-model`、`make:agent`、`make:tool` 和 `make:agent-middleware` 通过包语言资源提供 18 语命令与参数说明。框架在生成帮助时读取当前语言，应用可覆写包译文。当前语言的通用参数说明优先于回退语言的单命令说明。

实际 Artisan 在全部 18 语检查六个命令的 JSON 帮助，确认命令、参数、选项、默认值及用法元数据一致。验证日文 `--help`、繁中 `list`、法语应用覆写和未知语言回退。真实 HTTP 同进程并发检查各语言通过 `Artisan::call` 得到的说明。AI 与网关包测试、框架命令回归、严格 Clippy 和三语文档通过。

### AI-04 生成器交互国际化

`make:agent`、`make:tool` 和 `make:agent-middleware` 复用包语言资源生成类型名称提示、结构化输出确认、创建结果、模块声明提示和名称校验错误。应用可逐条覆写，生成结果的 `:path` 使用实际相对路径。命令选项、退出码、类型名称和模板维持原契约。

实际 Artisan 验证 18 语交互和文件生成、缺参、名称不合法和已存在文件，并检查同名模块、法语应用覆写和未知语言回退。真实 HTTP 在同一进程通过 `Artisan::call` 并发验证各语言的结果与结构化源文件。SDK 严格 Clippy 与三语文档验收通过。

### AI-07 网关 Artisan 命令帮助

网关的 11 个运维命令使用独立的 `ai-gateway` 包语言资源，覆盖 18 种 BCP 47 语言。类型命令和闭包命令共用框架的帮助解析。应用可覆写命令及参数说明，命令名、参数名、选项和用法元数据在各语言下相同。

实际 Artisan 逐语言检查 11 个命令的 JSON 帮助，覆盖 `--help`、`list`、法语应用覆写、未知语言回退以及 SDK 与网关各自的语言资源。真实 HTTP 在同一应用进程并发通过 `Artisan::call` 检查全部 18 语。AI 与网关包测试、框架 Artisan 回归、严格 Clippy 和三语文档通过。网关运行时错误文案继续按国际化计划审计。

### AI-02 流式 UI 错误文案

Vercel 和 AG-UI 在构造 HTTP 响应时解析当前语言，异步消费正文期间使用该译文。上游错误仍以通用提示显示，Vercel 的 `error`、`errorText` 和 `[DONE]` 以及 AG-UI 的 `RUN_ERROR`、`message` 和运行标识沿用协议约定。应用可覆写 `ai::errors.stream`。

实际 Artisan 检查消费应用路由。真实 HTTP 对两个协议并发实测 18 语、法语覆写、未知语言回退、上游错误脱敏及终帧。SDK `ui_stream` 回归、严格 Clippy 和三语文档通过。

### GW-03 鉴权与 SSE 错误文案

OpenAI、Anthropic、Gemini 网关的凭证拒绝与 SSE 终止失败提示使用请求语言。流式响应在返回正文前解析提示，原生转发和跨协议转换均保留译文。响应状态、鉴权头、协议错误字段、事件类型和终止行为沿用原契约。

实际 Artisan 核对路由，真实 HTTP 在同一应用并发验证 18 语、语言请求头、法语覆写、默认语言回退、缺凭证和上游中断。覆盖原生 Chat Completions、Anthropic Messages、Gemini 流，以及 Responses 转 Anthropic 流。协议错误体精确比较，上游诊断未出现在客户端响应。网关包测试、严格 Clippy 与三语文档通过。其他运维错误和协议校验文案继续按计划核对。

### AI-03 审批错误文案

`AiError` 的审批决定无效、未找到、过期、重复或执行中、等待审批、不可恢复提示使用当前语言。错误变体及其数据维持原契约。应用提供的审批理由沿用原文，可在应用渲染时通过 `ai::errors.approval` 覆写框架译文。

实际 Artisan 与并发 HTTP 验证 18 语、法语覆写和回退。SQLite 验收覆盖审批键隔离、错误渲染的 404／409／410／422 状态、应用审批理由、跨进程暂停和恢复、三种流式协议，以及持久化后的审批事件。会话、工具、UI 回归、严格 Clippy 和三语文档通过。

### AI-01 Artisan 连接与供应商诊断

`ai:models`、`ai:prompt` 和 `ai:select-model` 的连接配置、凭证、模型目录、供应商状态、响应及结构化输入诊断通过 `ai::errors.console` 渲染，HTTP 状态号原样传入。解析错误类型及 `ai:models --json` 的目录数据沿用原契约。

实际 Artisan 与并发 HTTP 验证 18 语、应用覆写、回退及退出码。使用本地 HTTP 上游返回推理 429 和目录 503，检查错误脱敏、配置模型保留、选择输出和机器 JSON 一致。文本、模型选择回归、严格 Clippy 和三语文档通过。

### AI-02 流式错误回调

`StreamableAgentResponse::catch` 对照固定 Laravel 流响应的错误回调。流消费失败时，已登记的回调按顺序各执行一次，并将原始错误交给消费端。Rust 在 `stream(...).await` 启动请求，启动失败从该结果返回。HTTP 正文异步消费时，回调仍可使用登记时的应用容器。

实际 Artisan 检查正常结束、流中错误、两个回调的调用顺序和启动失败。实际 HTTP 分别检查 Vercel 与 AG-UI 的正常结束、上游流中断、错误脱敏和回调应用作用域。文本、工具与 UI 流回归、严格 Clippy 和三语文档通过。

- [固定 Laravel StreamableAgentResponse](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Responses/StreamableAgentResponse.php)

### AI-02 逐事件消费

`StreamableAgentResponse::each` 在同一流上逐事件执行回调，返回 `false` 暂停遍历。响应继续持有该上游流，重播已读事件时沿用原 ID。随后可继续遍历、取原生流或输出 HTTP，直接消费的路径无需缓存已读事件。

实际 Artisan 和 HTTP 对照完整遍历、提前停止、重播、错误传播及上游请求次数。消费端覆盖默认 SSE、Vercel 和 AG-UI，原生流的事件 ID、顺序与用量经过精确比较。文本、工具与 UI 流回归、严格 Clippy 和三语文档通过。

### AI-02 流式完成回调

`StreamableAgentResponse::then` 在流完整消费后交付 `StreamedAgentResponse`，登记在完整 `each` 之后时立即交付。响应按固定 Laravel 的工具步骤规则合并文字与用量，并提供原始事件、最后一步模型、工具调用及结果。等待审批的流在交付审批终帧前回报已持久化的审批批次与会话 ID。上游错误沿用 `catch`。推理、引文及完整对话元数据随各自契约继续核对。

实际 Artisan 与 HTTP 通过两步 OpenAI Responses／Chat Completions 模型调用检查文字分段、9 输入及 5 输出 Token、工具结果、事件 ID、顺序与错误边界。实际 SQLite 及默认 SSE、Vercel、AG-UI 核对暂停回调中的持久化审批批次，独立 HTTP 进程恢复。文本、工具、会话及 UI 回归、严格 Clippy 和三语文档通过。

- [固定 Laravel StreamedAgentResponse](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Responses/StreamedAgentResponse.php) · [固定 Laravel TextDelta](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Streaming/Events/TextDelta.php)

### AI-03 技能文档与加载工具

`Skill::new` 定义内联技能，`Skill::from_directory` 读取 `SKILL.md` 的 YAML 元数据及 Markdown 正文。YAML 使用纯 Rust 的 Saphyr，配套文件通过框架已使用的 `cap-std` 在对应目录中读取。应用通过 `LoadSkill::new` 显式注册技能。出站 Schema 仅列技能名与描述，模型请求时才发送正文及配套文件或读取指定 UTF-8 文件。重复名称按配置中首次出现的技能处理。

真实 Artisan 与并发 HTTP 经本地 Responses 上游验证 18 语、应用覆写、回退、目录名称回退、折叠 YAML、畸形文档、内联资源、256 KiB 配套文件边界、二进制及目录外路径。流式工具执行恢复创建流时的语言，防止 HTTP 正文延后读取时切回默认语言。SDK 工具、UI 与会话回归、严格 Clippy 和三语文档通过。

- [固定 Laravel Skill](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Skills/Skill.php) · [固定 Laravel LoadSkill](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/LoadSkill.php) · [Agent Skills 规范](https://agentskills.io/specification) · [Saphyr 文档](https://docs.rs/saphyr/0.4.1/saphyr/)

### AI-03 Agent 技能自动装配

固定 Laravel 的 `HasSkills` 使用 Rust `Agent::skills` 对应，匿名 Agent 使用 `with_skills` 声明。技能来源可为内联实例、集合目录或延迟解析函数。显式 `LoadSkill::from_sources([])` 发现应用的 `resources/skills`。已有的 `LoadSkill` 与 Agent 声明合并为一个工具，已有同名技能优先。普通调用、流式调用及审批恢复共用工具装配。

实际 Artisan 验证匿名声明、已有加载器合并、默认目录和解析器只在实际调用时执行一次。真实并发 HTTP 验证具名 Agent 的流式用法及 18 语。独立 SQLite 的审批暂停与恢复验收确认两段出站 Schema 都包含声明的技能。SDK 工具、UI 与会话回归、严格 Clippy 和三语文档通过。

- [固定 Laravel HasSkills](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Contracts/HasSkills.php) · [固定 Laravel 技能来源与合并](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/LoadSkill.php)

### AI-03 MCP 客户端工具桥

`McpTool` 接收应用已连接的官方 Rust MCP 对端及选定的工具定义。模型侧使用固定 Laravel 的 `mcp_tools_` 前缀，MCP 请求使用原工具名与原参数。服务器描述、标题与名称依次用于工具说明，应用可读取 annotations 并设置审批策略。结构化结果优先输出 JSON，普通内容合并文本，工具错误按当前语言保留服务返回的正文，协议错误走 SDK 通用失败路径。

复用框架 JSON Schema 的本地引用解析及类型转换。适配器构造时，当前无法表示的 Schema 返回 `AiError::InvalidSchema`。固定 Laravel 的剩余归一化规则及失败时空 Schema 的行为需要继续复核。

实际 Artisan 与并发 HTTP 经本地 Responses 上游和官方 RMCP 双向字节流核对会话协商、工具发现、Schema 中的本地引用、出站参数、结果、协议故障、先审批后执行及 18 语覆写和回退。框架 JSON Schema 回归、SDK 严格 Clippy 和三语文档通过。

独立 Artisan 进程启动官方 RMCP Streamable HTTP 服务。另一个 Artisan 进程连接、发现工具，并用同一会话完成两次 Agent 调用，服务端记录核对了会话复用。真实 HTTP 流式应用也经独立 MCP 服务调用，核对结果、错误回传和审批边界。传输由消费应用启用，三语文档记录对接方式。

- [固定 Laravel McpTool](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/McpTool.php) · [MCP Tools 契约](https://modelcontextprotocol.io/specification/2025-11-25/server/tools) · [官方 RMCP 3.12.0](https://docs.rs/crate/rmcp/3.12.0)

### AI-03 MCP 本地服务工具桥

`McpServerTool` 使用已声明的 MCP 工具定义和应用处理函数。工具保留服务侧名称，处理函数收到 `CallToolRequestParams`，输出 `CallToolResult`，沿用共享的参数 Schema、annotations、审批及输出转换。Rust 用类型明确的异步处理函数传递参数，固定 Laravel 的同类入口通过容器注入 MCP 请求。

实际 Artisan 和并发流式 HTTP 核对本地直接执行、应用作用域、工具原名、参数、结构化及文本返回、审批拦截和 18 语错误文案。同一处理函数经官方 RMCP 客户端再次验证，确认两类适配器均可使用。严格 Clippy 和三语文档通过。

- [固定 Laravel McpServerTool](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Tools/McpServerTool.php)

### AI-02 Responses 文本流网页引文

原生 Responses 文本流将 URL 标注转换成 SDK `citation`、Vercel `source-url` 和 AG-UI `CUSTOM` 事件。按上游输出项及内容位置关联文本消息，已结束的文本仍可关联后到引文。完成回调从 `citations()` 读取原顺序及上游提供的起止位置。网页引文接受 HTTP 与 HTTPS 地址。

实际 Artisan 和 HTTP 核对两个交错输出、提前和迟到引文、分片 UTF-8、文本 ID、无标题来源、文件标注和非网页地址。三种 SSE 输出及最终 Token 用量逐项比较。文本、工具与 UI 回归、严格 Clippy 和三语文档通过。

带工具的 Responses 流复用相同的文本分段与引文关联。实际 Artisan 和 HTTP 通过本地上游执行两步调用，核对首轮引文、工具参数与结果、后续引文、消息归属及三种 SSE 输出。每轮可使用相同的上游输出项 ID，SDK 为各轮分配独立的文本消息 ID。文本、工具与 UI 回归及严格 Clippy 通过。非流式元数据继续按所属契约验收。

- [固定 Laravel Citation 事件](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Streaming/Events/Citation.php) · [OpenAI Responses 流式事件](https://developers.openai.com/api/reference/resources/responses/streaming-events)

### AI-02 文本响应 Meta

`AgentResponse::meta()` 提供来源连接、实际模型和网页引文。普通 OpenAI Responses 从文本内容项按上游顺序读取引文，保留分段的原始位置。普通结构化响应通过同一 Rust 接口提供元数据，序列化结果继续遵循应用 Schema。完成的流也可经聚合响应访问已发出的引文。自定义文本驱动可用 `AgentResponse::new` 创建带空引文列表的响应。

真实 Artisan 与 HTTP 通过隔离协议服务核对普通和结构化 Responses、Chat Completions 空引文、Unicode 分段、空缺标题、非法 URL 和文件注解、未知注解结构、实际模型以及计数。文本、工具、UI、会话回归、SDK 严格 Clippy 和三语站点通过。缓冲 Responses 工具调用的顶层 `meta` 取终步，与固定基线 `RunContext::recordedResponse` 一致。Artisan 与 HTTP 核对首轮与终轮各自带引文，终步模型和引文正确、结果回传一次、引文不随工具结果回传上游，计数为 18 输入和 6 输出 Token。仓库回归、SDK 严格 Clippy 和三语文档通过。内置文本驱动的非流式 `AgentResponse.steps` 公开每步的实际模型、结束原因、用量、引文、摘要及本步工具结果。Artisan 与 HTTP 验收普通和结构化 Responses、Chat 兼容、两步工具调用及终步元数据。记录首步 11 输入／3 输出、终步 7 输入／3 输出，顶层汇总 18／6。旧审批快照至少可缺失结束原因，SQLite 独立进程复验恢复为 `unknown`，后续步骤仍按上游报告，工具只执行一次。Rust 的 `ApprovalResponse::Completed` 以 `Box<AgentResponse>` 承载完结响应。仓库回归、SDK 严格 Clippy、三语站点通过。流式完成回调通过 `result.response.steps` 交付每步记录。真实 Artisan 与 HTTP 核对普通 Responses、两步工具续轮、原生／Vercel／AG-UI SSE 和逐步引文与摘要。两步用量为 13 输入／5 输出／3 推理和 2 输入／3 输出／1 推理，回调同时保留原始事件。SQLite 独立进程验收审批终帧交付前的回调，确认该步记录两项工具调用、结束原因为 `tool_calls`、结果为空，后续恢复工具仅执行一次。仓库文本、工具、UI 测试与 SDK 严格 Clippy 通过。

上游终帧的实际模型和用量同步进入 Rust 完成回调、逐步事件及 `AgentStreamed`。真实 Artisan 与 HTTP 在流开始时使用请求模型 `fixture`，在普通和带工具的 Responses 终帧分别返回 `resolved-first`、`resolved-final`，回调和逐步事件按步骤读取实际值。Artisan 同时验收 `each` 完整遍历后才登记回调。原生、Vercel、AG-UI 仍按各自契约发送。SQLite 联调在持久审批终帧交付前读取实际模型及 31 输入／20 输出 Token，回调逐步与顶层用量一致，工具后来只执行一次。仓库回归、SDK 严格 Clippy 和三语站点通过。

非流式 Anthropic Messages 从网页搜索引文和成功网页抓取中提取 URL，Gemini GenerateContent 从所选候选的 Grounding 网页块提取 URL。两者保留首次出现的 URL 及公开标题。真实 HTTP 与 Artisan 覆盖普通和两步工具调用、重复 URL、非法 URL、其他候选和非网页来源的隔离、正常用量，以及私有签名／抓取正文保留在协议所需范围。仓库工具回归、SDK 严格 Clippy 和三语站点通过。原生 Messages 流也已验收。实际 Artisan 与 HTTP 覆盖普通和两步工具调用、网页引文增量、抓取来源、同一步去重、文本结束后的消息关联以及原生 SSE、Vercel 和 AG-UI 三种输出。完成回调可读取公开引文，续轮保留 Anthropic 所需的原始引用字段。两步用量为 15 输入和 8 输出 Token。文本、工具和 UI 回归、SDK 严格 Clippy、三语站点均通过。Gemini 原生流已通过相同三种输出和 Artisan。验收普通和两步工具调用，首帧／终帧 Grounding 来源、跨帧去重、所选候选、工具签名隔离及仅含用量的尾帧。SDK 严格 Clippy、仓库工具回归和三语站点通过。

- [Anthropic Web Search](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool) · [Anthropic Web Fetch](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-fetch-tool) · [Gemini Grounding](https://ai.google.dev/gemini-api/docs/google-search)

- [固定 Laravel Meta](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Responses/Data/Meta.php) · [Laravel Responses 解析](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/OpenAi/Concerns/ParsesTextResponses.php) · [OpenAI Responses 注解](https://developers.openai.com/api/reference/resources/responses/methods/create)

### AI-02 Responses 公开推理摘要

Responses 的公开摘要按上游输出项与摘要位置生成 `reasoning_start`、`reasoning_delta`、`reasoning_end`。原生、Vercel 和 AG-UI 分别沿用各自的协议形状。完成回调按首次出现的摘要块顺序合并。增量、直接终帧及工具续轮共用相同解析。SDK 只识别独立的公开摘要帧。

实际 Artisan 与 HTTP 核对普通及带工具的两步 Responses，请求中的 `reasoning.summary`、交错分块、重复终帧、缺少增量的完整块、上游私有字段隔离及 Token 用量。两步调用合计 15 输入、8 输出及 4 推理 Token。文本、工具和 UI 回归、严格 Clippy 和三语文档通过。

非流式 Responses 从公开摘要项读取 `response.reasoning`。同一个推理项内拼接摘要片段，各项以空行分隔。结构化响应通过 Rust 字段读取摘要，非流式工具调用取最后一步摘要。实际 Artisan 和 HTTP 覆盖普通／结构化／工具续轮、空白和未知摘要类型、私有推理与加密内容隔离以及 Chat 兼容路径的空摘要。流的原接口与聚合字段用独立进程复验一致；原生 SSE、Vercel、AG-UI、严格 Clippy、仓库回归和三语站点通过。

- [固定 Laravel ReasoningDelta](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Streaming/Events/ReasoningDelta.php) · [OpenAI 推理摘要文档](https://developers.openai.com/api/docs/guides/reasoning?api-mode=responses&site_locale=en) · [OpenAI Responses 流式事件](https://developers.openai.com/api/reference/resources/responses/streaming-events)

### AI-02 Agent 生成事件

非流式 SDK 每次供应商尝试分派 `PromptingAgent`，成功后交付含逐步响应的 `AgentPrompted`，失败时交付含原类型错误的 `AgentFailed`。事件使用已解析的连接与请求模型、当前 Agent 类型及出站请求的调用 ID。提示参数为步骤中间件执行前的快照。SDK Fake 复用相同调用链，并提供提示断言入口。

隔离服务经实际 Artisan 及 HTTP 验证普通和结构化请求、401、跨协议切换、全部候选失败、两步工具成功和续轮失败。监听器看到每次尝试先开始、再成功或失败，出站请求与监听器记录的调用 ID 一致。两步工具成功事件带两个步骤及一个工具结果。续轮 503 后只分派原连接的失败事件。SDK 文本、工具和会话回归及严格 Clippy 通过。

流式供应商起始分派 `StreamingAgent`，完全消费分派 `AgentStreamed`，启动和消费错误分派 `AgentFailed`。有完成事件监听器或对应 Event Fake 时收集完整响应。只有失败事件监听器时仍逐帧传输。实际 Artisan 与 HTTP 核对 Responses 到 Chat 切换、两轮工具调用、首事件交付前失败和已发送部分内容后断开。SQLite 会话监听器用事件中的会话 ID 当场查到已写入的消息，数据库触发器注入落库失败时只收到错误事件。Artisan Event Fake 记录一条完整成功事件。SQLite 审批从首选连接 503 切换后暂停，原生／Vercel／AG-UI 仍交付审批终帧，Artisan 验证完成事件、回调和已存审批批次一致。SDK 与框架 Event 回归通过 27 项，框架事件缓存的两个子进程用例由各自主测试启动。SDK 严格 Clippy 和三语站点通过。

- [固定 Laravel GeneratesText](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Providers/Concerns/GeneratesText.php) · [固定 Laravel StreamsText](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Providers/Concerns/StreamsText.php)

非流式 SDK 的 `StartingStep` 在步骤中间件处理后交付请求快照及此前的工具结果，`StepCompleted` 在本步工具执行前报告响应，`StepFailed` 持有类型化错误。两个终态事件提供耗时，步骤从零开始。Artisan 与 HTTP 覆盖结构化及普通文本、两步工具、首步与续轮 503、401、连接切换、每步改写模型及短路中间件。事件区分请求模型和供应商实际模型。短路响应无出站请求及逐步事件。SQLite 审批由独立 HTTP 进程恢复时，模型步骤事件沿用存档调用 ID、原连接、Agent 类型和续轮编号，工具仍只执行一次。SDK 回归、严格 Clippy 和三语站点通过。

模型步骤事件覆盖普通和带工具的流。Artisan 与 HTTP 实测 Responses 到 Chat 的启动 503 切换、部分文本后断开、工具续轮 503、原生／Vercel／AG-UI 两步成功以及晚到用量帧。两步请求均为 `backup-model`，事件报告实际的 `backup-first-rev` 与 `backup-final-rev`，用量为 12 输入／5 输出及 6 输入／3 输出，跟随模型步骤交付。修复兼容 Chat 流从终帧解析的结束原因在构建步骤响应时丢失。SQLite 审批独立进程确认 `StepCompleted` 在工具运行前报告 31 输入／20 输出、两项待批调用及原调用 ID。文本、工具、会话、UI 回归及 SDK 严格 Clippy 通过。

- [固定 Laravel TextGenerationLoop](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/TextGenerationLoop.php) · [固定 Laravel RunContext](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/RunContext.php)

### AI-03 工具执行事件

`InvokingTool` 在实际工具处理器执行前分派，返回后分派 `ToolInvoked` 或 `ToolFailed`。事件共用 Agent 调用 ID、工具执行 ID 与处理器收到的参数。实际 Artisan 和 HTTP 覆盖非流式、流式、跨连接切换、工具成功与类型化参数错误。日志确认每个调用依次为起始事件、一次处理器执行、一个终态事件。供应商续轮收到原有通用失败提示。SQLite 审批独立进程验证编辑将金额由 80 改为 40，事件和处理器沿用审批快照中的工具 ID，等待、拒绝、过期与重放时无额外执行事件。工具、会话、UI 回归、SDK 严格 Clippy 与三语站点通过。

- [固定 Laravel InvokingTool](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Events/InvokingTool.php) · [固定 Laravel ToolInvoked](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Events/ToolInvoked.php) · [固定 Laravel ToolFailed](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Events/ToolFailed.php)

### AI-02 具名供应商故障转移

`Agent::providers` 与匿名 Agent 的 `with_providers` 可声明有序具名连接。连接内指定模型或使用当前档位。单次调用显式指定供应商时只使用指定连接。切换时发布 `AgentFailedOver`，候选请求、事件和原生文本流沿用同一调用 ID。需要向自托管端传递该 ID 时沿用已有的 `forward_usage_context`。固定基线的数组供应商使用逐连接模型映射。旧版工作树 `6d1a81924` 未发现对应 AI SDK 实现。

隔离应用以真实 Artisan 和 HTTP 调用 Responses 与 Chat Completions 模拟服务。已核对 HTTP 402、429、502、503、504、520、522、524 触发顺序切换，401 和显式连接直接返回，全部候选失败返回末次错误。已核对按连接选择模型、结构化 Schema、首事件前传输中断转移、交付首事件后断开原流，以及监听器每次切换只收到一个与出站请求同 ID 的事件。文本、工具、UI 回归和三语站点通过。

带工具的流在首个模型步骤交付事件前可切换。模拟上游用实际 Artisan 与 HTTP 核对了初始 503 和首帧前传输中断。切换成功后工具执行一次，续轮向选中连接发送结果，全部步骤与切换事件关联同一调用 ID。已执行工具后的续轮返回 503 时保留原流的工具结果并终止，未尝试备用连接。仓库回归覆盖初次切换成功、执行一次工具和续轮失败。

按固定 Laravel 基线加入 `Retry-After` 同连接重试。实际 Artisan 与 HTTP 覆盖 OpenAI Responses、有序备选 Chat Completions、Anthropic Messages 和 Gemini GenerateContent。已验证等待 1 秒后重试、同连接最多重试一次、重试耗尽后切换、超长及日期格式不等待、401 不重试、不切换、额度代码和类型立即切换、事件与出站请求沿用原调用 ID。工具阶段复用同一内置文本传输规则，网关原生转发仍使用自己的重试策略。

非流式工具调用可在首个模型步骤失败时切换。成功收到首步结果后固定连接，已执行工具后的续轮错误按原错误返回。实际 Artisan 和 HTTP 核对首步 503、跨协议续轮、同一个调用 ID、只执行一次工具及续轮 503 后无重放。SQLite 独立进程通过 Artisan 及三种 SSE 暂停，在切换后的连接上恢复审批，覆盖并发决定、编辑、拒绝、过期和跨参与者访问。应用提示词中间件目前保持首选连接，以免自动重放中间件行为。

Anthropic Messages 依固定驱动处理 529 和额度正文。实际 Artisan 和 HTTP 覆盖 529 短等待后同连接成功、无等待时切换 Chat Completions、额度类 400 切换、普通 400 与 401 返回原状态、非 Messages 的 529 保留状态、原生流切换后的结束帧及原始正文脱敏。事件和 SDK 可分别识别 `ProviderOverloaded`、`ProviderInsufficientCredits`，同时保留原 HTTP 状态。

- [固定 Laravel AnthropicGateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Anthropic/AnthropicGateway.php) · [Anthropic Messages 错误](https://platform.claude.com/docs/en/api/errors)

按计划收紧 SDK 的重发边界。核心 HTTP 客户端可单独报告首次发送前的建连失败，SDK 的内置文本请求据此切换。已发出请求但结果未知、耗尽超时、已收到成功响应但正文中断时结束本次调用。此裁决替代上文初版对首帧前传输中断的切换行为。已有状态码重试策略仍生效。

实际 Artisan 与 HTTP 核对端口拒绝连接可切换、明确指定连接收到类型化错误、上游读到请求后断开或超时只发出一次请求，以及 200 头后正文中断返回错误。监听器仅收到已确认建连失败的切换事件。40 项框架 HTTP 客户端回归、13 项 SDK 回归、SDK 严格 Clippy 和三语站点通过。

待补其余供应商错误正文分类、应用中间件的故障转移，以及剩余供应商和入口的逐项验收。

- [固定 Laravel Promptable](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Promptable.php) · [供应商与模型列表](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Providers/Provider.php) · [故障分类和重试](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Concerns/HandlesFailoverErrors.php)

### AI-02 DeepSeek 直连与阿里云 Token Plan 交互核对

DeepSeek 官方 `/v1` 基础地址通过 SDK Artisan 发现 `deepseek-flash` 和 `deepseek-v4-pro`。选用 `deepseek-flash`，分别用 `openai-compatible` 和 `openai` 连接实测 Chat Completions 与 Responses。Chat 普通响应、流式响应和自动工具选择两轮通过。Chat 严格 JSON Schema 返回 400，原生 Responses 的严格结构化输出通过。Responses 普通和工具响应通过，独立应用 HTTP 经 SDK 读取实际 Responses 流并核对结束用量。

Chat 思考模式的具名工具选项触发上游 400。使用自动选择并在实际工具续轮回传 `reasoning_content` 后通过，缓冲与分片流分别验证工具历史，测试确认 UI 流不展示保留的推理片段。Chat 工具总量为 695 输入、60 输出 Token，Responses 工具总量为 692 输入、57 输出 Token。它们是上游回报的用量。本次未验证供应商账单、Embeddings 或媒体。

阿里云使用官方文档指定的 Token Plan Responses 地址及隔离的 Codex 0.170.0 交互会话。上游返回 HTTP 403，代码 `AccessDenied.Unpurchased`。本次无成功生成记录。该凭证按官方规定用于兼容 AI 工具的交互链路，百炼按量付费后端另行验收。所有凭证从运行时环境读取，仓库只记录协议、状态和汇总数据。

- [DeepSeek 思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode/) · [Chat Completions API](https://api-docs.deepseek.com/zh-cn/api/create-chat-completion/) · [Responses API](https://api-docs.deepseek.com/zh-cn/api/create-response/)
- [阿里云 Token Plan 使用规范](https://help.aliyun.com/zh/model-studio/token-plan-permissions-and-usage) · [阿里云 Codex 接入](https://help.aliyun.com/zh/model-studio/codex)
