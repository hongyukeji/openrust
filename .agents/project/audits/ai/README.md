# E07 AI SDK 与模型网关契约账本

固定基线、供应商矩阵、分批目标及整体完成条件见 [开发计划](../../plans/laravel-ai-sdk.md)。Laravel AI SDK 固定在 `1d269f2359aa166aaf10b212151bf556dd86a02e`，Sub2API 固定在 `0f6671457132425d1b84f3d80536b53e02942424`。当前进展包括 SDK 文本、模型发现及自托管网关的 OpenAI 文本入口。端点清单和裁决见 [GW-00 网关端点账本](gateway.md)。后续继续按 SDK 公开成员和实际消费者核对。

## AI-01 非流式文本

| 上游位置及成员 | 公开实现及裁决 |
|---|---|
| `src/AiServiceProvider.php` 的包配置、Manager 单例、配置发布 | `AiServiceProvider` 通过已有 Cargo 包发现启用，`Ai::manager` 与容器的 `AiManager` 指向应用级单例。发布标签为 `ai`、`ai-config`。stub 与迁移发布归 AI-03／04。 |
| `src/AiManager.php` 的 `getDefaultInstance`、`setDefaultInstance`、`getInstanceConfig`、`textProvider`、`textProviderFor` 及继承的实例缓存、扩展 | `AiManager` 对应的 Rust 方法为 `get_default_instance`、`set_default_instance`、`get_instance_config`、`text_provider`、`text_provider_for`、`extend`、`forget_instance`。按具名配置惰性解析，应用缺默认连接时错误留在调用时。尚待核对基本方法完整的消费者覆盖、驱动别名及其他能力和生命周期。按需实例在下方单独记录。 |
| `src/Contracts/Agent.php`、`src/Promptable.php`、`functions.php` 的 `instructions`、`prompt`、`agent()` | `Agent::instructions`、`Agent::prompt` 与 `agent()` 可用于单条文本；Rust 以 `prompt_with` 加 `PromptOptions` 实现 Provider／模型／超时覆盖。普通和显式配置验收见包测试。Trait 的 `provider`／`model` 钩子保留单连接的选择优先级。普通用户和助手历史消息在 AI-02 实现。待补 schema、数组／按需 Provider 选择、附件、事件及其余成员。 |
| `src/Providers/OpenAiProvider.php`、`OpenAiCompatibleProvider.php` 和对应 Gateway 的非流式请求与响应 | 用 `async-openai` 的请求类型，OpenRust HTTP 工厂实际发送两种协议。由内部投影解析文本及上游用量，非成功状态返回不带上游载荷的错误。用户确认预设不指定默认模型，各上游模型需显式选择。待补高级模型选项、普通与流式终止细节、响应元数据完整契约和供应商实机。 |
| `src/Promptable.php` 的文本 Fake 及 `Gateway/FakeTextGateway.php` | 可从 `Agent::fake` 或 `Ai::fake` 在作用域内对 Agent 类型安装字符串响应序列。已提供记录、匹配、反向、计数和防止非预期请求。待补按需响应闭包、结构化输出、排队和流式等。 |

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

已下载并按 GitHub 发布记录中的 SHA-256 校验 Sub2API `v0.2.25` Darwin ARM64 发行包。二进制自报编译修订 `348997267d756eab5729bb6b78a275a15d18796a`。在回环隔离环境使用 PostgreSQL 18.7、Redis 8、临时管理员及模拟协议上游，已启动并登录。创建分组返回 HTTP `423 ADMIN_COMPLIANCE_ACK_REQUIRED`，要求管理员确认 `v2026.06.10` 的合规承诺。配置真实分组、API Key 和 OpenRust 请求链路仍待经确认的验收实例，当前网关测试是协议模拟。

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

`make:agent-middleware` 生成到 `app/ai/middleware`，Agent 中间件模板加到 `ai-stubs`。独立 Artisan 验证生成、发布及生成文件编译后的真实调用。流式入口在配置中间件时显式返回未支持错误。Agent Fake 中间件、Provider 选项编辑及流式步骤待后续批次。

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

本批选择把完成的一轮原子持久化，并从第一条提示生成短标题。固定 Laravel 的单独落库及队列标题、可替换存储契约、可配置表名、流式和结构化会话、附件与原始工具历史重建、审批暂停／恢复与并发语义仍待补齐。

## AI-03 流式与结构化持久会话

沿用 `for_participant` 和 `r#continue` 的归属校验与历史构造。持久会话的 `stream` 与 `stream_structured` 在返回流时提供会话 ID，采用任一已有 SSE 渲染器。流式正文照常交付。最后的完成事件在成功落库后发送，截断或客户端提前丢弃的流不会生成半轮记录。工具流的各步用量按已报告的值合并，结构化调用使用现有 Schema 和 JSON 对象解析后再写入。

包测试验证 Fake 的会话输入、结构化输出、流内可见的持久化边界和取消。独立 Artisan 从结构化入口创建会话，独立 HTTP 进程恢复历史、实时发送首段文本、提供存储后的最终事件，并对截断流返回错误。三种真实数据库 SQLite、MySQL 8.4.13 和 PostgreSQL 18.7 均通过 HTTP／Artisan 与迁移回滚。可替换存储、附件、原始工具步骤重放及审批暂停／恢复仍待相应批次。

## AI-05 Reranking Fake 与 Collection

`Reranking::fake` 可接收结果列表或完整响应，`fake_with` 接收闭包。默认使用框架 Collection 打乱输入索引并按限制返回，支持阻止意外调用及 Laravel 风格断言。Fake 使用应用内状态，在 SDK 的正常入口拦截并通过与供应商相同的索引和数量校验。包测试覆盖元数据、回调、随机结果、耗尽和无出站请求。

`Reranks` 为框架 Collection 提供 `rerank` 与 `rerank_with`，应用用闭包选择条目中的文本，返回原始条目和上游排名顺序。独立 HTTP 通过 TCP 模拟 Voyage 验证条目关联，独立 Artisan 验证 Fake 拦截、记录和耗尽。固定 Laravel 的字段路径及多字段提取、Bedrock 和供应商专用选项继续保留为待核对项。

- [Laravel 固定 Collection 与 Reranking 注册逻辑](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/AiServiceProvider.php)
