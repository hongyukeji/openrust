# Laravel AI SDK 与模型网关开发计划

用户确认 E07 的完整目标为 **Laravel AI + Sub2API + DeepSeek Harness（DSH）的模型接入体验**。Laravel AI 定义框架内的 SDK 契约，Sub2API 是自托管网关的主要对齐基线，DSH 提供添加连接和选用模型的体验参考。已交付两种 OpenAI 文本协议、模型目录与试调用，以及自托管网关的 OpenAI 入口。已落实的成员、实测及缺口见 [契约账本](../audits/ai/README.md)，其余 SDK 批次、多协议网关和联合验收仍在推进。

## 开发需求

- 将 OpenRust 的 AI 模块对齐固定 Laravel AI SDK。公开入口、目录职责、配置键、Provider 生命周期、Artisan 命令、测试方式及可观察行为逐项对应 Laravel。按用户确认预填常用供应商连接及可覆盖的基础 URL，默认连接和模型由应用显式选择。Rust 接口使用本项目的命名规范和异步类型，必要适配随契约核对记录依据。
- 按固定版本交付完整适用能力。覆盖 Agent、结构化输出、流、工具及审批、会话、队列、广播、图像、音频、转录、Embeddings、Reranking、Classification、文件与向量仓库，以及用量、故障转移、事件和 Fake。供应商按下方矩阵逐能力验收。
- 开发前参考成熟的 GitHub 项目及 Laravel 生态实现。优先复用经过验证的方案和 OpenRust 已有机制，按实际协议差异决定是否增加依赖或适配层。
- 同时交付三种使用方式。OpenRust 应用能在进程内使用 SDK，能通过兼容或原生协议接入供应商、自建模型服务和 Sub2API，也能启用模型网关，为外部客户端提供兼容 API。网关覆盖协议转换、模型路由与上游账号接入。
- 采用 DSH 式简洁接入流程。发布的 `ai.providers` 预填常用供应商的协议、官方基础 URL 和凭证引用，全部 URL 支持环境变量覆盖。缺少凭证的预设保持未配置状态。配置 Key 后可拉取模型供选择，手动填写始终可用。将 DSH 本身作为 OpenRust 对外网关的实际客户端验收。
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
| [腾讯云 TokenHub 语言模型调用概览](https://cloud.tencent.com/document/product/1823/130079) | 作为多协议网关的公开行为参考。同一地域基础 URL 下提供 Chat Completions、Responses 和 Anthropic Messages，按协议使用对应鉴权头。同一模型按协议区分原生、转换兼容和不支持。借鉴统一地址、动态模型目录和逐模型协议能力矩阵。 |
| [DeepSeek Harness `4b399c39a6d1479cd84ca591b4d7c88e24d38a96`](https://github.com/deepseek-ai/deepseek-harness/blob/4b399c39a6d1479cd84ca591b4d7c88e24d38a96/docs/user/guide/providers.md) | 采用其自定义模型 API 流程。命名连接选择单一协议，凭证独立保存，模型发现结果经明确选择后才保存，发现不可用时允许手填模型 ID。用 OpenRust 原生配置和 Artisan 实现。 |
| [OpenCode Provider 配置](https://opencode.ai/docs/providers) · [LiteLLM 路由名称](https://docs.litellm.ai/docs/router/learn/names) · [LiteLLM 模型权限](https://docs.litellm.ai/docs/routing) | OpenCode 将地址和凭证归 Provider，模型 ID 和能力归模型。LiteLLM 分开公开名、部署目标和访问权限。采用命名连接与可列出、可调用范围一致的发布规则。配置沿用 Laravel 键名。 |
| [TensorZero 配置指南](https://www.tensorzero.com/docs/gateway/configure-models-and-providers) · [Rust 源码 `62eb8f63e8ec62018d70420dbf1a8c5d1c026315`](https://github.com/tensorzero/tensorzero/tree/62eb8f63e8ec62018d70420dbf1a8c5d1c026315) | 参考模型名映射具体 Provider 和有序路由。GW-02 先用精确模型名映射一个目标，GW-03 再按兼容需求补规则和回退。继续使用 OpenRust 的传输、配置和路由装配。 |

现有代码优先复用包发现、应用容器及生命周期、HTTP 客户端、事件、Cache、Storage、Queue、Broadcast／Reverb、数据库与 Artisan。旧版 `openrust-v0.x`（`6d1a81924`）的 `.agents/project/plans/ai-capabilities.md` 保留了 AI SDK 后续交付意图与自建模型服务边界。当前计划承接其通用 SDK 范围。可复用回归场景见旧版 `vendor/seaql/sea-orm/tests/embedding_tests.rs`。自建服务通过 HTTP 接入，旧计划中的千问（Qwen）可在各自接口验收后作为兼容配置案例；DeepSeek 和 Anthropic 继续按官方驱动验收。数据库查询侧 `EmbeddingProvider`、`HttpEmbeddingProvider`、缓存查询及向量结果校验见[当前实现](../../../vendor/openrust-framework/src/database/query/embeddings.rs)。通过适配器接入 SDK，保留旧查询入口、缓存和 SQL 行为。复核应用现有的向量实现后再定适配位置。

### 协议类型试编

已在 Rust 1.94.0 通过隔离 Cargo 消费者验证：固定 `async-openai` 关闭默认特性，只启用 `chat-completion-types`、`response-types`、`embedding-types`，三组 JSON 往返用例全部通过，`cargo tree --edges normal` 确认未引入 reqwest 或其他传输客户端。样本分别覆盖 Chat Completions 的 system/user 消息、Responses 的 instructions/input/store，以及 Embeddings 的批量输入与 dimensions。此证据覆盖依赖编译和样本字段往返；SDK 请求构建、HTTP 行为、响应提取、工具、流和供应商实机验收仍归对应批次。

## 包和契约

- 框架通过可选 Cargo feature `ai` 装配 `vendor/openrust-ai`。包发现注册 `AiServiceProvider`，由当前应用容器持有 `AiManager`。`Ai` 门面转发 Manager，`openrust_ai::agent(...)` 对应 Laravel 的独立 `agent()` helper。应用可实现 `Agent` trait 并通过 Rust async／Builder 表达提示词、覆盖选项与能力组合。
- `AiManager` 分开解析 Provider 实例名及 `driver`，保留命名自定义实例、按能力默认、显式覆盖、按需实例、驱动扩展和作用域清理。每类能力是独立 Provider／Gateway 契约。协议适配保留在包内，第三方类型可在内部使用。
- 发布 `config/ai.rs`。保留 Laravel 的 `ai.default`、各 `default_for_*`、`caching.embeddings.{cache,store,individually}`（false、`CACHE_STORE` 默认 database、true）及 `ai.providers.<name>.driver`。应用显式配置默认连接，调用时也可显式选择连接和模型。无可用凭证或默认连接时仍可启动，调用中未能解析连接、凭证或模型时报告配置错误。预设只包含公开连接信息与凭证引用，模型从上游发现或手填。本地配置缓存和调试输出避免泄露原始密钥，应用及 Worker 从运行时解析凭证。
- `openai` 和 `openai-compatible` 分开验收。固定版的 `OpenAiGateway` 发 `responses`，`OpenAiCompatibleGateway` 发 `chat/completions`。兼容驱动以 `models.text.default` 或显式模型为准，其余模型默认及覆盖同样按各自 Provider 源码核对。
- Artisan 验收 `make:agent`（含 `--structured`）、`make:agent-middleware`、`make:tool`、`vendor:publish` 的 `ai`、`ai-config`、`ai-stubs` 及迁移发布／执行，逐命令查明参数、stub 优先级、覆盖、退出码和副作用。固定 SDK 将 `ChatCommand` 的注册注释掉，`agent:chat` 保留在来源清单的未注册项。

### 常用流程与配置验收

以 Laravel 的命名实例、默认实例和显式覆盖为准。应用侧只配置连接与模型，协议、请求构造、鉴权和响应解析由驱动完成。高级能力在相应入口或网关批次按需配置。

| 常用流程 | 开发者最少操作 | 验收 |
|---|---|---|
| 首次接入预设供应商 | 在 `.env` 填一个 Key，选择该连接和一个模型。Artisan 可从目录选择并试调用，也可用配置直接填写模型 ID。把同一次确认的连接和模型选作默认时，在同一操作完成。 | 未填写 URL 使用预设；只有一家的 Key 时优先提示该可配置连接，仍由开发者确认默认值。多家时显式选择。完整配置后无需运行向导即可调用。 |
| 接入自建或兼容端点 | 新增一个 `ai.providers.<name>`，提供 `driver`、基础 `url` 和凭证引用，模型发现与手填沿用同一流程。若端点无需鉴权则使用协议和连接允许的无凭证配置。 | 新兼容服务通过同一驱动完成调用，保留 URL 的路径前缀。认证失败、目录为空和模型调用失败分别报告。 |
| 日常应用调用与测试 | 通过 `Ai`、`Agent::prompt` 等入口直接调用已配置的默认连接与模型。有需要时只覆盖当前调用的 Provider 或模型。测试通过对应能力的 Fake 设置输出和断言。 | 从公开 Rust 入口用单次调用得到结果。覆盖显式模型、命名连接默认模型、能力默认连接及未设置时的配置错误。仅使用无状态 SDK 时不需要建会话表、开网关或运行 Worker。 |
| 对外发布第一个模型 | 网关启用后，从已配置连接选模型，填一个公开名并签发一个下游 Key。网关映射引用该连接，模型 ID 可从当前选择带入并显式保存。 | 用一个上游凭证完成目录与调用。外部客户端输入基础 URL、协议、下游 Key 和公开模型名即可使用。日常发布无需配置账号池、回退、会话黏性或计费规则。 |

- 按固定 `Promptable` 核对文本解析顺序。单连接常用路径的 Provider 依次取调用参数、Agent 配置和 `ai.default`；模型依次取调用参数、Agent 配置和所选 Provider 的模型。高阶或低价模型、按需配置和回退列表的细节按源码验收。模型发现用于辅助选择，启动、普通推理和目录刷新均不会自动改默认模型。 [Laravel Promptable 固定源码](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Promptable.php)
- `config/ai.rs` 是连接配置的入口，网关发布配置只引用连接名及上游模型 ID。普通接入无需重复录入 URL、协议或上游凭证。Artisan 与手动配置共用解析、校验和试调用逻辑。向导确认后按现有配置管理方式保存选择，或者输出可复制配置；手写 Rust 配置不依赖向导重写。未启用对应功能时，启动无需访问供应商、初始化账号池或发布网关路由。
- 三语文档先给出“填 Key、选模型、调用”的最小示例，再展示自定义端点和网关发布。AI-01 验证配置和第一次直接调用，AI-04 核对生成与发布，GW-02 从新增连接到外部调用记录必填字段及手工操作，AI-07／GW-05 从发布模板重跑以上流程。

固定源码复核补充了以下验收点，接口实现和行为验证随所属批次推进。

| 固定 Laravel 契约 | OpenRust 计划与验收 |
|---|---|
| `agent(instructions, messages, tools, schema)` 按 schema 是否存在构造普通或结构化匿名 Agent | 保留独立 `agent()`，通过 Rust 的参数类型或 Builder 表达可选参数。AI-01 验证普通 Agent，AI-02 验证 schema 分支与历史消息，AI-03 补工具参数的执行与失败用例。 |
| `AiManager` 继承 `MultipleInstanceManager`，`Ai` 门面转发至容器中的同一个 Manager | AI-01 对照继承的公开实例解析、扩展、缓存清理接口与实际调用，后续按能力核对对应的 Provider 和 Fake 解析入口。门面和容器解析分别编译调用，验证共享状态。 |
| `AiServiceProvider` 使用 `ai.conversations.connection` 构造 `DatabaseConversationStore`，在 `Looping` 和 Octane `OperationTerminated` 事件清理按需 Provider 状态 | AI-03 核对默认及显式会话数据库连接。AI-04 验证 OpenRust 常驻 Worker 和 HTTP 运行环境对应生命周期的清理时点，连续运行两个作业或请求验证隔离。具体 Hook 及等价依据记录在验收账本。 |
| `Embeddings::for` | `for` 是 Rust 关键字，AI-05 用 `Embeddings::r#for` 保留方法名，编译验证公开入口及数据库查询桥接。 |

### 复审发现与实现约束

| 核对项 | 复审结论与处理 |
|---|---|
| 默认装配与扩展 | `AiServiceProvider`、`AiManager`、`Ai` 及实际消费者列入 AI-00。保留 `AiManager` 的实例解析、注册扩展、按需构建和清理。与 Laravel 的默认行为差异为应用显式选择默认连接与模型。AI-01 验收无凭证启动、全部预设 URL 的环境变量覆盖、显式/默认选择、未知驱动以及缓存配置。 |
| Provider 入口和序列化 | `Provider` 有 `name`、`driver`、`providerCredentials`、`additionalConfiguration`、`withHeaders` 及序列化行为。`AiManager` 有各能力和 Fake 入口。AI-00 按所属类型登记，AI-01/04 验证应用、请求、作业之间隔离。对于 Laravel 凭证可访问与 PHP 序列化行为，明确 Rust API、作业凭证引用和缺少凭证时的结果；作业在运行时解析凭证，协议内容及密钥按框架脱敏能力处理。 |
| Prompt、响应与测试 | `Promptable::make`、`withMessages`、`withTools`、`assertPromptedTimes`、`assertNeverPrompted` 等公开入口随所属能力核对。AI-02 覆盖 `StreamableAgentResponse` 的 `each`、`catch`、`then`、`usingProtocol`、`usingVercelDataProtocol`、`usingAgentUserInteractionProtocol`，并逐项核对 `AgentUserInteraction` 和 Vercel 的输入、状态与事件。Fake、模拟协议服务和供应商实机提供三种独立证据。 |
| 真实流式传输 | 当前框架 [HTTP 客户端响应转换](../../../vendor/openrust-framework/src/http/client/response.rs) 会将网络块读完才返回，现有 [HTTP 响应流](../../../vendor/openrust-framework/src/http/response.rs) 用于向下游发送。AI-02 先补最小通用出站流入口，SDK 和网关共同使用，并复用 HTTP 工厂、目标校验、TLS、代理、重定向策略和脱敏。以慢速上游验证首块在完成前到达、取消时释放连接、限额和下游背压。 |
| 会话、审批与远程资源 | AI-03 按 `PendingApproval`、`Decision`、`Decisions`、`ConversationStore`、审批恢复路径及会话迁移核对状态绑定和授权。AI-05 对比 SDK Embeddings 缓存配置和旧查询缓存策略。AI-06 补 `Store::add/remove/refresh/delete`、成员文件 ID 与上游文件 ID、文件删除选项及各 Fake 断言。文件、仓库和响应 ID 后续调用按发出它们的 Provider 及权限处理。 |
| Artisan 和对外响应 | Laravel 的 `make:agent`、`make:tool`、`make:agent-middleware` 和 `vendor:publish` 依照固定命令实现。模型连接查询、选择和试调用复用现有 Artisan 的 choice、confirm 和 secret 能力，其命令名称由 AI-00 确认。GW-02 对协议路由逐项验证正常结果和鉴权、限流、解析失败等异常，保留 HTTP 状态、响应头、协议错误体、SSE、multipart／二进制行为，避免业务 API envelope 改写协议。 |

## 交付与验收账本

各批开始前，按上游类型建立成员级清单，跟踪到实际声明、下游应用调用和测试。继承方法、trait/macro、门面转发、Pending 到 Response、事件、Fake 和反例随所属能力一同关闭。以下是分批的入口索引，成员逐项复核和实现均待办。

| 批次 | 按类型跟踪的入口与行为 | 首要实测 |
|---|---|---|
| AI-00 契约清单与依赖 | 从固定 SDK 建立模块、类型、公开成员、继承／trait／宏／Facade、配置、Artisan、迁移和实际调用入口的索引。落定各批次责任与协议命名，记录现有框架可复用能力及真实缺口。复核显式选择默认连接与模型的差异和二进制特性隔离 | 按条记录上游位置、本地入口、裁决、批次和实际测试。尚未准备好实现的成员保留待办。按包启停验证仅有无凭证预设时无网络请求，网关依赖 SDK 的协议实现 |
| AI-01 包发现＋文本 | `AiServiceProvider`、`Ai`、`AiManager` 的命名实例、`build`、`text_provider`、`text_provider_for`、默认及清理，`Agent`、`AnonymousAgent`、`agent()`、`prompt`、文本请求／响应及 Agent Fake。先完成 OpenAI Compatible 与 OpenAI 的非流式纯文本，并明确默认/显式 Provider 及模型优先级 | 启用、未启用及配置缓存后的应用启动。真实 HTTP 路由调用、两个本地协议服务验证 URL／鉴权／JSON／用量。覆盖不支持能力、缺模型、错误响应、Fake 断言及应用隔离。用 Artisan 检查包发现与发布；以 Rust 1.94 试编协议类型并验证不引入重复网络路径 |
| AI-02 多轮文本基础 | `AgentPrompt`、Messages、附件、`HasStructuredOutput`、`StructuredAgentResponse`、`HasProviderOptions`、属性／模型选项、显式默认／廉价／高级模型、`SummarizeAgent` 与 `Str`／`Stringable` 摘要入口、`StreamableAgentResponse`、SSE／Vercel／AG-UI、Usage、提示缓存、代理及每步生成事件；各文本供应商的限流、过载、故障转移及异常 | 相同调用分别实测普通、结构化和流。覆盖 Schema 深层对象／数组、拒答、畸形响应、UTF-8 与行跨数据块、取消与用量、已经向客户端输出时的失败处理。供应商失败分类用协议样本逐个确认 |
| AI-03 工具与会话 | `HasTools`、`Tool`／`ToolCall`／`ToolResult`、校验、修复、检索／延迟加载、文件工具、Provider 工具、MCP 工具桥、Skills、`CanActAsTool`、子 Agent、`HasMiddleware`、`StepTextGateway`、停止条件；`Conversational`、`RemembersConversations`、`ConversationStore`、参与者与归属检查；审批请求、审批结果、恢复相关类型与事件 | 实测多个步骤的 Middleware 次数及顺序、工具调用关联、会话迁移、历史裁剪／分页、跨账户隔离、拒绝／批准／过期、重放及并发恢复。Provider 工具用独立合同约束其支持范围 |
| AI-04 队列、广播和生成器 | `Agent::queue`／`broadcast`／`broadcast_now`／`broadcast_on_queue`、Jobs、Pending、广播事件、失败和大事件，Fake 排队断言；前述 Artisan 生成器、配置／stub／迁移发布 | 独立 Artisan／Worker 进程实际消费，并通过 HTTP／订阅者观察流或广播事件；核对重试、作业间 Provider 清理、私有频道授权及序列化边界。编译模板生成的消费代码 |
| AI-05 向量、分类和排序 | `Embeddings::for`、`Reranking::of`、`Classification::of`、各 Provider／Gateway、Pending／Response、Stringable／Collection 对应便利入口、缓存与数据库桥，附件分类、二元分类与 Fake | 批量及单条、并发、维度／非法浮点、缓存开关／Store／粒度、真实向量查询；排序命中原始索引；分类分别核对问题、标签、附件支持与错误 |
| AI-06 媒体与文件 | `Image::of`、`Audio::of`、`Transcription::of` 及来源入口、`Files` 的上传／读取／删除、`Stores` 的创建／读取／删除、对应 Gateway／Responses、文件与向量仓库关系、非公开文件与附件、下载存储及 Fakes | 用生成的图片和音频等本地样本测试 multipart、媒体返回、存盘、转录、仓库与文件 CRUD、分页、元数据、归属及删除回读。媒体输出通过实际 HTTP 和 Storage 回读 |
| AI-07 收尾 | 逐成员审查所有未关闭分支、供应商子能力及对应模型，核对三语文档、生成产物、支持矩阵和 E07 主账本 | 开启／关闭 feature、Release HTTP／Artisan 及相应 Worker／浏览器实测；按每家每项能力留证据并区分协议模拟与供应商实机验收 |

### AI 模块国际化

从 AI-03 的 [`AgentTool`](../../../vendor/openrust-ai/src/agent_tool.rs) 开始，将默认委托描述和 `task` 参数说明纳入 AI 包语言资源。工具定义生成时按当前应用语言解析译文，用命名占位符传入子 Agent 名称。应用提供的工具说明、提示词和审批理由由应用控制。复用 `Lang`、ServiceProvider 的包语言资源注册、应用覆写及既有回退机制。框架内置译文覆盖[项目规定的 18 种运行时语言](localization-18-locales.md#目标与边界)，沿用 BCP 47。开发者文档维持简体中文、繁體中文和英文。

| 范围 | 验收 |
|---|---|
| 工具 Schema | 实测同一 Agent 在不同语言作用域生成的出站 Schema。两处子 Agent 文案随语言变化，占位符值保持原样，核对普通、流式及工具续步，并逐项检查其他框架提供的工具和参数说明。工具名、`task`、字段类型与必填约束保持原契约。 |
| Artisan 帮助与交互 | 验证实际命令的帮助、参数及选项说明、选择提示和面向用户的失败文案随当前语言变化。命令签名、参数名、退出码及机器可读输出维持原契约。 |
| 审批与用户可见错误 | 审查框架生成的审批提示、工具执行提示和 HTTP／SSE 用户可见错误。在展示或生成响应时解析译文，复核暂停与恢复、CLI 和 HTTP 的对应文案。错误码、HTTP 状态、协议字段、事件类型及第三方协议要求的固定内容维持原契约。 |
| 资源与运行环境 | 校验 18 种资源的键、占位符与译文完整性。用实际 HTTP 和 Artisan 验证简中、繁中、英文及至少一种其他语言，覆盖应用覆写、已配置的回退、同一长生命周期进程的语言切换和并发隔离。验证发布后的消费应用可加载译文，队列路径沿用现有语言作用域，并核对三语开发者文档。 |

## 供应商矩阵

以下 17 个驱动名称取自固定 Laravel 源码，用于跟踪供应商和能力覆盖。OpenRust 按协议实现并通过命名连接验收，首批连接预设见下文。模型清单动态发现或手动选择。工具支持和协议差异依据实际端点及模型验证，对应能力均待实现。

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

Laravel 基线的驱动行为作为能力验收依据，供应商配置通过文档示例接入。可共用的请求、响应、SSE 及工具映射在协议层复用。固定源码中的 `deepseek` 驱动与 `openai-compatible` 已共用 Chat Completions 请求逻辑，可作为复用参考。供应商特有的鉴权、字段及事件差异通过具体协议适配补齐，例如 DeepSeek 的思考模式和 Bedrock Converse 的区域、模型路径及 AWS 凭证。参考 [Laravel DeepSeek Gateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/DeepSeek/DeepSeekGateway.php)及 [AWS Bedrock Converse](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html)。

以供应商、模型、协议和能力为单位记录验收结果。对话可用后，分别验证流、工具调用、结构化输出、Embeddings、图片和音频的实际端点及模型限制。同一家同时开放多个协议时，每种协议用独立命名连接和测试用例。

### 国内供应商接入

在以上 Laravel 供应商与能力覆盖范围基础上，将以下四家纳入可配置的接入示例和兼容验收。应用可同时配置多家供应商，也可为同一家配置多个命名实例，例如两套百炼工作空间。`ai.default` 用于未指定 Provider 的文本调用，其他任务仍可显式选择 Provider 和模型。

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

E07 优先交付 OpenRust 自托管模型网关。部署者自行接入和管理上游账号、定义对外模型及路由、签发并限制下游 Key，客户端通过 OpenRust 网关地址使用这些能力。首个可独立验收的网关须跑通「配置上游账号 → 发布模型名 → 签发下游 Key → 客户端调用 → 查看实际用量」。TokenHub 作为可配置上游及多协议补充参考。

Sub2API 仅作为实现和联调参考。可借鉴账号池、路由、回退和计量行为，并用其实际实例验证通用协议接入。OpenRust 的路由、响应、配置与授权使用自身或对应通用协议的契约。供应商和模型按 OpenRust 连接配置、部署者授权、上游账号权限及实际协议能力确定，Sub2API 的供应商或模型限制不纳入默认策略。

### 成熟实现与复用选择

| 来源 | 核对结果及采用方式 |
|---|---|
| [Sub2API 固定组合路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/docs/COMPOSITE_GROUPS.md) | 按公开模型、请求端点和分组决定目标，再选择该目标的上游账号。用其可观察行为核对别名、组合路由、账号和用量。保留 OpenRust 原生装配。 |
| [Prism Server 固定源码](https://github.com/prism-php/prism/blob/385022b5e0fec9be49a5b8e9a468fdafaa8f1267/src/PrismServer.php) · [路由](https://github.com/prism-php/prism/blob/385022b5e0fec9be49a5b8e9a468fdafaa8f1267/src/Routes/PrismServer.php) | Laravel 生态已有在 ServiceProvider 注册公开名、按路由中间件启用模型目录和 Chat Completions 的实现。采用显式发布、可选 Provider 注册路由、共享 SDK 调用能力的做法。对外兼容路径按 E07 约定。 |
| [LiteLLM Router 固定源码](https://github.com/BerriAI/litellm/blob/3c18ad5c0f5c607359737390d6aded832969b288/litellm/router.py) · [Key、限流与预算](https://docs.litellm.ai/docs/proxy/users) | 参考部署目标分组、按凭证或分组限制访问、冷却，以及失败切换前校验访问权限和限额。用量记录和硬限额分别验收。 |
| [Bifrost 固定路由设计](https://github.com/maximhq/bifrost/blob/53c2fcc22acd5ec4a02b4bd03da9bcb34856c055/docs/providers/provider-routing.mdx) | 参考先选 Provider、再选上游 Key，以及会话黏性仅在可用目标内调整顺序。GW-04 先用可重复验收的顺序、轮询或固定权重规则。企业版自适应评分作为调研记录。 |
| [Portkey 固定项目文档](https://github.com/Portkey-AI/gateway/tree/669825cbe89ee51569918b8f78a9db486fd69dd4) · [TensorZero 重试与回退](https://www.tensorzero.com/docs/gateway/guides/retries-fallbacks) | 分别参考有条件的重试和有序回退、流式首块超时与每次尝试的超时。OpenRust 设置一次请求的总尝试次数和总时限，并保留协议自身的错误分类。 |

运行时保持纯 Rust。上述项目用于验证设计或作为兼容上游，基础协议仍使用 E07 已选的 Rust 类型、框架 HTTP 和既有 Provider 装配，网关按需加载。复核的源码许可分别为 Sub2API LGPL-3.0、Prism MIT、Bifrost Apache-2.0、Portkey MIT；LiteLLM 根许可区分 MIT 部分与企业目录。当前采用行为参考和协议测试样例，源码或依赖复用须核对具体文件许可与接入成本。

网关按 **下游 Key 鉴权与授权 → 公开模型路由 → 上游账号选择 → 协议调用 → 用量记录** 执行。GW-02 每步先提供一个可用选项。GW-03 增加按端点和模型的规则，GW-04 在已授权路由内增加账号池与限额，每次回退重新核对目标权限。保持配置的 `providers`、`driver`、`models` 和项目既有 ServiceProvider／中间件用法；网关账号或路由管理随相关批次接入现有管理入口。

- **下游 Key。** 复用框架现有凭证机制时核对主体、撤销和过期契约，以兼容协议要求的 Bearer 或 `x-api-key` 收取 Key。参考 Laravel Sanctum 的明文只在签发时输出和摘要持久化做法。上游凭证通过运行时引用读取。验收泄露防护、Key 撤销、目录与调用权限一致，以及同一上游账号被多个下游 Key 使用时的归属隔离。 [Laravel Sanctum](https://laravel.com/framework/docs/sanctum) · [OpenRust 凭证实现](../../../vendor/openrust-framework/src/sanctum/personal_access_token.rs)
- **限额与用量。** 请求次数优先复用现有 `RateLimiter::reserve` 和支持对应原子语义的 Cache Store。Token 或金额硬限额单独验收并发扣减、取消、中途断流和结算失败的行为。记录外部请求与各次上游尝试的关联、实际 Provider、账号标识和上游报告用量，无法核实的用量标记为未知；核对重试与回退对配额、审计和总时限的影响。 [OpenRust RateLimiter](../../../vendor/openrust-framework/src/cache/rate_limiter.rs)

| 使用方式 | 装配与行为 |
|---|---|
| 框架内调用 | 应用通过 `openrust-ai` 的 `Ai`、`Agent`、各能力入口和测试 Fake 调用模型。进程内直接使用容器中的 `AiManager`，按配置选直连供应商、自建服务或外部网关。 |
| 接入上游 | `openrust-ai` 的命名 Provider 配置基础 URL、凭证、模型及能力选项。接入 Sub2API 时分别验证 `openai` 的 Responses 和 `openai-compatible` 的 Chat Completions，也按基线验证 Anthropic Messages、Gemini 原生入口。先复用通用协议驱动，实际存在网关专属语义时再确定适配方式。 |
| 对外服务 | 采用独立可选的 `ai-model-gateway` 工作名称，允许单独部署。外部客户端按已有协议和鉴权头访问 OpenRust 网关。网关复用 SDK 的供应商适配与 HTTP 传输，通过自己的路由、凭证与限额策略调用上游。SDK 的 `Gateway` trait 仍表示应用侧协议适配。 |

固定版 [Sub2API 路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/server/routes/gateway.go)、[Responses 桥接](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler_responses.go)和[组合路由设计](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/docs/COMPOSITE_GROUPS.md)作为首批参考。优先走同协议上游。跨协议时明确映射请求、响应、工具 ID、流事件、停止原因、错误和用量；对于无法表达的能力返回相应协议的错误。外部客户端的工具调用按协议返回供客户端处理，应用自有 Agent 工具仍由 SDK 按权限与审批配置执行。

GW-03 参考 [TokenHub 的统一入口和模型协议矩阵](https://cloud.tencent.com/document/product/1823/130079)。在同一基础 URL 下，以 `/chat/completions`、`/responses` 和 `/messages` 区分协议，验证 Bearer 与 `x-api-key` 的对应入口。逐模型记录原生、协议转换及不支持的行为。目录发现时分别验证模型在线状态、账号开通和实际调用，记录转换兼容模式的字段与工具限制。

### OpenRust 自有 AI HTTP 接口暂缓

当前 E07 暂缓 OpenRust 自有 AI HTTP 接口及其挂载配置，包括 `ai.route_prefix` 和 `AI_ROUTE_PREFIX`。有明确的使用需求时，再随首个自有接口一并设计、实现和验收。默认 `/ai` 与完整前缀只挂载一次的约定留作后续设计依据。

E07 保留的网关协议兼容路由沿用现有路径、启用条件与响应契约，包括 `/v1`、`/v1beta`、`/backend-api/codex`、`/v1/usage` 以及可选的无版本别名。

移除 `GET /v1/sub2api/billing` 的路由注册、专用响应及 `sub2api.key_billing` 标识，同步清理 `zh-Hans`、`zh-Hant`、`en` 开发者文档中的该接口说明。GW-05 通过 `route:list` 和 HTTP 验证该入口已移除，并核对 `/v1/usage` 等保留路径及 OpenRust 自有的计量、计费与对账行为。已有 Sub2API 联调记录及上游参考链接作为历史证据保留。

### 模型配置、发现与发布

| 入口和用途 | 约定 |
|---|---|
| `config/ai.rs` 中的 `ai.providers.<name>` | 保存连接、协议、凭证引用、手填或经确认选择的上游模型，以及 `models.text.default` 等能力默认值。SDK 直接调用时解析命名连接与模型 ID。 |
| Artisan 模型选择 | 同一命名连接显示服务器返回的模型和手填模型，按原始模型 ID 去重，保持大小写。已保存的设置优先，发现结果补充可用的展示信息。刷新时保留手填及已选模型。不同连接的同名模型分别展示。查询失败时保留可编辑列表与错误原因。 |
| 网关发布配置，建议文件名 `config/ai_gateway.rs` | 每个公开模型名显式映射到 `provider` 与真实 `model`。可以发布自定义名，也可以显式发布上游原名。多个公开名可以指向同一模型。首批使用精确模型名映射一个目标，后续按固定协议与路由需求增加端点匹配和回退。默认发布表为空。 |
| 对外模型目录与调用 | `/v1/models` 返回当前 Key 在该协议上获准调用的已发布模型；发现模型作为可选集合，由显式操作加入发布配置。列表与调用共用发布和授权规则。核对两个 Key 的可见集合、未发布或无权限模型、映射冲突、禁用以及同一别名改绑后客户端配置保持可用。 |

示例关系为 `support-chat → deepseek / deepseek-flash`。网关用发布名解析目标，以真实 ID 请求上游，通过协议定义返回对外模型标识。管理与审计可按权限读取实际路由。配置应在启用时验证发布名称冲突、目标连接及已知协议能力，手填可用的模型以试调用确认。框架内的 SDK 模型参数沿用 Laravel 语义，网关发布名归网关配置。

### 网关分批交付

| 批次 | 交付与验收 |
|---|---|
| GW-00 协议与能力清单 | 按固定 Sub2API 源码建立端点、上游协议、账号类型、能力选项及管理行为清单。除主协议外，逐项裁决 `/models/{id}`、`/messages/count_tokens`、Responses 子路径／WebSocket／状态、模型目录、Embeddings、图片／视频／语音、兼容别名、用量等公开路由。将请求方法、路径／版本、鉴权与能力、实现批次和证据逐条记录；固定源码有路由只证明存在入口。商业平台界面与业务仍按独立项目范围确认。 |
| GW-01 接入现有网关 | 随 AI-01／02／05／06 使用真实 Sub2API 进程和本地协议服务，分批验收普通请求、流、工具、结构化输出、Embeddings 及媒体。记录网关版本、端点、分组、上游及模型；供应商实机结果单独记录。 |
| GW-02 自托管网关最小闭环 | 对外提供 `/v1/models`、`/v1/responses`、`/v1/chat/completions`。以一个上游 API Key、精确模型映射、独立下游 Key、模型授权和调用用量跑通部署与客户端调用。核对 HTTP 状态、协议错误、鉴权头、JSON、SSE 和取消。支持 Responses 持久状态与否按操作和上游实际能力裁决，覆盖 `store`、`previous_response_id` 及对不支持的请求的响应。 |
| GW-03 多协议与路由 | 补 Anthropic Messages、Gemini 原生入口及双向协议适配，首个跨协议验收为 Responses 请求经 Anthropic 上游返回 Responses 事件。覆盖模型别名、按端点和模型选路、能力检测、同协议透传、回退限制。Embeddings、图片、音频、WebSocket 等按 GW-00 实际能力矩阵推进。 |
| GW-04 账号池与运维 | 在 GW-02 基础上交付上游多账号池（API Key、适用的 OAuth／订阅账号）、按组授权、账号选择及会话黏性、冷却和故障切换。补下游 Key 签发、轮换和撤销、分组与模型授权、并发和用量限额、审计、精准计量、货币计费与账单对账。覆盖后台配置或运维入口，按账号类型验证授权生命周期、失效、路由回退与调用归属。跨租户、部分失败及账务一致性按[精准用量、计费与账单对账](#精准用量计费与账单对账)验收。 |
| GW-05 联合验收 | 同一合成任务分别从应用 SDK、经 Sub2API 的上游调用、现有外部客户端访问 OpenRust 网关完成。覆盖直接连接、协议转换和实际路由，对照各路径的用量证据、应收和待核对范围，验证接入文档、三语支持矩阵和 E07 账本。 |

网关回退与 SDK failover 独立验收。在向客户端发出首个可见事件之前，按错误分类和能力允许范围选择备援；输出开始、工具产生副作用或上游是否已接受请求不确定时，按对应协议处理失败并记录用量状态，避免自动重放。对外路由和上游连接各做一次明确的选路决策，验收组合启用时的总尝试边界。兼容结果以实际调用和可观察行为为准，模型目录可见、HTTP 路由存在和单一模型成功各自保留验证边界。E07 在 SDK 与网关批次均完成、三条使用路径均通过相应验收后关闭。

### 精准用量、计费与账单对账

AI-02 负责供应商用量字段和流式聚合，AI-03 负责 Agent 多轮关联，GW-04 负责生产账本、计费、预算和对账，GW-05 验收三条调用链。复用现有用量记录、Key 归属、`/v1/usage` 和 Artisan 查询。SDK 直连可交付结构化用量。SDK 经本地网关调用时，网关作为该次上游计费的入账点，SDK 用量可供应用观察。同一个调用跨层记录时，通过调用标识关联，避免重复入账。网关货币计费与硬预算显式启用，普通模型发布只需现有配置。

| 契约 | 设计与验收 |
|---|---|
| 计量单位与归属 | 关联下游请求或 Agent 调用、模型回合、上游尝试，以及可获得的供应商请求 ID。记录授权主体、公开模型、实际连接／账号、已知实际模型和端点。以固定 Laravel `TextUsage` 对齐输入、输出、缓存读写与推理子项。计费按供应商定义拆出互斥用量，避免把已包含的子项再次计费。媒体、搜索及服务工具按适用的 Token、秒、图片数或调用次数处理。明确区分完整回报、部分回报、未知及明确零。旧记录保留已有可证实的用量和归属。 |
| 流式完成与断开 | 同一尝试中，累计帧按字段合并，增量帧仅按协议定义累加，重复终帧不产生新账目。区分下游断开、上游中断、上游最终用量已落账和缺少最终用量。已有可信最终用量即进入结算，缺失则保留已知数据和待核对状态。验收首帧前失败、已输出后断开、完整终帧后的断开、终帧缺失、重复和迟到、进程退出以及迟到证据补录。 [OpenAI 流式用量](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events) |
| 工具多轮、重试与切换 | 每次实际模型回合与每次实际出站尝试分别留证。据已确认回报归集各上游成本，共享同一逻辑请求。下游按已生效的计费规则收取应计回合，默认由运营方承担内部失败重试和回退尝试的成本。账本保存失败尝试，以及已发送但是否执行未知的尝试。客户端重发只在端点有幂等契约且同一主体和请求指纹匹配时复用原结算。同键异参报冲突，已发生工具副作用的请求遵守现有重放边界。验收工具审批续跑、同账号重试、跨账号／模型回退、SDK 与网关嵌套尝试及上游实际用量各不相同的情况。 |
| 价格与金额 | 记录请求时可追溯的价格和计费政策版本，包括实际供应商模型、适用档位、币种、计价单位、费率、规则及舍入点。区分上游成本与下游应收金额，用可持久化的定点十进制精确计算和汇总。同一币种内聚合，跨币种折算须有明确汇率来源及版本。价格缺失或上游身份不明时，相关成本保持待核对。有依据的明确免费才记零。验收折扣、跨日换价、退款或贷项调整、汇总时舍入和多币种。 |
| 入账与预算 | 用数据库唯一约束和事务防重。出站前持久化尝试关联，结算时原子更新账目及对应预算状态。使用稳定的入账键处理回调重送、任务重投、并发结算和提交后响应丢失。已启用硬预算时，按可证实的上界预占，结算后扣减并释放差额。无已知上界的能力按事先配置的拒绝或仅监测策略处理。超时预占可恢复且待核对成本继续可见。验收两进程竞争、事务回滚、进程重启以及扣减与释放的幂等性。 |
| 供应商账单对账 | 先用现有 Artisan 入口提供可重复的账单导入、差异查看和修正，能经过授权获取报表时再加适配器和调度。按账单自身的时区、窗口、币种和可验证粒度匹配。可用请求 ID 时逐笔匹配，只提供账号／项目／日期聚合时在同粒度对账，保留未归属差异。报表迟到、更正、重复导入、费用分类及舍入差异均可追溯。以追加调整修正已结算记录，关联原账目、证据和执行主体。外部 Sub2API 仅提供 Token 时可按本地价格计费，上游实际成本留待账单或授权报表确认。 |
| 运维与关闭条件 | 保留 `/v1/usage` 既有 Key 隔离和字段语义。下游查询只读自己的账目，运维入口按权限处理上游成本、差异及调整，记录仅保留结算所需元数据。使用本地合成上游和可复算账单，通过真实 HTTP、Artisan 和适用 Worker 核对尝试总数、用量、成本、应收、预占余额及差异。对已授权实例单独记录实机能见度。三语文档给出默认计费策略、待核对状态和排查方式。 |

#### GW-04 账务验收断言

| 场景 | 通过条件 |
|---|---|
| 流中断与结算竞态 | 分别在请求未发出、可能已被上游接受、已有输出及最终用量已收到的阶段断开。可确认未出站的尝试无上游费用，可能已接受的尝试保留待核对。已收到的最终用量经重启仍可结算。模拟重复、乱序、迟到、冲突及缺字段的用量证据，验证完整终值优先于部分值，冲突进入核对，部分报表不能把已确认值清零。 |
| 多轮与多次出站 | 固定一个两轮工具调用，第二轮先失败再切换。证据给出三次上游成本分别为 `0.12`、`0.03`、`0.08`，两轮下游应收分别为 `0.15`、`0.10`。同币种断言总成本 `0.23`、应收 `0.25`、三个尝试和一个逻辑请求。逐次核对实际账号和模型。再覆盖拒绝或重复审批、重复工具结果、失败尝试晚到、协议转换和 SDK 经本网关时只扣一次。按端点增加长连接多次生成和异步任务完成／取消用例，使用生成或任务标识结算。 |
| 本地响应缓存与伪造数据 | 如果响应缓存命中且没有出站，只记录本地命中，上游成本为零，下游按明确的缓存定价规则结算。客户端提交的 Token、价格和上游标识不进入可信账本。供应商请求 ID 在正确的连接或账号范围内用于关联，验证不同账号的同名 ID 不会互相去重。 |
| 预算与数据库故障 | 在支持的数据库上实测资金入账、有效预占和释放的原子语义。启用硬预算时，并行请求及回退尝试不得绕过各层适用预算。验证数据库暂不可用、预占超时、提交后响应丢失、账期边界和历史迁移。待核对尝试的预占处置与晚到费用须独立验证。硬预算缺少已知上界时，明确拒绝或由部署者为该能力配置仅监测模式。 |
| 对账、修正及可见性 | 锁定同一数据快照，验证旧报表、分页中断、重复导入、更正报表及人工重复修正。分页未完成的报表不标成已对账，恢复后只补缺页。部分账单、共享上游账号和跨日时区差异保留可解释余额，不把聚合差异摊成逐次费用。核对期初调整、扣费／贷项以及期末净额可从账本复算。验收 Key 轮换／撤销、跨租户查询拒绝、余额预占和待核对状态可查。 |

采用固定 [Laravel AI TextUsage](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Responses/Data/TextUsage.php) 的用量口径和 [Laravel 数据库事务](https://laravel.com/framework/docs/13.x/database#database-transactions) 的装配习惯。固定 [Sub2API 计费仓库](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/repository/usage_billing_repo.go) 提供去重与扣减同事务的参考，[LiteLLM 价格表](https://docs.litellm.ai/docs/proxy/model_cost_map) 提供按用量类别定价的参考。复用现有框架事务、Cache、队列、调度和 Artisan。对照 [Laravel Cashier](https://laravel.com/framework/docs/13.x/billing#reporting-usage) 与 [Stripe 用量事件](https://docs.stripe.com/billing/subscriptions/usage-based/recording-usage-api#idempotency) 的入口设计，支付与订阅继续由现有 Pay／Cashier 承担，网关账本可提供已结算用量供其消费。对账样本按 [OpenAI Costs](https://developers.openai.com/api/reference/resources/admin/subresources/organization/subresources/usage/methods/costs) 和 [Anthropic Usage and Cost API](https://platform.claude.com/docs/en/manage-claude/usage-cost-api) 支持的报表字段设计。

## DSH 式模型接入

### 首批供应商预设

`config/ai.rs` 的每个预设都显式写出默认基础 URL。按项目配置习惯使用 `env.get_utf8("变量名")?.unwrap_or("默认基础 URL")`。环境变量未设置时使用默认地址，设置后以环境变量的完整值替换。对带凭证启用的连接验证 URL，显式空值或非法地址返回配置错误。`.env.example` 可列出 URL 覆盖示例，未覆盖的项目可省略该变量。

| 连接名 | `driver` | 默认 `url` | URL 覆盖变量 | 凭证变量 |
|---|---|---|---|---|
| `deepseek` | `openai-compatible` | `https://api.deepseek.com` | `DEEPSEEK_URL` | `DEEPSEEK_API_KEY` · [官方文档](https://api-docs.deepseek.com/) |
| `qwen` | `openai-compatible` | `https://dashscope.aliyuncs.com/compatible-mode/v1` | `QWEN_URL` | `DASHSCOPE_API_KEY` · [百炼基础 URL](https://docs.modelstudio.console.alibabacloud.com/en/model-studio/base-url) |
| `tencent` | `openai-compatible` | `https://tokenhub.tencentmaas.com/v1` | `TOKENHUB_URL` | `TOKENHUB_API_KEY` · [TokenHub 调用概览](https://cloud.tencent.com/document/product/1823/130079) |
| `qianfan` | `openai-compatible` | `https://qianfan.baidubce.com/v2` | `QIANFAN_URL` | `QIANFAN_API_KEY` · [千帆文本生成](https://cloud.baidu.com/doc/qianfan-api/s/wmwbl48n4) |
| `ark` | `openai-compatible` | `https://ark.cn-beijing.volces.com/api/v3` | `ARK_URL` | `ARK_API_KEY` · [方舟兼容 OpenAI SDK](https://docs.volcengine.com/docs/ark/compatible-with-openai-sdk?lang=zh) |
| `openai` | `openai` | `https://api.openai.com/v1` | `OPENAI_URL` | `OPENAI_API_KEY` · [Responses API](https://developers.openai.com/api/reference/resources/responses/methods/create) |
| `anthropic` | `anthropic` | `https://api.anthropic.com/v1` | `ANTHROPIC_URL` | `ANTHROPIC_API_KEY` · [Messages API](https://platform.claude.com/docs/en/api/messages/create) |
| `gemini` | `gemini` | `https://generativelanguage.googleapis.com/v1beta` | `GEMINI_URL` | `GEMINI_API_KEY` · [Gemini 原生 API](https://ai.google.dev/api/generate-content) |

百炼使用北京共享域名作为示例默认值，使用其他地域或工作空间专属域名时覆盖 `QWEN_URL`。TokenHub 默认使用广州地址，新加坡地域可将 `TOKENHUB_URL` 设置为 `https://tokenhub-intl.tencentmaas.com/v1`。覆盖后的地址仍须与凭证及指定协议匹配。

`url` 是基础地址，保留自带的版本与代理路径前缀。协议驱动只追加相对操作路径，例如 `chat/completions`、`responses`、`messages` 或 `models`，确保 `/v1`、`/v2`、`/v1beta` 和 `/api/v3` 恰好出现一次。AI-01 使用本地模拟覆盖各预设的默认地址、覆盖地址、自定义路径前缀及有无末尾斜杠。按实例鉴权和发现模型的流程用同一基础 URL；提供特殊目录端点的服务按已验证协议适配。

### 添加与选用模型

1. **选择或添加连接。** 使用预设时配置对应的 Key，必要时覆盖 URL。自建服务或 Sub2API 在 `ai.providers.<name>` 选择 `driver`，填写基础 `url` 和凭证引用。`openai-compatible` 走 Chat Completions，`openai` 走 Responses，Anthropic 与 Gemini 使用各自驱动。网关提供多种协议时可为不同协议配置命名连接。验收协议鉴权和模型 ID 的大小写。
2. **动态发现。** 使用当前命名连接对应的凭证和目录端点，请求按协议保留版本路径、处理分页并读取模型 ID。端点示例见 [OpenAI `/v1/models`](https://developers.openai.com/api/reference/resources/models/methods/list)、[Anthropic `/v1/models`](https://platform.claude.com/docs/en/api/models/list)、[Gemini `/v1beta/models`](https://ai.google.dev/api/models)及 [DeepSeek `/models`](https://api-docs.deepseek.com/api/list-models/)。模型能力元数据按上游实际声明显示，缺少描述的能力标记为未知。目录缺失、为空、无权限或返回失败时，保留已配置模型并允许手填模型 ID。
3. **选用和试调用。** Artisan 显示目录结果并允许手填，用户确认后才写入选择结果。刷新目录时保留已选项和 `models.text.default` 等显式设置。发现目录和实际调用分别验证，且使用同一命名连接。实测覆盖分页、缺字段、错误、空目录、无目录、已选模型在刷新后消失及手填后仍能按协议调用。工具、流、图片等能力以对应调用结果为准。Artisan 命令名及参数作为 OpenRust 扩展随实现核对。
4. **对外接入。** OpenRust 网关以对外模型名映射实际 Provider 和模型 ID，客户端只配置地址、匹配的协议、网关凭证和对外模型名。用固定版 DSH 的自定义模型 API 完成拉取列表（支持时）、手填、选择和真实调用；覆盖 Chat Completions、Responses 及后续 Anthropic Messages、Gemini 原生入口，按批补工具和 SSE。沿用 Sub2API 的模型路由验收，并继续使用本地模拟上游和授权实机分别留证据。

DeepSeek 官方端点作为首个完整示例。使用 Chat Completions 的命名连接对照固定 Laravel `deepseek` 驱动行为，测试文本、流、思考模式和工具回合的 `reasoning_content`。DeepSeek 官方另提供 Responses 和 Anthropic Messages，作为对应协议的独立配置案例，分别验收请求字段和能力。参考 [Laravel DeepSeek Provider 固定源码](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Providers/DeepSeekProvider.php)、[Prism DeepSeek 文档](https://prismphp.com/providers/deepseek/)、[DeepSeek 首次调用](https://api-docs.deepseek.com/)、[Responses](https://api-docs.deepseek.com/guides/responses_api/)、[Anthropic](https://api-docs.deepseek.com/guides/anthropic_api/)及[思考模式](https://api-docs.deepseek.com/guides/thinking_mode/)。

## 实施顺序与完成条件

1. AI-00 与 GW-00 先确认契约、可选包边界和能力对应。按类型或端点保留待核对项，计划复审结果用于确定批次，成员级清单继续随实现核对。
2. AI-01 验收无凭证启动、八家预设及 URL 覆盖、显式连接和非流式文本，GW-01 用现有 Sub2API 验证接入。随后接模型发现、选择与独立试调用。配置发布只写包拥有的预设和示例，用户确认的连接或模型写入方式按现有配置管理约定验收。
3. AI-02 完成出站流能力后，GW-02 先验收有上下游凭证、模型映射和用量记录的自托管网关，再按 GW-03／04 推进多协议路由、上游账号池、下游权限和运营能力。AI-03 至 AI-06 按依赖补齐 SDK 能力。SDK 可独立启用，网关复用 SDK 专用调用所需的协议实现及框架公共能力。
4. AI-07 与 GW-05 用独立消费者、真实 HTTP／Artisan、适用的 Worker 与 DSH 客户端验收三种调用路径；逐项比对三语配置示例、发布行为、供应商能力矩阵及主账本。计量、计费和对账按 GW-04 的账务验收逐项闭合。[AI 模块国际化](#ai-模块国际化)从 AI-03 工具文案开始，随所属功能逐批实现，在 AI-07 核对台账与实际入口。兼容性记录保留上游版本、模型、协议、能力、凭证类型及实测条件。公开宣告的能力有实际入口证据；缺账号、地区、权限等条件的能力保持待实机验收。
