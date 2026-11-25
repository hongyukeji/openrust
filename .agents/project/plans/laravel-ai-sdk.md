# Laravel AI SDK 与模型网关开发计划

用户确认 E07 的完整目标为 **Laravel AI + Sub2API + DeepSeek Harness（DSH）的模型接入体验**。Laravel AI 定义框架内的 SDK 契约，Sub2API 提供外部协议与模型网关参考，DSH 提供添加供应商、选择协议、发现及选用模型的体验参考。E07 已完成基线固定、生态调研、协议类型试编和初步分批。SDK、网关和模型接入体验的实现及运行验收待办。

## 开发需求

- 将 OpenRust 的 AI 模块对齐固定 Laravel AI SDK。公开入口、目录结构、配置键、默认值、Provider 生命周期、Artisan 命令、测试方式及可观察行为逐项对应 Laravel。Rust 接口使用本项目的命名规范和异步类型，必要适配随契约核对记录依据。
- 按固定版本交付完整适用能力。覆盖 Agent、结构化输出、流、工具及审批、会话、队列、广播、图像、音频、转录、Embeddings、Reranking、Classification、文件与向量仓库，以及用量、故障转移、事件和 Fake。供应商按下方矩阵逐能力验收。
- 开发前参考成熟的 GitHub 项目及 Laravel 生态实现。优先复用经过验证的方案和 OpenRust 已有机制，按实际协议差异决定是否增加依赖或适配层。
- 同时交付三种使用方式。OpenRust 应用能在进程内使用 SDK，能通过兼容或原生协议接入供应商、自建模型服务和 Sub2API，也能启用模型网关，为外部客户端提供兼容 API。网关覆盖协议转换、模型路由与上游账号接入。
- 采用 DSH 式简洁接入流程。内置供应商提供合理默认值，自定义服务使用协议、基础 URL、凭证和模型 ID 即可接入。可发现模型的端点支持选取和显式保存，手动填写始终可用。将 DSH 本身作为 OpenRust 对外网关的实际客户端验收。
- 应用语言词条、机器翻译、内容审核及 Boost／MCP 服务各自保留职责。AI SDK 自身的 MCP 工具桥接纳入 E07。AI Agent 翻译维持后续计划状态，当前阶段暂缓实施。

## 建议实施方案

2026-11-26 用户确认采纳以下方案。固定 Laravel AI SDK 的公开契约作为验收依据，Rust 适配沿用项目规范。

- 新增独立可选包 `openrust-ai`，复用包发现、容器与 HTTP。通过 `Ai`、`AiManager`、`agent()` 及按能力的 Provider／Gateway 保留 Laravel 的调用习惯。数据库查询侧的 Embeddings 通过适配器接入并保留现有缓存和查询行为。
- 首批实现包发现、配置、非流式 `Agent::prompt` 及 Fake。OpenAI Responses 与 OpenAI Compatible Chat Completions 分开验收。优先采用已通过 Rust 1.94 试编的 `async-openai` 协议类型，HTTP 请求由 OpenRust 客户端发送。其余供应商和能力按下文批次补齐。
- 每批从公开 API 编译调用，使用合成输入和本地 HTTP 协议服务实测，涉及 Artisan、Worker 或广播时覆盖相应入口。分别记录协议模拟和供应商实机结果。把已实现的用法与边界同步至简体中文、繁体中文和英文文档，更新 E07 账本并分批本地提交。

## 固定基线与选型

| 对照 | 核对结果与采用范围 |
|---|---|
| [Laravel AI SDK `1d269f2359aa166aaf10b212151bf556dd86a02e`](https://github.com/laravel/ai/tree/1d269f2359aa166aaf10b212151bf556dd86a02e) · [Laravel 文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8`](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/ai-sdk.md) | MIT。以实际 Provider 注册、Contracts、Gateway、Pending、Response、Events、迁移、命令和调用路径为准。文档用于核对开发者入口，源码用于裁决版本内差异。 |
| [Goravel v1.18.0 固定源码](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/ai) | 参考静态注册、请求上下文、媒体存储、流取消及故障转移测试。其 `AI.Agent` 装配、选项及可配置错误规则按 Laravel 基线裁决。 |
| [Prism `385022b5e0fec9be49a5b8e9a468fdafaa8f1267`](https://github.com/prism-php/prism/tree/385022b5e0fec9be49a5b8e9a468fdafaa8f1267) | MIT。参考分能力 Pending／Response、供应商独立协议及 Fake 测试。运行路径保持纯 Rust。见 [Prism OpenAI 适配文档](https://prismphp.com/providers/openai/)。 |
| [rust-genai `2a8a5b39c96028d2143d8754ee2bba42a79e32c5`](https://github.com/jeremychone/rust-genai/tree/2a8a5b39c96028d2143d8754ee2bba42a79e32c5) | MIT／Apache-2.0。多供应商模型与流适配候选，支持注入 reqwest 客户端。注入后仍须验证指定版本能通过 Rust 1.94、保留出站策略及日志边界，并完整映射协议事件。 |
| [async-openai `18a85cba921ccef08b2d058e5080f7083071e39c`](https://github.com/64bit/async-openai/tree/18a85cba921ccef08b2d058e5080f7083071e39c) | MIT。按能力启用 Chat Completions、Responses、Embeddings 等协议类型，声明的 workspace MSRV 为 1.75，实际传递依赖仍需编译核对。首选试用类型特性，经 OpenRust HTTP 发送和解析，保持加密、代理、超时、观测及测试入口一致。若类型与锁定协议不吻合，适配该差异并用 HTTP 用例固定。 |
| [Rig `f4d73dacdfabb3d9daa3f1de091efa490a84c601`](https://github.com/0xPlaygrounds/rig/tree/f4d73dacdfabb3d9daa3f1de091efa490a84c601) | MIT。参考按模型和工具分隔运行步骤、流式测试及检查点。该版本最低 Rust 1.97.1，当前项目为 1.94，本轮作设计参考。 |
| [Sub2API `0f6671457132425d1b84f3d80536b53e02942424`](https://github.com/Wei-Shaw/sub2api/tree/0f6671457132425d1b84f3d80536b53e02942424) | 核对公开协议路由、Responses 转 Anthropic Messages，以及按模型和端点选择上游的组合路由。按固定源码核对协议入口、账号类型、实际能力和运维行为，作为网关实现与实测参考。 |
| [DeepSeek Harness `4b399c39a6d1479cd84ca591b4d7c88e24d38a96`](https://github.com/deepseek-ai/deepseek-harness/blob/4b399c39a6d1479cd84ca591b4d7c88e24d38a96/docs/user/guide/providers.md) | 采用内置供应商和自定义模型 API 两条流程。自定义实例选择单一协议，凭证独立保存，发现结果经选择后才保存；发现不可用时允许手填模型 ID。参考其交互流程，用 OpenRust 原生配置和 Artisan 实现。 |
| [OpenCode Provider 配置](https://opencode.ai/docs/providers) · [LiteLLM 模型管理](https://docs.litellm.ai/docs/proxy/model_management) | OpenCode 将自定义地址和凭证归 Provider，模型 ID 和能力元数据归模型。LiteLLM 分开对外模型名与真实上游模型。分别用于佐证 OpenRust 的配置组织和网关别名，保持现有 Laravel 键名和 Rust 依赖栈。 |

现有代码优先复用包发现、应用容器及生命周期、HTTP 客户端、事件、Cache、Storage、Queue、Broadcast／Reverb、数据库与 Artisan。旧版 `openrust-v0.x`（`6d1a81924`）的 `.agents/project/plans/ai-capabilities.md` 保留了 AI SDK 后续交付意图与自建模型服务边界。当前计划承接其通用 SDK 范围。可复用回归场景见旧版 `vendor/seaql/sea-orm/tests/embedding_tests.rs`。自建服务通过 HTTP 接入，旧计划中的千问（Qwen）可在各自接口验收后作为兼容配置案例；DeepSeek 和 Anthropic 继续按官方驱动验收。数据库查询侧 `EmbeddingProvider`、`HttpEmbeddingProvider`、缓存查询及向量结果校验见[当前实现](../../../vendor/openrust/framework/src/database/query/embeddings.rs)。通过适配器接入 SDK，保留旧查询入口、缓存和 SQL 行为。复核应用现有的向量实现后再定适配位置。

### 协议类型试编

已在 Rust 1.94.0 通过隔离 Cargo 消费者验证：固定 `async-openai` 关闭默认特性，只启用 `chat-completion-types`、`response-types`、`embedding-types`，三组 JSON 往返用例全部通过，`cargo tree --edges normal` 确认未引入 reqwest 或其他传输客户端。样本分别覆盖 Chat Completions 的 system/user 消息、Responses 的 instructions/input/store，以及 Embeddings 的批量输入与 dimensions。此证据覆盖依赖编译和样本字段往返；SDK 请求构建、HTTP 行为、响应提取、工具、流和供应商实机验收仍归对应批次。

## 包和契约

- 框架通过可选 Cargo feature `ai` 装配 `vendor/openrust/ai`。包发现注册 `AiServiceProvider`，由当前应用容器持有 `AiManager`。`Ai` 门面转发 Manager，`openrust_ai::agent(...)` 对应 Laravel 的独立 `agent()` helper。应用可实现 `Agent` trait 并通过 Rust async／Builder 表达提示词、覆盖选项与能力组合。
- `AiManager` 分开解析 Provider 实例名及 `driver`，保留命名自定义实例、按能力默认、显式覆盖、按需实例、驱动扩展和作用域清理。每类能力是独立 Provider／Gateway 契约。协议适配保留在包内，第三方类型可在内部使用。
- 发布 `config/ai.rs`。配置键采用 `ai.default`（openai）、`default_for_images`（gemini）、`default_for_audio`（openai）、`default_for_transcription`（openai）、`default_for_embeddings`（openai）、`default_for_reranking`（cohere）、`default_for_classification`（typesafe）、`caching.embeddings.{cache,store,individually}`（false、`CACHE_STORE` 默认 database、true）及 `ai.providers.<name>.driver`。以固定 `config/ai.php` 核对供应商变量、凭证优先级和默认模型。本地配置缓存和调试输出避免泄露原始密钥，应用及 Worker 从自己的运行时配置解析凭证。
- `openai` 和 `openai-compatible` 分开验收。固定版的 `OpenAiGateway` 发 `responses`，`OpenAiCompatibleGateway` 发 `chat/completions`。兼容驱动以 `models.text.default` 或显式模型为准，其余模型默认及覆盖同样按各自 Provider 源码核对。
- Artisan 验收 `make:agent`（含 `--structured`）、`make:agent-middleware`、`make:tool`、`vendor:publish` 的 `ai`、`ai-config`、`ai-stubs` 及迁移发布／执行，逐命令查明参数、stub 优先级、覆盖、退出码和副作用。固定 SDK 将 `ChatCommand` 的注册注释掉，`agent:chat` 保留在来源清单的未注册项。

固定源码复核补充了以下验收点，接口实现和行为验证随所属批次推进。

| 固定 Laravel 契约 | OpenRust 计划与验收 |
|---|---|
| `agent(instructions, messages, tools, schema)` 按 schema 是否存在构造普通或结构化匿名 Agent | 保留独立 `agent()`，通过 Rust 的参数类型或 Builder 表达可选参数。AI-01 验证普通 Agent，AI-02 验证 schema 分支与历史消息，AI-03 补工具参数的执行与失败用例。 |
| `AiManager` 继承 `MultipleInstanceManager`，`Ai` 门面转发至容器中的同一个 Manager | AI-01 对照继承的公开实例解析、扩展、缓存清理接口与实际调用，后续按能力核对对应的 Provider 和 Fake 解析入口。门面和容器解析分别编译调用，验证共享状态。 |
| `AiServiceProvider` 使用 `ai.conversations.connection` 构造 `DatabaseConversationStore`，在 `Looping` 和 Octane `OperationTerminated` 事件清理按需 Provider 状态 | AI-03 核对默认及显式会话数据库连接。AI-04 验证 OpenRust 常驻 Worker 和 HTTP 运行环境对应生命周期的清理时点，连续运行两个作业或请求验证隔离。具体 Hook 及等价依据记录在验收账本。 |
| `Embeddings::for` | `for` 是 Rust 关键字，AI-05 用 `Embeddings::r#for` 保留方法名，编译验证公开入口及数据库查询桥接。 |

## 交付与验收账本

各批开始前，按上游类型建立成员级清单，跟踪到实际声明、下游应用调用和测试。继承方法、trait/macro、门面转发、Pending 到 Response、事件、Fake 和反例随所属能力一同关闭。以下是分批的入口索引，成员逐项复核和实现均待办。

| 批次 | 按类型跟踪的入口与行为 | 首要实测 |
|---|---|---|
| AI-01 包发现＋文本 | `AiServiceProvider`、`Ai`、`AiManager` 的命名实例、`build`、`text_provider`、`text_provider_for`、默认及清理，`Agent`、`AnonymousAgent`、`agent()`、`prompt`、文本请求／响应及 Agent Fake。先完成 OpenAI Compatible 与 OpenAI 的非流式纯文本，并明确默认/显式 Provider 及模型优先级 | 启用、未启用及配置缓存后的应用启动。真实 HTTP 路由调用、两个本地协议服务验证 URL／鉴权／JSON／用量。覆盖不支持能力、缺模型、错误响应、Fake 断言及应用隔离。用 Artisan 检查包发现与发布；以 Rust 1.94 试编协议类型并验证不引入重复网络路径 |
| AI-02 多轮文本基础 | `AgentPrompt`、Messages、附件、`HasStructuredOutput`、`StructuredAgentResponse`、`HasProviderOptions`、属性／模型选项、默认／廉价／高级模型、`SummarizeAgent` 与 `Str`／`Stringable` 摘要入口、`StreamableAgentResponse`、流事件、SSE／Vercel、Usage、代理及每步生成事件；各文本供应商的限流、过载、故障转移及异常 | 相同调用分别实测普通、结构化和流。覆盖 Schema 深层对象／数组、拒答、畸形响应、UTF-8 与行跨数据块、取消与用量、已经向客户端输出时的失败处理。供应商失败分类用协议样本逐个确认 |
| AI-03 工具与会话 | `HasTools`、`Tool`／`ToolCall`／`ToolResult`、校验、修复、检索／延迟加载、文件工具、Provider 工具、MCP 工具桥、Skills、`CanActAsTool`、子 Agent、`HasMiddleware`、`StepTextGateway`、停止条件；`Conversational`、`RemembersConversations`、`ConversationStore`、参与者与归属检查；审批请求、审批结果、恢复相关类型与事件 | 实测多个步骤的 Middleware 次数及顺序、工具调用关联、会话迁移、历史裁剪／分页、跨账户隔离、拒绝／批准／过期、重放及并发恢复。Provider 工具用独立合同约束其支持范围 |
| AI-04 队列、广播和生成器 | `Agent::queue`／`broadcast`／`broadcast_now`／`broadcast_on_queue`、Jobs、Pending、广播事件、失败和大事件，Fake 排队断言；前述 Artisan 生成器、配置／stub／迁移发布 | 独立 Artisan／Worker 进程实际消费，并通过 HTTP／订阅者观察流或广播事件；核对重试、作业间 Provider 清理、私有频道授权及序列化边界。编译模板生成的消费代码 |
| AI-05 向量、分类和排序 | `Embeddings::for`、`Reranking::of`、`Classification::of`、各 Provider／Gateway、Pending／Response、Stringable／Collection 对应便利入口、缓存与数据库桥，附件分类、二元分类与 Fake | 批量及单条、并发、维度／非法浮点、缓存开关／Store／粒度、真实向量查询；排序命中原始索引；分类分别核对问题、标签、附件支持与错误 |
| AI-06 媒体与文件 | `Image::of`、`Audio::of`、`Transcription::of` 及来源入口、`Files` 的上传／读取／删除、`Stores` 的创建／读取／删除、对应 Gateway／Responses、文件与向量仓库关系、非公开文件与附件、下载存储及 Fakes | 用生成的图片和音频等本地样本测试 multipart、媒体返回、存盘、转录、仓库与文件 CRUD、分页、元数据、归属及删除回读。媒体输出通过实际 HTTP 和 Storage 回读 |
| AI-07 收尾 | 逐成员审查所有未关闭分支、供应商子能力及对应模型，核对三语文档、生成产物、支持矩阵和 E07 主账本 | 开启／关闭 feature、Release HTTP／Artisan 及相应 Worker／浏览器实测；按每家每项能力留证据并区分协议模拟与供应商实机验收 |

## 供应商矩阵

驱动名取自固定源码。以下为目标，OpenRust SDK 的对应能力均待实现。工具支持、默认模型及协议差异随对应 Provider 单独验收。

| 能力 | 目标驱动 |
|---|---|
| 文本 | `openai`、`openai-compatible`、`anthropic`、`gemini`、`azure`、`bedrock`、`groq`、`xai`、`deepseek`、`mistral`、`ollama`、`openrouter`、`cohere` |
| 图像 | `openai`、`gemini`、`xai`、`azure`、`bedrock`、`openrouter` |
| 语音合成 | `openai`、`eleven`、`gemini`、`mistral`、`openrouter` |
| 语音转录 | `openai`、`openai-compatible`、`eleven`、`groq`、`mistral`、`gemini`、`openrouter` |
| Embeddings | `openai`、`openai-compatible`、`gemini`、`azure`、`bedrock`、`cohere`、`mistral`、`jina`、`voyageai`、`ollama`、`openrouter` |
| Reranking | `cohere`、`jina`、`voyageai`、`bedrock`、`openrouter` |
| Classification | `openai`、`openai-compatible`、`typesafe`、`openrouter`、`gemini`（固定源码 `GeminiProvider` 实现该契约，固定文档总表未列） |
| 供应商文件 | `openai`、`anthropic`、`gemini`、`azure`、`openrouter` |
| 供应商向量仓库 | `openai`、`gemini`、`azure`（取自源码 `StoreProvider`） |

本地实测将使用合成业务输入和真实 HTTP 回环协议服务，Fake 断言与 HTTP 协议测试分别记证据。遇到脱敏、缓存键归属、重试中的副作用或应用间状态隔离差异时，加入对应反例验证。涉及供应商账号、权限、地区、配额或模型可用性的实机结果逐项记录，缺条件的项目保持待验收。运行时用法和当前支持范围同步到现有三语 AI SDK 文档。

### 按协议复用

用户确认优先维护共用的 Chat Completions、Responses、Anthropic Messages 和 Gemini 原生协议实现。`ai.providers.<name>` 的名称标识命名连接，`driver` 选择对应驱动。兼容服务以现有 `driver`、基础 URL、凭证引用和模型 ID 接入，沿用同一个应用调用入口。新增供应商优先提交配置示例和协议验收用例。

Laravel 基线的内置驱动名称、默认地址、模型和实际行为随对应批次验收。可共用的请求、响应、SSE 及工具映射在协议层复用。固定源码中的 `deepseek` 驱动与 `openai-compatible` 已共用 Chat Completions 请求逻辑，可作为复用参考。供应商特有的鉴权、字段及事件差异在相应驱动或适配器内补齐，例如 DeepSeek 的思考模式和 Bedrock Converse 的区域、模型路径及 AWS 凭证。参考 [Laravel DeepSeek Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/DeepSeek/DeepSeekGateway.php)及 [AWS Bedrock Converse](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html)。

以供应商、模型、协议和能力为单位记录验收结果。对话可用后，分别验证流、工具调用、结构化输出、Embeddings、图片和音频的实际端点及模型限制。同一家同时开放多个协议时，每种协议用独立命名连接和测试用例。

### 国内供应商接入

用户确认在保留以上 17 个固定 Laravel 驱动的基础上，将以下四家纳入可配置的接入示例和兼容验收。应用可同时配置多家供应商，也可为同一家配置多个命名实例，例如两套百炼工作空间。`ai.default` 用于未指定 Provider 的文本调用，其他任务仍可显式选择 Provider 和模型。

| 命名实例 | 服务 | 首批接入与实测范围 |
|---|---|---|
| `qwen` | 阿里云百炼／通义千问 | 通过 `openai-compatible` 接入 Chat Completions。基础 URL 与 Key 对应同一地域和工作空间，保留 `/compatible-mode/v1` 路径。 [百炼基础 URL 文档](https://docs.modelstudio.console.alibabacloud.com/en/model-studio/base-url) |
| `tencent` | 腾讯云 TokenHub／混元等 | 先验收 Chat Completions，再按实际模型分别验证 Responses 和 Anthropic Messages。多协议接入时使用各自的命名实例和驱动。 [TokenHub 协议矩阵](https://cloud.tencent.com/document/product/1823/130079) |
| `qianfan` | 百度智能云千帆 | 通过 `openai-compatible` 接入 Chat Completions，基础 URL 为 `https://qianfan.baidubce.com/v2`，核对实际模型 ID 或 API 名称。 [千帆文本生成文档](https://cloud.baidu.com/doc/qianfan-api/s/wmwbl48n4) |
| `ark` | 火山引擎方舟 | 通过 `openai-compatible` 接入 Chat Completions，模型 ID 取实际开通的模型或推理接入点。 [方舟兼容 OpenAI SDK 文档](https://docs.volcengine.com/docs/ark/compatible-with-openai-sdk?lang=zh) |

配置示例复用 `ai.providers.<name>.{driver,url,models.text.default}` 和运行时凭证解析。配置预览中的 `key_env` 仍是凭证引用字段的建议名称。确认真实协议差异后才补专用适配，兼容端点继续复用通用驱动。同一个对外别名（例如 `support-chat`）可改绑到这些实例，验证客户端沿用相同网关地址、接口协议、Key 和模型名即可调用。

随 AI-01 验收基础 URL 拼接、鉴权、模型选择、显式覆盖及 Chat Completions 请求与响应。流、结构化输出、工具调用、Embeddings 和额外协议随对应 SDK 与网关批次按供应商、模型和协议逐项核对。三语文档提供最小配置与调用示例，实测记录分别标明协议模拟、服务商实机和待验收范围。

以下供应商登记为后续兼容候选，目前有官方协议文档，OpenRust 实机验收待办。先按各自公开端点复用驱动，再按模型核对其余能力。

| 命名实例建议 | 服务 | 候选协议 |
|---|---|---|
| `kimi` | 月之暗面 Kimi | Chat Completions、Responses、Anthropic Messages 分开配置和验收。 [Kimi API 快速开始](https://platform.kimi.ai/docs/get-your-api-key) |
| `zhipu` | 智谱 GLM | 优先以 `openai-compatible` 验收 Chat Completions。 [智谱 OpenAI API 兼容文档](https://docs.bigmodel.cn/cn/guide/develop/openai/introduction) |
| `minimax` | MiniMax | 优先以 `openai-compatible` 验收 Chat Completions。 [MiniMax OpenAI SDK 文档](https://platform.minimax.io/docs/api-reference/text-openai-api) |

## Sub2API 兼容接入与模型网关

| 使用方式 | 装配与行为 |
|---|---|
| 框架内调用 | 应用通过 `openrust-ai` 的 `Ai`、`Agent`、各能力入口和测试 Fake 调用模型。进程内直接使用容器中的 `AiManager`，按配置选直连供应商、自建服务或外部网关。 |
| 接入上游 | `openrust-ai` 的命名 Provider 配置基础 URL、凭证、模型及能力选项。接入 Sub2API 时分别验证 `openai` 的 Responses 和 `openai-compatible` 的 Chat Completions，也按基线验证 Anthropic Messages、Gemini 原生入口。先复用通用协议驱动，实际存在网关专属语义时再确定适配方式。 |
| 对外服务 | 采用独立可选的 `ai-model-gateway` 工作名称，允许单独部署。外部客户端按已有协议和鉴权头访问 OpenRust 网关。网关复用 SDK 的供应商适配与 HTTP 传输，通过自己的路由、凭证与限额策略调用上游。SDK 的 `Gateway` trait 仍表示应用侧协议适配。 |

固定版 [Sub2API 路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/server/routes/gateway.go)、[Responses 桥接](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler_responses.go)和[组合路由设计](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/docs/COMPOSITE_GROUPS.md)作为首批参考。优先走同协议上游。跨协议时明确映射请求、响应、工具 ID、流事件、停止原因、错误和用量；对于无法表达的能力返回相应协议的错误。外部客户端的工具调用按协议返回供客户端处理，应用自有 Agent 工具仍由 SDK 按权限与审批配置执行。

### 网关分批交付

| 批次 | 交付与验收 |
|---|---|
| GW-00 协议与能力清单 | 按固定 Sub2API 源码建立端点、上游协议、账号类型、能力选项及管理行为清单。记录客户端可观察契约、版本、实现批次和证据。以网关协议能力确定兼容范围，完整商业平台界面与业务仍按独立项目范围确认。 |
| GW-01 接入现有网关 | 随 AI-01／02／05／06 使用真实 Sub2API 进程和本地协议服务，分批验收普通请求、流、工具、结构化输出、Embeddings 及媒体。记录网关版本、端点、分组、上游及模型；供应商实机结果单独记录。 |
| GW-02 OpenAI 兼容入口 | 对外提供 `/v1/models`、`/v1/responses`、`/v1/chat/completions`，验证现有客户端只改基础 URL、凭证及可用模型即可调用。核对 HTTP 状态、协议错误、鉴权头、JSON、SSE、取消及用量。协议路由使用对应响应格式。 |
| GW-03 多协议与路由 | 补 Anthropic Messages、Gemini 原生入口及双向协议适配，首个跨协议验收为 Responses 请求经 Anthropic 上游返回 Responses 事件。覆盖模型别名、按端点和模型选路、能力检测、同协议透传、回退限制。Embeddings、图片、音频、WebSocket 等按 GW-00 实际能力矩阵推进。 |
| GW-04 账号与运维 | 交付下游 API Key、分组与权限，上游 API Key、适用的 OAuth／订阅账号接入、轮询和冷却、并发与用量限额、审计及计量。按账号类型验证授权生命周期、失效、路由回退与使用记录；跨租户及部分失败纳入反例。 |
| GW-05 联合验收 | 同一合成任务分别从应用 SDK、经 Sub2API 的上游调用、现有外部客户端访问 OpenRust 网关完成。覆盖直接连接、协议转换和实际路由，验证接入文档、三语支持矩阵和 E07 账本。 |

同一请求输出了流事件或执行了工具之后，测试其失败路径及重复执行风险。兼容结果以实际调用和可观察行为为准，模型目录可见、HTTP 路由存在和单一模型成功各自保留验证边界。E07 在 SDK 与网关批次均完成、三条使用路径均通过相应验收后关闭。

## DSH 式模型接入

1. **内置供应商。** 应用在 `ai.default` 选 `deepseek` 等已有 Provider，通过环境或已有凭证配置供给密钥。默认模型沿用固定 Laravel Provider；需要指定模型时设置 `ai.providers.<name>.models.text.default` 或在调用时显式覆盖。先用 Artisan 打通选择供应商、模型和测试调用的入口，具体命令名及参数列为 OpenRust 扩展并在实现前核对。
2. **自定义服务。** 在 `ai.providers.<name>` 以 `driver` 选择协议，填写 `url`、凭证和模型 ID，沿用 SDK 已计划的配置键。`openai-compatible` 走 Chat Completions，`openai` 走 Responses，Anthropic 与 Gemini 使用各自驱动。一个网关提供多种协议时可按协议配置不同实例。验收带 `/v1`、`/v1beta` 或自定义路径前缀的基础 URL，并核对协议鉴权和模型 ID 的大小写。
3. **发现、选用和验证。** 目录发现按 Provider 实际公布的能力调用，沿用已有 HTTP 和凭证解析，复用同一条模型配置与调用路径。遇到目录缺失、为空或请求失败，保留已配置模型和手填入口；确认后才写入选择结果。目录结果与实际调用测试分别展示。工具、流、图片等能力以协议和模型实测为准。
4. **对外接入。** OpenRust 网关以对外模型名映射实际 Provider 和模型 ID，客户端只配置地址、匹配的协议、网关凭证和对外模型名。用固定版 DSH 的自定义模型 API 完成拉取列表（支持时）、手填、选择和真实调用；覆盖 Chat Completions、Responses 及后续 Anthropic Messages、Gemini 原生入口，按批补工具和 SSE。沿用 Sub2API 的模型路由验收，并继续使用本地模拟上游和授权实机分别留证据。

DeepSeek 官方端点作为首个完整示例。固定 Laravel `deepseek` 驱动保持 Chat Completions 契约，测试文本、流、思考模式和工具回合的 `reasoning_content` 行为。DeepSeek 官方另提供 Responses 和 Anthropic Messages，作为对应协议的独立配置案例，分别验收请求字段和能力；稳定模型 ID 以验收时官方目录为准。参考 [Laravel DeepSeek Provider 固定源码](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Providers/DeepSeekProvider.php)、[Prism DeepSeek 文档](https://prismphp.com/providers/deepseek/)、[DeepSeek 首次调用](https://api-docs.deepseek.com/)、[Responses](https://api-docs.deepseek.com/guides/responses_api/)、[Anthropic](https://api-docs.deepseek.com/guides/anthropic_api/)及[思考模式](https://api-docs.deepseek.com/guides/thinking_mode/)。

实施顺序沿用 AI-01／02／03、GW-01／02／03／05。先验收配置即可调用，再接模型发现、选择与独立试调用，最后用 DSH 连接 OpenRust 网关复现整条流程。E07 关闭时须同时具备三语配置示例、真实客户端验收结果和供应商能力矩阵。
