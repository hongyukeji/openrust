# Laravel AI SDK 对齐计划

E07 的交付目标是在 OpenRust 提供独立可选的 `openrust-ai`，保留 Laravel AI 的公开入口、配置、Provider 选择、执行语义与验收范围。当前完成基线固定、依赖调研、协议类型试编和功能分批，SDK 实现及运行验收待开始。现有数据库查询的 Embeddings 路径只作为复用候选。应用语言词条、机器翻译、AI Agent 翻译后续计划、内容审核及 Boost／MCP 服务各自保留职责。AI SDK 自身的 MCP 工具桥接纳入本计划。

## 固定基线与选型

| 对照 | 核对结果与采用范围 |
|---|---|
| [Laravel AI SDK `1d269f2359aa166aaf10b212151bf556dd86a02e`](https://github.com/laravel/ai/tree/1d269f2359aa166aaf10b212151bf556dd86a02e) · [Laravel 文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8`](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/ai-sdk.md) | MIT。以实际 Provider 注册、Contracts、Gateway、Pending、Response、Events、迁移、命令和调用路径为准。文档用于核对开发者入口，源码用于裁决版本内差异。 |
| [Goravel v1.18.0 固定源码](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11/ai) | 参考静态注册、请求上下文、媒体存储、流取消及故障转移测试。其 `AI.Agent` 装配、选项及可配置错误规则按 Laravel 基线裁决。 |
| [Prism `385022b5e0fec9be49a5b8e9a468fdafaa8f1267`](https://github.com/prism-php/prism/tree/385022b5e0fec9be49a5b8e9a468fdafaa8f1267) | MIT。参考分能力 Pending／Response、供应商独立协议及 Fake 测试。运行路径保持纯 Rust。见 [Prism OpenAI 适配文档](https://prismphp.com/providers/openai/)。 |
| [rust-genai `2a8a5b39c96028d2143d8754ee2bba42a79e32c5`](https://github.com/jeremychone/rust-genai/tree/2a8a5b39c96028d2143d8754ee2bba42a79e32c5) | MIT／Apache-2.0。多供应商模型与流适配候选，支持注入 reqwest 客户端。注入后仍须验证指定版本能通过 Rust 1.94、保留出站策略及日志边界，并完整映射协议事件。 |
| [async-openai `18a85cba921ccef08b2d058e5080f7083071e39c`](https://github.com/64bit/async-openai/tree/18a85cba921ccef08b2d058e5080f7083071e39c) | MIT。按能力启用 Chat Completions、Responses、Embeddings 等协议类型，声明的 workspace MSRV 为 1.75，实际传递依赖仍需编译核对。首选试用类型特性，经 OpenRust HTTP 发送和解析，保持加密、代理、超时、观测及测试入口一致。若类型与锁定协议不吻合，适配该差异并用 HTTP 用例固定。 |
| [Rig `f4d73dacdfabb3d9daa3f1de091efa490a84c601`](https://github.com/0xPlaygrounds/rig/tree/f4d73dacdfabb3d9daa3f1de091efa490a84c601) | MIT。参考按模型和工具分隔运行步骤、流式测试及检查点。该版本最低 Rust 1.97.1，当前项目为 1.94，本轮作设计参考。 |

现有代码优先复用包发现、应用容器及生命周期、HTTP 客户端、事件、Cache、Storage、Queue、Broadcast／Reverb、数据库与 Artisan。旧版 `openrust-v0.x`（`6d1a81924`）的 `.agents/project/plans/ai-capabilities.md` 保留了 AI SDK 后续交付意图与自建模型服务边界。当前计划承接其通用 SDK 范围。可复用回归场景见旧版 `vendor/seaql/sea-orm/tests/embedding_tests.rs`。自建服务通过 HTTP 接入，旧计划中的千问（Qwen）可在各自接口验收后作为兼容配置案例；DeepSeek 和 Anthropic 继续按官方驱动验收。数据库查询侧 `EmbeddingProvider`、`HttpEmbeddingProvider`、缓存查询及向量结果校验见[当前实现](../../../vendor/openrust/framework/src/database/query/embeddings.rs)。通过适配器接入 SDK，保留旧查询入口、缓存和 SQL 行为。复核应用现有的向量实现后再定适配位置。

### 协议类型试编

已在 Rust 1.94.0 通过隔离 Cargo 消费者验证：固定 `async-openai` 关闭默认特性，只启用 `chat-completion-types`、`response-types`、`embedding-types`，三组 JSON 往返用例全部通过，`cargo tree --edges normal` 确认未引入 reqwest 或其他传输客户端。样本分别覆盖 Chat Completions 的 system/user 消息、Responses 的 instructions/input/store，以及 Embeddings 的批量输入与 dimensions。此证据覆盖依赖编译和样本字段往返；SDK 请求构建、HTTP 行为、响应提取、工具、流和供应商实机验收仍归对应批次。

## 包和契约

- 框架通过可选 Cargo feature `ai` 装配 `vendor/openrust/ai`。包发现注册 `AiServiceProvider`，由当前应用容器持有 `AiManager`。`Ai` 门面转发 Manager，`openrust_ai::agent(...)` 对应 Laravel 的独立 `agent()` helper。应用可实现 `Agent` trait 并通过 Rust async／Builder 表达提示词、覆盖选项与能力组合。
- `AiManager` 分开解析 Provider 实例名及 `driver`，保留命名自定义实例、按能力默认、显式覆盖、按需实例、驱动扩展和作用域清理。每类能力是独立 Provider／Gateway 契约。协议适配保留在包内，第三方类型可在内部使用。
- 发布 `config/ai.rs`。配置键采用 `ai.default`（openai）、`default_for_images`（gemini）、`default_for_audio`（openai）、`default_for_transcription`（openai）、`default_for_embeddings`（openai）、`default_for_reranking`（cohere）、`default_for_classification`（typesafe）、`caching.embeddings.{cache,store,individually}`（false、`CACHE_STORE` 默认 database、true）及 `ai.providers.<name>.driver`。以固定 `config/ai.php` 核对供应商变量、凭证优先级和默认模型。本地配置缓存和调试输出避免泄露原始密钥，应用及 Worker 从自己的运行时配置解析凭证。
- `openai` 和 `openai-compatible` 分开验收。固定版的 `OpenAiGateway` 发 `responses`，`OpenAiCompatibleGateway` 发 `chat/completions`。兼容驱动以 `models.text.default` 或显式模型为准，其余模型默认及覆盖同样按各自 Provider 源码核对。
- Artisan 验收 `make:agent`（含 `--structured`）、`make:agent-middleware`、`make:tool`、`vendor:publish` 的 `ai`、`ai-config`、`ai-stubs` 及迁移发布／执行，逐命令查明参数、stub 优先级、覆盖、退出码和副作用。固定 SDK 将 `ChatCommand` 的注册注释掉，`agent:chat` 保留在来源清单的未注册项。

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
