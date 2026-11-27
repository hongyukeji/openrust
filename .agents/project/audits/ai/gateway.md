# GW-00 网关端点账本

基线为 Sub2API [`0f6671457132425d1b84f3d80536b53e02942424` 的路由与中间件装配](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/server/routes/gateway.go)。固定源码用于确定可访问的路径及准入方式。下表中「待验收」表示 OpenRust 尚无对应的端到端证据。

## 主协议

| 固定基线路由 | 鉴权和行为 | OpenRust 裁决 |
|---|---|---|
| `POST /v1/chat/completions` | 网关 Key 与分组模型白名单，依上游平台派发，普通与 SSE | 已验收 OpenAI 同协议和 Anthropic 转换的 JSON、SSE、下游取消与客户端函数工具。上游选择覆盖配置回退、组授权、冷却、并发及会话黏性。 |
| `POST /v1/responses` | 网关 Key 与分组，原生 Responses 或替代上游 | 已验收原生 Responses、转换到 Messages 和 Chat Completions 的普通及 SSE 文本与客户端函数工具。Chat 转换另覆盖 JSON 对象模式。结构化 Schema、附件、推理及其他高级字段仍待核对。 |
| `POST /v1/responses/*subpath` | 网关 Key，路由守卫只放行允许的操作路径，`input_tokens` 可独立派发 | 已验收 `input_tokens`，可使用 OpenAI 原生计数或映射到 Anthropic 和 Bedrock 计数。其余子路径待核对。共享凭证当前拒绝 `store: true`、`background: true`、`previous_response_id`、会话与旧资源引用。持久状态先取得租户与账号归属证据再开放。 |
| `GET /v1/responses` | 网关 Key，升级 WebSocket 并走专用 Responses handler | GW-03 已实测下游 WebSocket 串行生成、首段、断开取消、权限撤销、关闭码和 HTTP 上游转换。已验收同连接的文本与客户端函数续接、四个并行通道和分叉；上游 WebSocket、预热和转向待核对。 |
| `POST /v1/messages` | 网关 Key 与分组，按上游平台在 Messages 与 OpenAI 网关间派发 | 已验收 Anthropic 同协议、独立凭证和原生错误。到 Responses 与 Chat Completions 的普通、SSE 文本及客户端函数工具均已通过真实 HTTP 协议模拟。原生资源和签名推理块仍待核对。 |
| `POST /v1/messages/count_tokens` | 网关 Key，按供应商选择原生计数、桥接或本地估算 | 已验收 Anthropic 原生计数及映射到 OpenAI Responses 和 Bedrock 计数。上游返回的计数与生成用量分开；其他上游仍待核对。 |
| `GET /v1/models`、`GET /v1/models/{model}` | 网关 Key，按授权返回目录或单模型；列表中 `client_version` 有 Codex 模型清单分支 | GW-02 OpenAI 列表，GW-03 单模型及 Anthropic 格式。GW-05 的 `client_version` 清单、鉴权、别名及 ETag 已通过 HTTP 和 Artisan，固定版 Codex 已通过真实进程验收。 |
| `GET /v1beta/models`、`GET /v1beta/models/{model}`、`POST /v1beta/models/{model}:{action}` | Google 协议 Key 与分组，原生模型目录及动作派发 | GW-03 已验收目录、单模型、`generateContent`、`streamGenerateContent` 的 JSON 数组与 SSE、`countTokens`、`x-goog-api-key` 及 Google 认证错误。另已验收原生 `embedContent`、`batchEmbedContents`、目录能力、同一公开模型重写及共享文件隔离。内联异步批任务见下文。 |
| `POST /v1beta/models/{model}:{batchGenerateContent,asyncBatchEmbedContent}`、`GET /v1beta/batches[/{id}]`、`POST /v1beta/batches/{id}:cancel`、`DELETE /v1beta/batches/{id}` | Google Batch API 原生生命周期，资源归属及单项计量 | GW-03／04 已验收内联任务及文本生成批次硬预算和输出配额。结果下载和文件型任务提交见下文。 |
| `POST/GET /v1beta/cachedContents`、`GET/PATCH/DELETE /v1beta/cachedContents/{id}` | Google 协议 Key 与分组，网关资源 ID 绑定上游账号 | GW-03 已验收创建、分页、读取、过期更新、删除及普通／SSE 生成引用。GW-04 已验收生成缓存命中计费，缓存创建和存储费用待对账。 |
| `POST /v1/embeddings` | 网关 Key 与分组，固定版本限制 OpenAI 平台 | AI-05／GW-03 已验收 OpenAI 协议 JSON、维数、批量、公开模型与真实 Token。其他原生协议待核对。 |

## 媒体、实时与运维

| 固定基线路由 | 鉴权和行为 | OpenRust 裁决 |
|---|---|---|
| `POST /v1/images/generations`、`POST /v1/images/edits` | 网关 Key，平台适配生成／编辑、JSON 或 multipart | GW-03 已验收 OpenAI 同协议的 JSON 生成与编辑、multipart 编辑、SSE、模型映射、分组及用量，以及 Bedrock 普通 JSON 文生图。其他原生上游及任务接口分别验收。 |
| `POST /v1/images/{generations,edits}/async`、`GET /v1/images/tasks/{id}` | 网关 Key，异步任务写入及读取 | GW-03／04 待验证任务所有权、失效及结果读取。 |
| `GET/POST /v1/images/batches`、`GET /v1/images/batches/models`、`GET/DELETE /v1/images/batches/{id}` | 网关 Key，批量任务增删查与模型目录 | GW-03／04 待按批次、租户持久化能力裁决。 |
| `GET /v1/images/batches/{id}/items`、`GET /v1/images/batches/{id}/items/{custom_id}/content`、`GET /v1/images/batches/{id}/download`、`POST /v1/images/batches/{id}/cancel`、`DELETE /v1/images/batches/{id}/outputs` | 网关 Key，任务项、结果、取消与删除 | GW-03／04 待验证资源所属、二进制结果、部分失败和可重试性。 |
| `POST /v1/videos`、`POST /v1/videos/{generations,edits,extensions}`、`GET /v1/videos/{id}[/content]`、`GET /v1/videos/{generations,edits,extensions}/{id}[/content]` | 网关 Key，依平台支持创建／编辑／扩展、状态与二进制读取 | AI-06／GW-03／04 待按平台验证任务 ID、状态、取消与内容。 |
| `POST /{api/v3,v3,v1}/contents/generations/tasks`、`GET/DELETE /{api/v3,v3,v1}/contents/generations/tasks/{id}` 及无版本同路径 | 网关 Key，Seedance 任务创建、查询和删除 | GW-03／04 待按国内视频供应商任务及资源所有权验收。 |
| `POST /v1/tts`、`POST /v1/stt`、`GET/POST /v1/custom-voices`、`GET/PATCH/DELETE /v1/custom-voices/{id}`、`GET /v1/custom-voices/{id}/audio` | 网关 Key，固定版本依平台开放；文本、上传、下载或资源管理 | AI-06／GW-03／04 待逐操作核对，音色资源要求所属校验。 |
| `POST /v1/live`、`GET /v1/live/{call_id}`、`GET /v1/realtime` | 网关 Key，Live 与 Sideband 或实时协议升级 | GW-03／04 待验收不同实时入口的上游支持、双向生命周期与归属。 |
| `POST /v1/systemone`、`POST /v1/alpha/search`、`POST /v1/{web_search,x_search}` | 网关 Key 与平台能力门控，部分动作有独立文本正文限制 | GW-03 待按原生平台能力裁决，分别验收错误和限制。 |
| `GET /v1/usage` | 网关 Key，调用方用量 | GW-04 已实测按 Key 聚合、UTC 日序列及按公开模型分日期范围统计。旧 Sub2API 专用计费入口的历史联调和移除结果见本账本末节。 |

## 兼容路径与账号类型

| 基线入口 | 裁决 |
|---|---|
| 无版本 `/models[/{model}]`、`/messages/count_tokens`、`/responses[/*subpath]`（POST、升级 GET）、`/chat/completions`、`/embeddings`、图片／视频／语音／实时／搜索部分路径 | 通过 `unversioned_paths` 可选启用目录、Chat、Responses、`input_tokens`、Embeddings 与 Messages 计数的 HTTP 别名，实测共用授权及限流，并验证 `GET /responses` WebSocket 别名。其他无版本操作待验收。 |
| `/backend-api/codex` 下的 `GET /models`、`POST /responses[/*subpath]`、`GET /responses`、`POST /alpha/search`、`POST /realtime/calls`、`GET /{call_id}` | GW-05 已实测专用模型目录、Responses 生成与输入计数、WebSocket。搜索与实时会话待验收，专用账号归 GW-04。 |
| `GET /antigravity/models`、`/antigravity/v1` 下的目录、Messages、计数和用量、`/antigravity/v1beta` 下的 Gemini 目录与动作 | GW-04／05 待结合对应账号和平台能力裁决与验收，保留专用路由的账号隔离。 |
| 上游 API Key、OAuth／订阅、平台专用连接与组合分组 | API Key 已通过 SDK 与自托管网关验证。同名模型按端点、原生／转换支持和账号可用性选路。OAuth 刷新、订阅限制、会话黏性、分组模型白名单和跨账号失败回退归 GW-04；管理与发布入口验收归 GW-05。 |

## GW-04 上游账号冷却

`accounts.<provider>.cooldown_seconds` 以命名连接标识账号。收到 429、503 或 529 后，优先使用标准 `Retry-After`，缺少有效值时按配置冷却。所有引用该连接的模型共用状态。复用项目 Cache 和所有者锁，多个并发失败保留更晚的截止时间。冷却账号在发送前退出候选，依序选可用目标。无可用且有冷却候选时返回协议 HTTP 503 和最早重试时间。没有配置冷却的连接沿用原行为。

包测试与本地上游验证跨模型别名、原有限流及重放边界。独立 Artisan 验证配置，两个独立 HTTP 网关进程使用同一隔离 Redis 8，验证从一个进程触发到另一个进程路由、过期自动恢复、标准 HTTP 日期、并发更新时间及凭证脱敏。账号容量、粘性、分组和持久化管理归后续批次。

## GW-04 上游账号并发容量

在同一个账号配置中增加 `max_concurrent_requests`。参考固定 Sub2API 的 [ConcurrencyService](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/service/concurrency_service.go) 保留独占账号槽位和所有者语义。OpenRust 复用 Cache 的并发限流器，当前按候选顺序探测有空位的连接。响应流持有槽位，完成或断开时释放。进程异常退出由缓存租约到期恢复，内置协议请求的超时短于租约。全部候选无可用槽位时返回协议 503。

独立 Artisan 验证配置。两个网关进程共享 Redis 8，经本地上游验证相同账号跨别名和进程的忙碌状态、切换备用账号、原生 SSE 完成以及客户端断开导致的回收。框架 Cache 回归覆盖直接获取槽位、主动释放和丢弃后的复用。

## GW-04 下游分组授权

按固定 [Sub2API 组合路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/docs/COMPOSITE_GROUPS.md) 核对账号候选与分组归属。OpenRust 在现有模型映射上增加 `groups.<name>.models` 和可选的 `providers`。环境 Key 使用 `group` 继承分组范围，自身 `models` 可进一步收窄。模型发现、单模型读取、首选账号与备用账号均使用同一个分组权限，调用日志记录分组名称。

HTTP Kernel 和独立网关进程验证分组 Key 的列表、模型交集、不同账号选择，以及主账号 429 时未授权备用账号无请求。Artisan 检查配置和凭证脱敏。持久化 Key 生命周期、账号黏性和分组额度仍在后续批次。

## GW-04 数据库托管 Key

`managed_keys` 独立启用，通过 Artisan 签发、列出、轮换及撤销。复用 Laravel Sanctum 的摘要存储原则和框架安全随机源，明文仅在签发／轮换命令输出。组授权沿用网关配置，按 Key ID 计限流，轮换不会重置限额。认证走写库连接且实时核对到期与撤销，轮换用旧摘要条件更新，撤销移除摘要。使用专用迁移表，不混用应用用户的访问令牌。迁移、发布与自定义连接由原生 Artisan 提供。

本地 SQLite、MySQL 8.4.13 与 PostgreSQL 18.7 的独立运行验证迁移发布／回滚、摘要读回、立即轮换与撤销、过期、授权模型隔离和模型调用。数据库表不可用时托管 Key 返回脱敏 503，配置中的环境 Key 仍可鉴权。管理操作历史、按用量额度扣减和控制台页面待对应批次。

## GW-03 Responses 与 Anthropic Messages 的非流式工具转换

参考固定 Sub2API 的 [Anthropic 到 Responses](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/pkg/apicompat/anthropic_to_responses.go) 和 [Responses 到 Anthropic](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/pkg/apicompat/responses_to_anthropic.go)。转换标准客户端函数定义、工具选择、对象参数、文本结果和调用 ID。Anthropic 错误结果转为带错误标记的文本。拒绝重复、缺失或不匹配的工具历史，包含工具历史的上游调用不自动重放。

HTTP Kernel 及独立网关验证两个方向的工具调用、上游鉴权、结果回送和普通生成。测试覆盖并行 ID、无效历史、流式工具拒绝以及原有文本 SSE。Artisan 检查两个协议路由。流式工具转换、非文本结果和服务端工具另列待办。

## GW-03 Responses 与 Messages 流式工具转换

两端工具声明与完整历史沿用非流式转换。Anthropic 的 `input_json_delta` 和 OpenAI 的 `response.function_call_arguments.delta` 实时双向转换，保留调用 ID、事件索引和终止用量。最终参数须是合法 JSON 对象，且与之前实际发送的增量一致。缺失或矛盾的完成帧终止为协议错误，已开始的流不重放。

包测试覆盖调用开始、分段参数、工具结束、终止用量和错误；独立进程的两条 HTTP 路径均在上游等待信号期间收到第一段参数，Artisan 核对路由。服务端工具、附件及特殊工具协议仍待后续批次。

## GW-04 会话黏性

可选的 `account_session_ttl` 使用现有 `account_cache_store`，请求带上 `X-Session-ID` 后优先选择同一个下游 Key 和公开模型最近成功的具名连接与上游模型。缓存键为长度分隔后计算的摘要。候选仍须通过端点能力、组授权、冷却及并发检查。成功的 JSON 及已确认终止的 SSE 刷新记录，失败与截断不写入。缓存出错按正常路由顺序处理请求。

HTTP Kernel 验证非法和重复 ID、Key／模型别名隔离、首选失败后将可用连接写回。独立 Artisan 确认配置，两个 HTTP 进程共享 Redis 8，实测先回退并完成 SSE、跨副本命中、截断不黏连以及有效期结束回到主连接。协议上游使用真实 TCP 模拟。

- [Sub2API 固定 Responses 账号选择](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler_responses.go) · [Bifrost 固定路由设计](https://github.com/maximhq/bifrost/blob/53c2fcc22acd5ec4a02b4bd03da9bcb34856c055/docs/providers/provider-routing.mdx)

## GW-03 Responses 到 Chat Completions

公开模型在授权和端点策略允许的情况下，Responses 优先使用原生连接，其次可选 Messages 或 Chat Completions。Chat 转换支持文本历史、系统指令、基础生成设置和 JSON 对象模式。共用 Responses 的输出与事件构造。用量中的提示 Token、缓存子集、生成 Token 与已报告的推理 Token 按 Chat 口径转换，缓存不重复计费。SSE 实时转发文本，确认 Chat 停止原因及 `[DONE]` 后发送终止事件。

包测试覆盖协议字段、JSON 格式、用量、显式端点限制及无完成状态时的错误。独立应用通过 HTTP 处理普通响应和实时 SSE，独立 Artisan 通过 SDK Responses 客户端调用网关后接入模拟的 Chat-only 上游。已确认首段文本早于上游结束。此批函数工具、附件、JSON Schema 以及反向 Chat 协议转换仍留待后续。

- [DeepSeek Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion/) · [OpenAI Responses](https://developers.openai.com/api/reference/resources/responses/methods/create)

## GW-03 Responses 到 Chat Completions 的客户端工具

带函数 Schema 的 Responses 工具转换为 Chat 的具名函数。支持工具选择、并行选项、历史调用和匹配的文本结果。缺失、重复或未配对的结果在出站前拒绝。普通响应与 SSE 提供原始调用 ID，SSE 可同时转发多个工具的参数增量。只有确认结束且每个工具参数是完整 JSON 对象后才交付最终事件，错误流不产生假的完成帧。

包测试验证多工具交错、无结果的调用历史、参数截断和 JSON 响应。独立 Artisan 调用 SDK，经 Responses 网关和 Chat 上游获得函数调用，使用框架参数验证执行后回传结果，最后得到文本。独立 HTTP 进程确认参数增量先于上游终止到达，随后验证完整参数、调用 ID 与用量。

## GW-03 Chat Completions 到 Anthropic Messages

同一 Anthropic 公开模型可接 Chat Completions。原生 Chat 仍优先，缺少原生支持时才转换。参考固定 Sub2API 的 Responses 中间映射，共用现有的文本、函数历史和 Anthropic 事件解析，再生成 Chat Completion JSON 或 SSE。转换覆盖停止词、生成 Token 参数、工具选择、调用 ID、参数片段、工具结果及可选的末尾用量帧。分段重放及跨协议工具结果必须完整且配对。

包测试验证原生上游认证、正常 JSON、工具回送、真实 SSE 和截断错误。独立 Chat SDK Artisan 调用网关并经 Anthropic 上游执行业务工具；独立 HTTP 验证首段工具参数早于终止到达，且最终用量准确合并缓存 Token。文本与客户端函数以本地真实 TCP 上游验收，附件、签名推理块和供应商实机另列。

- [Sub2API 固定 Chat Completions handler](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler_chat_completions.go) · [Anthropic Messages SSE](https://platform.claude.com/docs/en/build-with-claude/streaming)

## GW-03 Anthropic Messages 到 Chat Completions

Messages 现在可在保留原生优先级的情况下选择 Responses 或 Chat-only 上游。复用既有协议映射处理文本、停止序列、客户端工具、调用 ID 和结果，转换上游缓存与生成用量。Chat 参数流可以交错，Messages 的首个工具块即时发送，其余工具按内容块次序缓冲，前一块关闭后才开始下一块。

独立 Artisan 使用 Anthropic SDK 通过网关调用 Chat-only 上游，验证框架工具执行和同一调用 ID 的回送。独立 HTTP 测试确认首个参数片段早于上游完成，且两组交错参数在下游依次成为完整的 Messages 内容块。包测试验证内容块次序与终止。附件、签名推理和服务端工具待对应协议批次。

## GW-03／GW-05 输入 Token 计数与无版本路径

新增 Responses 输入计数的原生出站和网关入口。Responses 和 Anthropic Messages 两种计数可双向映射，遵循路由目标及 Key 的模型授权，由上游计数。计数请求共用请求限额和账号选择，生成用量字段保持为空。Responses 计数允许内联文本及函数上下文，拒绝旧会话和上游资源引用。无版本路径由 `unversioned_paths` 显式开启，并继续使用原有 HTTP handler。

包测试验证根路径默认关闭、授权目录、跨协议认证及计数、共享限流、外部资源拒绝和用量日志。独立 Artisan 的 SDK 用根地址调用计数，独立 HTTP 验证 OpenAI／Anthropic 的真实协议映射和同一 Key 的限流。本地上游使用真实 TCP 协议模拟。Codex 专用模型目录、自托管状态和 WebSocket 仍待后续。

- [OpenAI Responses 输入计数](https://developers.openai.com/api/reference/resources/responses/subresources/input_tokens/methods/count) · [Sub2API 固定路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/server/routes/gateway.go)

### Bedrock 原生输入计数

SDK 复用 Bedrock Runtime 客户端，通过 `CountTokens(Converse)` 转换 Messages 的系统块、文本、函数定义与历史，以及内联图片和文档。Responses 计数复用到 Messages 的无状态文本和函数转换。网关沿用分组、模型、Key 限额和供应商选择。按固定 Sub2API 的计数入口区分计数与生成用量。OpenRust 保留出站尝试及 AWS 请求 ID。Bedrock 明确免费，跳过组和账号货币预算预占。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 经独立 HTTP 和 Artisan 读取路由、尝试及预算，核对两条协议、无版本路径、Bearer 和 SigV4 签名、原始系统和工具内容、内联文件、无权限及非法输入拒绝。协议服务返回 81、43、55 和明确 0。缺失或负数返回 502，AWS 403 与 429 保留状态并使用脱敏响应。八次实际出站均无生成 Token 和费用，双预算余额各为原有的 0.01 USD。新增回归锁定显式零和缺失区分。17 项网关回归、严格 Clippy 和三语文档通过。正式 AWS 账户及模型区域支持范围待实机验收，依赖 Mantle 的模型由相应入口另行覆盖。

- [AWS Bedrock CountTokens](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_CountTokens.html) · [AWS 计数权限及支持范围](https://docs.aws.amazon.com/bedrock/latest/userguide/count-tokens.html) · [Sub2API 固定计数 handler](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler.go#L2160-L2271)

## GW-04 持久化用量

可选用量账本覆盖实际发起的上游尝试。回退、SSE 正常终止、异常截断和下游取消均有独立进程验收。记录关联请求 ID、密钥标识、分组、入站和上游协议以及供应商实际用量。网关未尝试发送给上游的鉴权、模型和限流拒绝走原有 HTTP 路径。独立 Artisan 支持按请求或托管 Key 分页查询，详见[AI 验收账本](README.md#gw-04-持久化上游用量)。

## GW-04 分组输出配额

在用量账本上通过数据库事务提供共享的输出 Token 日窗口。按上游协议注入生成上限，对回退逐次预留。过量回报停止该分组当日生成。验证了两个实例并发、真实结算、取消、异常、额度不足和数据库恢复三库行为，详见[AI 验收账本](README.md#gw-04-分组输出-token-配额)。待核对金额、其他协议及未结算记录的管理入口。

## GW-03 Responses WebSocket

网关使用相同的 Bearer Key 处理升级，每次 `response.create` 重新鉴权并走同一模型路由和上游协议转换。SSE 事件转换为 WebSocket JSON 帧，单连接当前允许一条活动请求，可传入 `stream_id` 作为事件关联标识。断开时取消上游请求。原生 HTTP Responses 缺失已知终止事件时记作截断，包括只有 `[DONE]` 的响应。

独立 WebSocket 客户端使用实际网关进程，并通过独立 Artisan 撤销 Key。实测未鉴权、别名、首段早于完成、Chat 上游回退、权限撤销、取消、关闭码及 SQLite 用量入账。上游使用本地 TCP 服务。包测试、严格 Clippy、三语构建、两种部署路径的十二组浏览器及完整导航通过。上游 WebSocket、同连接历史续接和并行通道继续单列。

- [OpenAI Responses WebSocket](https://developers.openai.com/api/docs/guides/websocket-mode) · [Sub2API 固定路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/server/routes/gateway.go)

## GW-04 调用方用量

`GET /v1/usage` 接受 Bearer 或 `x-api-key`，在同一数据库查询内汇总当日、累计和最多 90 天的 UTC 日数据。仅使用当前凭证对应的托管 Key ID 或环境 Key 的私有作用域筛选记录。托管 Key 轮换保留历史。环境 Key 换凭证后新建统计范围，迁移前无法确定归属的环境 Key 记录仍可由运维通过 Artisan 查询。调用方响应省略上游名称、请求明细与凭证。

SQLite、MySQL 8.4.13 和 PostgreSQL 18.7 的独立 HTTP 与 Artisan 验证了迁移发布、回退的请求／尝试去重、真实与缺失用量、UTC 日序列、两种鉴权头、仅大小写不同的 Key 名称隔离、凭证轮换、撤销及同名更换。两个包测试、严格 Clippy 与三语页面验收通过。依供应商价格计量、私有账单接口和固定基线的模型区间聚合继续单列。

- [Sub2API 固定 Usage handler](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler.go)

## GW-05 官方 Codex 客户端

固定版 Codex 0.170.0 的已校验发布资产在隔离 Linux ARM64 容器运行。独立的 `codex debug models` 加载 OpenRust 目录，并保留 Codex 自带目录，验证授权别名、显示名称、上下文窗口及排除其他分组的模型。`codex exec` 使用公开别名，经过真实网关和本地 Responses 模拟上游完成流式回复。独立 HTTP 与 Artisan 验证模型目录格式、条件请求与协议路由。

原生 Responses 接受只包含客户端函数或自定义工具的命名空间。公开模型可用 `server_tools: ["web_search"]` 启用上游网页搜索；原生目标仍须通过当前 Key 的分组授权。需要原生 Responses 的工具不进入协议桥接，也不自动重放。HTTP 和包测试验证未开启的搜索、通过命名空间声明供应商资源工具，以及 Chat-only 模型处理原生工具的拒绝。两个包测试、严格 Clippy、三语构建、十二组浏览器及导航通过。该轮覆盖工具声明与文本回复，搜索执行、工具回送及供应商真实账号仍分列。

- [Codex 0.170.0 发布资产](https://github.com/openai/codex/releases/tag/rust-v0.170.0) · [Codex 配置参考](https://developers.openai.com/codex/config-reference)

## GW-03 Responses WebSocket 续接

在同一连接及 `stream_id` 下，可用最新的已完成响应继续文本与客户端函数对话。内存保留最多 512 KiB 和 256 项。续接时将历史转换为内联输入，保留函数的调用 ID 和结果，去除供应商消息条目的资源 ID。只使用当前轮的 `instructions`。每轮仍走既有 HTTP 准入、用量和上游协议适配。

独立 WebSocket、HTTP 和 Artisan 验证原生 Responses 与 Chat-only 上游的续接、同轮工具结果、过期响应 ID、不同连接／模型／通道、超长或不支持的续接输入、权限撤销、断流及记录。两个包测试、严格 Clippy、三语构建、十二组浏览器及导航通过。当前验证串行文本和客户端函数。上游 WebSocket、预热、并行通道、分叉、推理与附件续接单列。

- [Responses WebSocket 续接](https://developers.openai.com/api/docs/guides/websocket-mode#continue-with-incremental-inputs)

## GW-03 共享凭证的附件与音频资源

保留原生 OpenAI、Anthropic 的内联附件线路。请求引用由上游账号持有的文件、持久音频、消息或容器时，在出站前返回协议错误。覆盖 Responses 请求和函数结果的嵌套附件、Chat Completions 文件及续接音频、Messages 文件和容器。Responses 输入计数沿用原有的错误类型。

独立 HTTP 与本地 TCP 上游验证另一个账号的文件 ID 拒绝、OpenAI 两种协议和 Anthropic 内联附件的字节继续出站。两个包测试及严格 Clippy、三语构建、十二组浏览器与完整导航通过。带租户授权的网关资源分配和 Gemini 远程文件引用留待后续验收。

## GW-05 固定 DSH 客户端

使用与固定提交对应的官方 npm 包 `@deepseek-ai/dsh@0.2.3-alpha.2`，在隔离 Linux ARM64 容器和 Node.js 24.22.3 中运行。用不同具名连接分别配置 Chat Completions、Responses、Anthropic Messages 与 Gemini 原生协议。四次独立 headless 调用都通过实际网关获取流式响应，真实 HTTP 协议模拟服务核实原生鉴权、模型映射和用量。

实际 DSH Web 与 Chrome 验证从公开模型目录发现授权别名、排除未授权别名、在自定义连接中保存发现的 Chat 模型、手填 Responses 别名、用模型选择器选择后调用并显示本轮独立回复。运行时使用合成业务输入和隔离凭证。三语文档分别列出四种协议的 URL。供应商实机及其他 GW-05 入口继续验收。

- [固定 DSH 自定义模型说明](https://github.com/deepseek-ai/deepseek-harness/blob/4b399c39a6d1479cd84ca591b4d7c88e24d38a96/docs/user/guide/providers.md) · [DSH 固定版本发布](https://github.com/deepseek-ai/deepseek-harness/releases/tag/dsh-v0.2.3-alpha.2)

## GW-03 Responses WebSocket 并行通道和分叉

一个连接最多运行四个通道，每个活动通道使用独立的 `stream_id`。事件按所属通道返回。同一连接可从仍在内存中的响应分出新通道，来源通道可继续。每条连接最多保存四份有界历史，关闭时取消全部活动响应。继续沿用普通 HTTP 的授权、路由、配额及用量。

独立 WebSocket 与 Artisan 实测原生慢流尚未结束时另一 Chat 通道先完成、同通道重复请求拒绝、分叉后来源继续、函数结果、第五个活动通道拒绝、关闭连接后四个上游尝试全部取消入账。包测试、严格 Clippy、三语构建、十二组浏览器及导航通过。上游 WebSocket、预热、实时转向和其他类型的续接单列。

- [Responses WebSocket 并行与分叉](https://developers.openai.com/api/docs/guides/websocket-mode#fork-a-conversation-onto-a-new-stream)

## GW-03 xAI 原生工具授权

公开模型可在 `server_tools` 启用 `web_search`、`code_interpreter` 与 `file_search`。集合搜索需要配置 `file_search_collections`，逐项精确匹配请求的集合 ID。代码执行只允许新沙箱。选路同时验证下游 Key、分组可用连接和供应商支持的工具。相关工具请求及工具续步沿用已有防重放约束。货币费用上界未知时沿用硬预算的拒绝或监测设置。

真实 HTTP、独立 Artisan 和本地原生协议服务验证默认关闭、非法静态配置、未授权集合及旧容器、分组隔离、兼容连接选择、原生 JSON 与 SSE、OpenAI 搜索兼容及 SQLite 计费。拒绝请求均未出站。监测模式保留上游实际费用。网关和 SDK 共 24 项回归、两包严格 Clippy 及三语站点检查通过。供应商官方账户的实际工具执行仍待验收。

- [xAI 集合搜索](https://docs.x.ai/developers/tools/collections-search) · [xAI 工具费用](https://docs.x.ai/developers/pricing)

## GW-05 Codex 专用前缀

`/backend-api/codex` 复用主 Responses 路由的鉴权、模型授权和用量记账。同一个网关 Key 可以读取专用模型目录、创建 Responses、读取输入 Token 数量和建立 Responses WebSocket。官方 Codex 0.170.0 的 `debug models` 和 `exec` 分别在隔离容器中重新加载该目录和完成流式生成。独立 HTTP、WebSocket 和 Artisan 覆盖计数、鉴权、分组范围与路径边界。搜索与 Realtime Calls 仍待后续验收。

- [Sub2API 固定基线路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/server/routes/gateway.go#L408)

## GW-04 每个 Key 的模型用量

`GET /v1/usage` 的 `model_stats` 汇总当前 Key 最近 30 天的公开模型用量。`start_date` 和 `end_date` 以 UTC 日期指定包含首尾两天的范围。缺失或不可解析的日期使用相应默认值。`days` 仍控制日序列，累计字段仍覆盖完整账本。累计、日序列和模型汇总共用一次数据库语句快照。模型名按精确大小写区分。

SQLite、MySQL 8.4.13 及 PostgreSQL 18.7 均通过真实 HTTP、Artisan 和本地协议上游验证。用例涵盖未报告用量、一次请求两次尝试、起止同日、旧时段无结果、大小写别名、其他 Key 隔离、轮换后历史归属、撤销和同名静态 Key 换凭证。三语文档构建与根路径及 `/docs/` 导航验收通过。

- [固定 Sub2API Usage 与日期范围实现](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_handler.go#L1616)

## GW-03 同步图片生成与编辑

`/v1/images/generations` 与 `/v1/images/edits` 复用公开模型映射、Key 分组、账号容量和持久用量。图片端点需在模型发布时显式配置，同时支持可选的无版本路径。生成接收 JSON，编辑接收 JSON 或 multipart 多图和遮罩。解析表单时保留提示词原始空格，向上游转发实际识别的图片 MIME。JSON 文件 ID 在出站前拒绝，以免跨 Key 引用共享资源。图片请求只派发一次，文本输出 Token 配额仍按支持的端点计量。

实际 Artisan 用 SDK 经 OpenRust 网关完成生成与编辑。独立 HTTP 验证相同图片原始字节、多图顺序、遮罩、SSE 完成与截断、否定鉴权、共享文件引用、公开模型名、503 后不自动重放及 SQLite 用量。异步图片任务、批量任务、其他媒体与供应商实机仍待对应批次。

- [固定 Sub2API 图片处理器](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/openai_images.go) · [OpenAI 图片生成](https://developers.openai.com/api/reference/resources/images/methods/generate) · [OpenAI 图片编辑](https://developers.openai.com/api/reference/resources/images/methods/edit)

## GW-03 Bedrock Chat Completions

`chat/completions` 发布端点将普通文本映射到 AWS Converse，SSE 映射到 ConverseStream。复用 SDK 的 AWS 凭证链、原生事件解码及用量换算。保留前置 system／developer 和消息内的文本顺序，映射最大输出 Token、temperature、top-p 与停止字符串。返回公开模型名及可见推理。跳过供应商签名与不透明推理。网关在用量帧及 `[DONE]` 发出前保存最终用量。明确请求 `stream_options.include_usage` 时输出末尾用量帧，并在此前各数据帧中将 `usage` 设为 null。

独立 Artisan 实测 SDK 直连与 SDK 经网关。外部 HTTP 通过本地 AWS 原生协议服务验证 JSON、首段文本早于终帧、流式结束原因、三种用量选项、身份隔离、结构化及工具请求出站前拒绝、AWS 403 和截断无 `[DONE]`。SQLite 核对七次网关尝试的原生操作与 AWS 请求 ID。成功样例按 12 个基础输入、2 个缓存读取、3 个缓存写入和 4 个输出 Token 记录，隐藏用量帧时仍保存最终状态。以独立缓存费率复算，每个成功样例的上游成本为 0.03 USD、下游应收为 0.06 USD。截断和 403 的成本待核对，应收为零。31 项 SDK 与网关回归、严格 Clippy、三语站点和根路径及 /docs/ 导航通过。客户端函数、原生 JSON Schema 和内联媒体见下文。真实 AWS 账户验收继续单列。

- [AWS Converse 与 ConverseStream](https://docs.aws.amazon.com/bedrock/latest/userguide/conversation-inference.html) · [AWS 推理参数](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_InferenceConfiguration.html) · [OpenAI Chat Completions](https://developers.openai.com/api/reference/resources/chat)

## GW-03 Bedrock Chat 客户端函数

Chat 函数定义、对象 Schema、`strict` 和工具选择复用 SDK 的 Converse 工具构造。模型支持相应选项时可用自动、必选及指定工具。在后续请求保留启用的工具配置，用原调用 ID 回送结果。同一助手轮的多个结果合并为一条 AWS user 消息。缺失、重复或不匹配的结果在出站前拒绝。网关核对返回的工具名称、调用 ID、选择策略及并行限制。SSE 在原生终帧验证参数和用量后交付工具。Chat Completions 签名或不透明推理的工具回放见下节。

本地 AWS 协议服务通过独立 Artisan 验证 SDK 直连与 SDK 经网关，两轮均保留 `stock:one`，总量为 34 输入和 8 输出 Token。外部 HTTP 验证 JSON 和 SSE 两轮、两个并行结果逆序回送、指定工具、必选、禁用、非法历史和签名不外泄。模拟工具调用的九次成功均各结算成本 0.03 USD、应收 0.06 USD。四次本地转换失败仍按 AWS 最终用量各记成本 0.03 USD，应收为零。缺最终元数据的截断流不发工具或 `[DONE]`，成本待核对。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均通过这组 HTTP 与 Artisan 验收。31 项回归、严格 Clippy 和三语站点通过。

- [AWS 客户端工具调用](https://docs.aws.amazon.com/bedrock/latest/userguide/tool-use-client-side.html) · [AWS ToolUseBlock](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_ToolUseBlock.html) · [固定 Laravel BedrockTextGateway](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Gateway/Bedrock/BedrockTextGateway.php)

## GW-03 Bedrock Chat 签名工具回放

普通助手消息及 SSE 增量可携带 `openrust_reasoning_replay`。使用应用加密服务保存原始 AWS 助手内容块，可见推理仍通过 `reasoning_content` 发出。状态绑定实际下游凭证、公开名、上游目标和显式 AWS 凭证，校验原始历史和助手文本及工具调用，有效期一小时。回放在预占和出站前验证，随后按原顺序恢复签名推理、脱敏块和工具调用。SDK 工具步骤自动保留此字段，跨进程共享应用密钥即可恢复。该批覆盖显式凭证的直接 Chat Completions 工具回放。Responses、纯文本轮次及默认凭证链见下文。

独立 Artisan 验证 SDK 原生直连，以及 SDK 经网关的普通和流式两轮工具调用。外部 HTTP 通过本地 AWS 协议服务验证原始签名和脱敏字节回送，两轮运用原始调用 ID。SSE 取回的状态可由另一工作进程接续。过期状态、修改密文、历史、助手文本、换下游 Key、轮换 Key、换公开别名或换 AWS 凭证均在出站前拒绝。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各核对八次签名网关尝试的用量及成本。31 项回归、严格 Clippy 和三语站点通过。正式 AWS 账户验收继续单列。

- [AWS 签名推理回放](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_ReasoningTextBlock.html) · [Converse 会话要求](https://docs.aws.amazon.com/bedrock/latest/userguide/conversation-inference.html)

## GW-03 Bedrock Responses 签名回放

无状态 Responses 使用标准 `reasoning.encrypted_content` 保存已完成轮次的加密状态，普通响应和 SSE 输出推理条目。SSE 维持消息、推理项与函数项的索引和完成顺序。支持携带 `include: ["reasoning.encrypted_content"]`。客户端依次回送此前输入、输出、工具结果或下一条用户消息。Chat Completions 也可为正常结束的纯文本签名轮次返回原有加密字段。复用应用加密、时效、历史摘要和凭证绑定，加上入站协议隔离。包含 Bedrock 加密状态的 Responses 请求只会匹配支持该状态的 Bedrock 目标，携带状态的续轮不触发故障重放。

用本地 AWS 协议服务、独立 Artisan 和真实 HTTP 验证 Responses SDK 与外部客户端的普通及流式工具续轮，覆盖带可见文本的工具步骤、原始 `stock:one` 调用 ID、原始签名及脱敏块、纯文本续聊和跨进程恢复。更换 Key、别名、供应商或入站协议在出站前被拒绝。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各核对新增的八次工具及四次文本尝试。模拟费率下每笔用量为 17 输入和 4 输出 Token，成本 0.03 USD，应收 0.06 USD。31 项回归、严格 Clippy 和三语站点通过。默认凭证链见下节，正式 AWS 账户实测仍待验收。

- [OpenAI 无状态推理回放](https://developers.openai.com/api/docs/guides/reasoning#preserve-reasoning-without-stored-responses) · [AWS Converse 签名要求](https://docs.aws.amazon.com/bedrock/latest/userguide/conversation-inference.html)

## GW-03 Bedrock 默认凭证链签名回放

默认凭证链用同一份 SigV4 凭证快照执行 STS `GetCallerIdentity` 和 Bedrock 推理。凭证指纹相同时复用连接内的身份结果。角色范围包含 AWS 分区、账户、角色名和 IAM 角色唯一 ID，临时会话名变化不改变作用域。身份不可核实时拒绝带签名状态的续轮，普通无状态文本继续按该凭证发出。显式 Bearer 与 SigV4 沿用已有回放范围。配置为空的 Bearer 环境变量时，IAM 路径优先使用 SigV4，推理和目录采用相同选择规则。

真实 HTTP 与本地 STS／Bedrock 协议服务核对两次 AWS 签名使用的凭证。模拟共享 AWS 配置的 `credential_process`，在同一网关进程内换发临时凭证。Responses 签名工具回放和 Chat 纯文本回放均接续同一角色的新会话。同名但唯一 ID 不同的角色、其他账户及 STS 故障在 Bedrock 出站前拒绝续轮。独立 Artisan 验证两个目录调用使用 SigV4。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各核对五次新增推理尝试的用量和账务。模拟费率下每笔 17 输入、4 输出 Token，成本 0.03 USD，应收 0.06 USD。31 项回归、严格 Clippy 与三语站点通过。正式 AWS 账户仍待验。SDK 独立进程的审批与历史回放见[默认凭证链的审批与历史回放](README.md#默认凭证链的审批与历史回放)。

- [AWS STS GetCallerIdentity](https://docs.aws.amazon.com/STS/latest/APIReference/API_GetCallerIdentity.html) · [AWS IAM 角色唯一 ID](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html) · [AWS Rust 凭证链](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/credproviders.html)

## GW-03 Bedrock 网关结构化输出

Chat Completions 的 `response_format.json_schema` 与 Responses 的 `text.format` 映射到 AWS `outputConfig.textFormat`。原样交付对象 Schema、名称与说明，供应商按其支持的 Schema 子集执行约束。网关为首次编译和生成提供 120 秒，支持同一请求声明客户端函数。格式为 `json_object` 且缺少明确 Schema 时拒绝。JSON 与 SSE 的正常停止需要产出可解析的对象，输出达到 Token 上限时保留部分文本及原停止原因。Responses 保留请求的格式元数据及缓存读取和写入用量。

本地 AWS 协议服务以独立 Artisan 验证 SDK 原生 `with_schema` 和 SDK 经网关，独立 HTTP 覆盖 Chat Completions／Responses 的 JSON 与 SSE、嵌套对象、与工具并用、首段 JSON 早于 AWS 终帧、预检、上游 400、达到 Token 上限，以及格式错误时不发成功终帧。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各核对 12 次网关尝试。AWS 报告 17 输入和 4 输出 Token 的成功样例按模拟费率成本 0.03 USD、应收 0.06 USD。四次格式错误仍按已确认用量入账，应收为零。31 项回归、严格 Clippy 和三语站点检查通过。原生 SDK 继续按固定 Laravel 的工具或 JSON 指令策略处理 `with_schema`。正式 AWS 账户及模型验收仍待执行。

- [AWS 原生结构化输出](https://docs.aws.amazon.com/bedrock/latest/userguide/structured-output.html) · [AWS JsonSchemaDefinition](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_JsonSchemaDefinition.html) · [OpenAI 结构化输出](https://developers.openai.com/api/docs/guides/structured-outputs)

## GW-03 Bedrock 网关内联媒体

Chat Completions 的 `image_url`、`file` 和 Responses 的 `input_image`、`input_file` 保留文本和媒体块顺序。复用 SDK 的附件 MIME 校验与 Converse 内容块转换。支持 PNG、JPEG、WebP、PDF、UTF-8 文本和 Markdown，文档接受文件名加原始 Base64 或 Base64 data URL。每条文档消息需要文本提示，允许仅含图片的消息。按 AWS 限制和 SDK 附件预算检查大小、宽高、总数量和文档数量。远程地址、共享文件 ID、未支持的细节档位及 MIME 不一致在出站前拒绝。

用独立 Artisan、真实 HTTP 和本地 AWS 协议服务验收 SDK 直连、SDK 经网关及外部访问。Chat Completions／Responses 分别验证 JSON 与 SSE、图片文档原始字节、文本间隔、单图输入及非法输入；另验证带图片和 PDF 的两轮工具调用回送同样的原始附件。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各核对八次媒体网关尝试。每笔记录 17 输入和 4 输出 Token，按模拟费率结算成本 0.03 USD、应收 0.06 USD。账本没有保存附件数据。31 项 SDK 与网关回归、严格 Clippy 和三语站点通过。AWS 实机的模型媒体能力待验。

- [AWS Converse 消息约束](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Message.html) · [AWS 文档块](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_DocumentBlock.html) · [OpenAI Chat Completions](https://developers.openai.com/api/reference/resources/chat)

## GW-03 Bedrock 文生图兼容入口

通过 `images/generations` 发布 Bedrock 文生图模型。Nova Canvas 和 Titan 的尺寸、质量复用 SDK 映射，在一次 AWS `InvokeModel` 中请求一至五张图片。Stability 按比例调用单图。入口返回 Base64 PNG，出站前拒绝 SSE、编辑、URL 或其他无等价映射的参数。网关账本保留实际上游模型、操作及 AWS 请求 ID。Token 缺失时保持未知，按图计费见下节。

本地 AWS 协议服务通过 SDK 直连和独立 Artisan 经网关生成同一 PNG 并从私有 Storage 逐字节回读。外部 HTTP 验证 Nova 的公开模型授权、Titan 双图与对应尺寸、Stability 模型 ARN 路径编码、无效响应、返回图片超出请求数量及供应商拒绝。SQLite 核对六次网关尝试的请求 ID 和状态，前置拒绝未出站。SDK 图片与八项网关回归、严格 Clippy 和三语站点通过。

- [OpenAI 图片生成](https://developers.openai.com/api/reference/resources/images/methods/generate) · [Nova Canvas 参数](https://docs.aws.amazon.com/nova/latest/userguide/image-gen-req-resp-structure.html) · [Titan Image Generator 参数](https://docs.aws.amazon.com/bedrock/latest/userguide/model-parameters-titan-image.html)

## GW-04 Bedrock 图片计费

迁移新增可空的 `output_images`，普通 Bedrock 图片响应只登记上游返回且通过驱动验证的图片数。Artisan 计费页和当前 Key 的 `/v1/usage` 显示已知图片数量，历史行及未确认数量保留未知。每图费率由部署者按模型配置并保存快照，使用现有分组倍率、账务精度及预算事务。预占按请求 `n`，结算按实际数量。失败尝试不产生下游应收，已出站但无可信数量的上游成本仍待核对。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过实际 HTTP、独立 Artisan 和本地 AWS 协议服务验收迁移发布、图片数量、费率快照、并发预占及 Key 用量汇总。模拟费率下，Titan 双图成本 0.06 USD、应收 0.14 USD。分组倍率 0.5 的样例请求两张并先预占 0.12 USD，实际返回一张后应收 0.06 USD。无效或被拒响应保留成本待核对。复验 Titan 批量向量、网关进程强制终止后的资金恢复、SDK 图片及网关回归。

图片费率按实际 Bedrock 参数查找 `image_tiers`。Nova Canvas／Titan 使用尺寸和质量，Stability 使用宽高比与默认质量。预占和结算共用选定的费率，持久保存价格版本、`image_tier` 和每图单价。没有匹配档位时回退至 `image`，无回退时保留待定金额，无法按该方向预占的硬预算拒绝出站。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 的本地协议样例验证 OpenAI 竖图请求使用 `768x1152/premium`，Titan 横图使用 `1152x768/standard`，Stability 使用 `3:2/default`。模拟费率下，Titan 双图上游成本为 0.07 USD。分组倍率样例预占 0.10 USD，实际返回一张后扣减 0.05 USD。还复验了 SDK 图片、向量批量、并发预算、中断恢复、网关回归、严格 Clippy 和三语站点。真实 AWS 账户的账单对账待验。Nova Canvas 的历史协议样例来自本地服务，现行 AWS 模型卡标记其已退役，公开示例改用 Stable Image Core。

- [AWS Titan 输出计价档位](https://docs.aws.amazon.com/bedrock/latest/userguide/titan-image-models.html) · [Nova Canvas 生命周期](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-amazon-nova-canvas.html) · [Stable Image Core 模型卡](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-stability-ai-stable-image-core-1-0.html)

## GW-03／GW-04 Gemini 异步批任务

内联批量生成和异步向量通过显式模型端点启用，复用 [Gemini 异步批任务 SDK](README.md#gemini-异步批任务-sdk)。提交登记一次出站尝试，保存下游 Key 归属、公开模型、原上游连接范围和实际任务名，客户端收到本地 ID。列表先按本地归属分页，再到对应连接读取状态。读取、取消和删除要求当前 Key 仍获准使用原模型和原上游范围。创建结果不确定时保留原尝试并停止选号。删除前先读取可用结果，成功删除后保留账务及本地删除标记。

结算复用提交时的费率快照。终态结果或必需计数迟到时保留待结算；证据齐全后用同一个数据库事务更新用量、两侧费用和预占，并发查询只结算一次。成功项形成应收，失败项的上游成本保留待核对。向量和文本生成批次按每项配置的输入上界预占货币预算。文本生成取分组预算、账号预算及分组配额的最严格单项限制，按候选数换算每个候选的输出上限，再汇总全批次。推理配置随原请求转发。文件型输入继续依赖私有 Storage 与网关文件归属。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 经真实 HTTP、独立 Artisan 和本地 Gemini 协议服务验证迁移发布、同名上游任务的账号隔离、公开模型重写、重启续查、托管 Key 轮换与撤销、上游凭证变更、分页、取消、删除、共享文件与文件搜索拒绝，以及备用连接存在时不重发提交。验证 SDK 经网关读取结果、终态先到及并发结算。生成样本一项成功一项失败，共确认 8 输入、3 候选输出和 2 推理 Token，应收 0.036 USD，成本待核对。向量批次预占 0.02 USD 成本和 0.04 USD 应收，结算 0.007 和 0.014 USD。`/v1/usage` 验证总量包含 Gemini 推理 Token。15 项回归、严格 Clippy 及三语文档通过。正式 Gemini 账号待实测。

同三库的独立 HTTP 与 Artisan 核对两条并发提交只放行一条。两项分别按两候选各 6 Token 和单候选 3 Token 预占，共 15 Token，预占成本 0.05 USD 和应收 0.10 USD。结果确认 13 输入、5 候选输出和 2 推理 Token，分别结算 0.027 USD 和 0.054 USD，配额计入 7 Token。取消且数据不足时保留输出配额和待核成本预占，释放应收预占。模拟上游超报 16 Token 时记录实际用量，停止该窗口继续准入。无效候选数、冲突字段和多模态输出在受限线路出站前拒绝。

- [Gemini GenerationConfig](https://ai.google.dev/api/generate-content#GenerationConfig) · [Gemini 推理 Token](https://aistudio.google.com/docs/thinking#token-limits)

### Gemini 批任务结果下载

上游提供 JSONL 结果时，网关在 `response.responsesFile` 公开绑定本地批任务的文件名。下载先按当前 Key 查任务，再验证当前模型授权和原上游账号范围，由已授权的上游任务响应解析真实文件名。上游文件标识、错误详情和响应头不随下载转发。逐行解析、保留条目 Key、重写模型名，并在最后一条发出前登记已确认的用量。截断或条目数不符时中断下载，原账目继续待结算。删除带结果文件的批任务前先读取结果证据。单行限制 64 MiB。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 使用独立 HTTP、Artisan 和本地 Gemini 协议服务验收网关下载。模拟内联批次返回文件结果，验证按分组隔离、原始文件名拒绝、JSONL 分块、没有换行符的最终行、截断重试、两进程并发下载和 SDK 下载。样本一项成功、一项状态错误，成功项仍结算 8 输入、3 候选、2 推理 Token，应收 0.036 USD，上游成本待核对。删除后原本地文件名不可再下载。正式账号验收仍待完成。

- [Gemini 批任务结果与 REST 下载](https://aistudio.google.com/docs/batch-api#batch-job-status)

### Gemini 文件型批任务提交

新增 Gemini JSONL 暂存上传，支持原生 resumable 会话启动、状态查询和单次完整文件提交。输入字节保存到私有 `local` 磁盘，数据库持有 Key 归属、对象路径、字节长度及 SHA-256。提交按同一 Key 查找本地文件，校验内容和每行的模型与资源，沿用内联批次的预算及输出配额处理，再将改写的 JSONL 上传到实际连接。客户端收到的任务、文件和上游凭证范围继续隔离。重复提交相同上传内容返回原文件，不同内容返回冲突。删除先标记本地文件不可再用，再清理文件对象，清理失败可重试。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过独立 Artisan SDK 上传、HTTP 文件型提交和本地 Gemini 协议服务验证文件仅存私有磁盘、跨 Key 拒绝、跨进程提交、上传重试和冲突、摘要不符拒绝、文件删除以及结果下载与结算。带硬预算和输出配额的两项样例在原生上传的 JSONL 中各限制 10 个输出 Token，应收预占 0.12 USD。首批采用单次 `upload, finalize`。续传和文件上限见下文，正式 Gemini 账号待实测。

- [Gemini 批任务 JSONL 文件上传](https://aistudio.google.com/docs/batch-api#input-file)

### Gemini 批文件分片续传

沿用 Gemini 的 `upload`、`query`、`upload, finalize` 和空正文 `finalize`。每次写入创建独立私有对象，数据库在同一次条件更新登记分片位置、对象引用、SHA-256 和已确认偏移。相同分片并发重发只登记一次，不同内容或不连续偏移返回当前进度与冲突。任务只能引用已经完成的文件，读取时按顺序验证每片长度和摘要。兼容已经发布的单对象记录。单次请求上限 8 MiB，每个文件上限 32 MiB、最多 256 片。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 以独立进程和本地 Gemini 协议服务验证 9 MiB JSONL 跨进程续传、同片并发重发、跨 Key 与错误偏移拒绝、空正文完成及其重试、旧单对象读取、分片损坏拒绝、目录删除和结果结算。带硬预算的样本验证重写后的文件仍发往同一上游连接，并预占 0.12 USD 应收。更大的文件和正式 Gemini 账号仍待验。

- [Google Go SDK 的 Gemini 分片上传实现](https://github.com/googleapis/go-genai/blob/main/api_client.go)

### Gemini 暂存批文件清理

上传会话自启动起保留 48 小时，已完成文件自首次确认上传起重新计时。到期后禁止查询、完成上传及提交批任务，升级前存在的文件从迁移起保留一个完整窗口。沿用网关用量连接的 Artisan 命令 `ai:gateway:batch-files:prune` 按过期索引有界选取候选，额外留 5 分钟收尾。该命令清理所属私有目录后删除对应记录，同一目录内未发布的文件一并清理。存储清理失败保留记录并返回非零退出码，`--pretend` 只统计。部署者可将命令登记到现有调度器。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 验收迁移回滚再升级、旧数据、过期拒绝、活跃文件保留、已完成和未完成会话清理、孤立暂存对象、清理失败重试及实际 `schedule:run`。Artisan 帮助通过现有语言资源覆盖 18 种运行时语言。三语开发文档给出调度用法。

- [Gemini 文件保留时限](https://aistudio.google.com/docs/files) · [Laravel 调度 Artisan 命令](https://laravel.com/framework/docs/12.x/scheduling#scheduling-artisan-commands)

## GW-03／GW-04 Gemini 网关显式上下文缓存

公开模型显式启用 `gemini_cached_contents` 后，网关先登记本地资源，再用获准的 Gemini 连接创建缓存。数据库保存归属、公开模型、上游范围及资源映射。未确认创建保留登记，外部列表和读取只返回已绑定资源。硬预算要求拒绝未知费用时，网关在出站前拦截创建和有效期更新。普通及流式生成在发送前校验当前 Key、公开模型、端点授权及原连接凭证。只在该连接内改写缓存名称，失败后不向备用账号提交同一请求。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 经真实 HTTP、独立 Artisan SDK 和本地 Gemini 协议服务验证迁移发布与回滚、迁移缺失时无上游请求、两个账号同名资源、分页、两种过期时间更新、跨 Key／模型阻断、凭证轮换失效、SDK 经网关创建、上游过期后的读取与删除、原账号出错不回退、普通／SSE 生成引用。各成功生成确认 3005 输入、3000 缓存输入和 2 输出 Token，模拟费率下成本为 0.309 USD、应收为 1.518 USD。缓存创建、持有费用及正式 Gemini 账号对账待后续验收。

- [Gemini 缓存 REST 参考](https://aistudio.google.com/api-reference/caching) · [Gemini 显式缓存 SDK](README.md#gemini-显式上下文缓存-sdk)

## GW-03 Gemini JSON 数组与 SSE

`streamGenerateContent` 使用 Google 的 `alt` 协商，缺省或 `json` 逐项返回 JSON 数组，`sse` 返回 SSE。复用原生 Gemini SSE 上游、模型授权、用量观察和结算。输出保留候选、函数调用及思考签名，将模型版本改为公开名。多候选流按索引确认全部结束或收到提示词拦截，待最终用量持久化后交付带终值的响应。缺少终态、传输中断或格式错误时按协商格式交付脱敏错误。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 经独立 HTTP、Artisan 和本地 Gemini 协议服务核对首段早于上游结束、UTF-8 跨数据块、`alt=json`、`alt=sse`、`$alt`、分组拒绝、截断、取消、伪终帧、错误响应及私有上游标识隔离。另一个 Artisan 进程在第一候选结束后读到未结算记录，并在最终响应块到达时即能从账本读到结算结果。每个成功样本报告 8 输入、3 候选输出、2 推理 Token，模拟单价下成本 0.018 USD、应收 0.036 USD。18 项网关回归、严格 Clippy 与三语文档通过。固定 DSH 0.2.3-alpha.2 的 Gemini SSE 实际调用通过，同步复验 Chat Completions、Responses 和 Anthropic Messages。正式 Gemini 账户仍待实测。

- [Google API 系统参数](https://cloud.google.com/apis/docs/system-parameters) · [Gemini 流式生成与候选](https://ai.google.dev/api/generate-content#method:-models.streamgeneratecontent) · [固定 Sub2API Gemini 路由](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gemini_v1beta_handler.go)

## GW-03 Gemini 原生向量网关

配置模型的 `embedContent` 和 `batchEmbedContents` 后，`/v1beta/models` 仅向获准 Key 显示当前模型支持的动作。正文可省略模型，或与路径中的公开别名一致。网关替换单条和批量项的模型为真实上游资源名，保持向量结构及 Token 用量。Gemini 同协议入口拒绝共享凭证的 `fileData` 与缓存资源，函数参数中同名的普通应用字段仍可转发。

独立 Artisan 通过 SDK 和网关完成 Gemini 批量向量，真实 HTTP 核对单条、批量、维度、实际模型重写、鉴权、其他分组、空批次、别名冲突及资源隔离。SQLite 账本记录三个向量请求并保留上游报告的 Token。覆盖的是同步原生动作，异步批次与跨协议转换另行验收。

- [Gemini 原生向量接口](https://ai.google.dev/api/embeddings)

## GW-03 Bedrock 向量兼容入口

显式发布端点 `embeddings` 的 Bedrock 模型可通过 `POST /v1/embeddings` 调用。Cohere Embed 文本批次对应一次 AWS `InvokeModel`。Titan V2 每条输入对应一次 AWS 调用，网关以同一逻辑请求归集，按输入顺序返回索引和向量。支持浮点数组和小端 float32 的 Base64 表示，并由授权规则决定下游可见的公开名。Token 数组、非法维数和超出协议上限的批次在预占和出站前拒绝。

账本记录上游模型 ID、`model/{modelId}/invoke` 和 AWS 回报的请求 ID。用量来自 Titan 正文或 Cohere 响应头。缺少用量时响应省略 `usage`，账本保持未知。AWS 响应未报告实际模型时，账本保留选路目标，回报模型字段为空。

本地 AWS 协议服务覆盖 SDK 直连、独立 Artisan 经网关、外部真实 HTTP 三条链路。验证别名、目录授权、Key 隔离、出站前拒绝、浮点数组和 Base64、已知与未知用量、403 请求证据。SQLite 中单条样例的五条网关尝试都有协议服务回报的请求 ID，SDK 直连和前置拒绝不产生网关记录。Titan V2 批量验收见下节。正式 AWS 账户仍待验收。

- [OpenAI Embeddings 请求和返回](https://developers.openai.com/api/reference/python/resources/embeddings/methods/create) · [AWS 向量模型和维度](https://docs.aws.amazon.com/bedrock/latest/userguide/knowledge-base-supported.html) · [AWS InvokeModel](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_InvokeModel.html)

## GW-04 向量输入计费与预算

`/v1/embeddings`、Gemini `embedContent` 和 `batchEmbedContents` 沿用现有费率快照、出站记录及预算事务。用量完整的向量尝试只按输入 Token 计价。明确零用量按零结算，缺用量保留待核对。配置预算时，预占上限按整次上游请求的 `input_token_limit` 和费率确定，无需为输出 Token 预留金额。无法得出上界时沿用拒绝或监测策略。

通过独立 HTTP、Artisan 和 SQLite 核对本地协议样例。按模拟费率，Bedrock Cohere 13 Token 结算成本 0.013 USD、应收 0.026 USD，Titan 7 Token 为 0.007 和 0.014 USD，Gemini 单条 9 Token 为 0.0045 和 0.0135 USD，批量 18 Token 为 0.009 和 0.027 USD。未知用量保留金额与预占待核对。缺上界时拒绝且未出站。并发时分组先预占 0.04 USD，再拒绝第二笔。首笔完成后扣减 0.026 USD，释放 0.014 USD，账号成本按独立费率结算。

13 项网关回归、SDK 与网关严格 Clippy、三语站点通过。另用独立 HTTP 和 Artisan 复验文本生成的分组与账号双重预占、跨币种选路、拒绝回滚、未知费用及资金恢复。

- [OpenAI Embeddings 用量字段](https://developers.openai.com/api/reference/python/resources/embeddings/methods/create) · [AWS Titan V2 输入上限与尺寸](https://docs.aws.amazon.com/bedrock/latest/userguide/titan-embedding-models.html)

Titan V2 批量在同一事务登记各条尝试与整批预占。每条调用保存 AWS 请求 ID 和用量证据，整批成功时原子结算所有应收。某条失败时停止后续调用，已确认用量的成功条目结算上游成本，所有条目的下游应收为零。尚未出站的条目释放预占，已出站的未知成本保持待核对。

SQLite 与本地 AWS 协议服务验证三条调用链、两条向量顺序和合计 18 Token、逐条金额 0.014 与 0.022 USD、部分失败保留 0.009 USD 成本且应收为零。校验第三条保持未出站，三条余额不足时整笔登记回滚。并发样例先预占整批 0.08 USD，拒绝另一笔后按实际应收扣减 0.018 USD。13 项网关回归、文本预算复验、两包严格 Clippy 和三语文档检查通过。

新登记的批次在尝试行保留批次待结算标记。强制停止网关后，用独立 Artisan 从分组和账号并发恢复同一批次。事务先认领整批，再核对最近出站时间，按原费率快照结算。SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 的两次并发恢复均分别处理 3 条和 0 条。已确认条目结算 0.009 USD，上游结果未知条目保留 0.02 USD，未出站条目释放预占，整批释放 0.12 USD 下游预占且应收为零。重复执行均返回零。PostgreSQL 与 MySQL 使用仓库验收入口创建独立容器，并复验 SDK 直连、经网关 SDK、外部 HTTP、批量顺序、部分失败、并发预算和 Gemini 向量计费。旧版中断时未登记批次标记的历史记录仍沿用原对账流程。正式 AWS 账户仍待验收。

## GW-04 最终流用量先入账

启用持久化用量时，网关先合并同一次上游尝试已收到的计数，再持久化完成结果，最后向下游交付最终帧。普通响应、转换流与原生流共用该结算顺序。原生 Responses 完成或截断帧、Messages 的 `message_stop`、Chat 的末尾用量块及 Gemini 的终态和最终总量触发即时结算。流结束后会继续处理缺失终值的记录。持久化失败时交付协议错误，保留待完成记录；同一尝试只能更新一次既有结算记录。

独立 HTTP 网关通过本地 TCP 分别读取四种协议的最终帧，上游仍保持连接。独立 Artisan 立即读到已结算用量。杀掉并重启网关后 SQLite 与原生 PostgreSQL 18.7 的记录一致。SQLite 隔离故障样本在首帧后使账本不可用，网关返回协议错误。包测试、严格 Clippy 与三语文档验收通过。用量证据状态、晚到补录和冲突对账另行验收。

- [OpenAI Chat 流用量](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events) · [OpenAI Responses 流式事件](https://developers.openai.com/api/reference/resources/responses/streaming-events) · [Anthropic Messages 流](https://platform.claude.com/docs/en/build-with-claude/streaming)

## GW-04 出站用量证据状态

尝试记录在网络调用前保存出站标记。该标记表示网关开始发送流程，上游是否收到或处理由后续证据确认。Artisan 用 `usage_state` 区分未出站、未知、部分、最终和旧记录，明确零值保留。响应头的上游请求 ID 和响应体报告的模型在有可信字段时写入；外部请求的同名字段不作为供应商证据。流式新计数在转发前保存，进程异常退出后保留最近逐字段合并的部分用量。最终结果仍在同一尝试内只结算一次。

独立 SQLite 与 PostgreSQL 18.7 上使用真实 HTTP 和 Artisan，检查出站记录、四协议的终值、零值、没有计数的完成响应、上游 ID 与报告模型、终帧前退出并重启，以及迁移保留的历史计数。SQLite 故障注入在出站标记失败时验证上游无请求。包测试、严格 Clippy 与三语文档通过。晚到或冲突证据的追加、价格快照和货币结算仍待验收。

## GW-04 文本成本与下游应收

启用 `billing` 及持久化用量后，文本尝试在出站前保存独立的成本和应收费率快照，包含价格版本、币种和每百万 Token 费率。已确认的用量拆分成互斥计价项，以定点十进制计算并在每笔最终金额处按 12 位小数半偶舍入。单笔尝试的用量完成、分类收费及原有输出 Token 配额共用事务。失败回退没有下游应收，可能产生的上游成本等待证据。价格缺失、报告模型不一致或用量不完整时保留待核对。Artisan `ai:gateway:usage:list --billing` 查询原始费率和金额，两类金额独立交付。

SQLite 和 PostgreSQL 18.7 的实际 HTTP 和 Artisan 验收费用拆项、人民币和美元的独立账目、微小金额半偶舍入、明确零、缺价、缺计数、模型变更、重复终帧及重启换价。SQLite 故障样本验证计费更新失败时同一用量结算回滚。两次成功回合和一次失败回退场景目前确认成本 0.20、应收 0.25，失败尝试金额待供应商账单补证。包测试、严格 Clippy 和三语文档通过。资金余额、硬预算、晚到调整及账单对账仍待验收。

- [固定 Sub2API 用量计费事务](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/repository/usage_billing_repo.go) · [固定 Laravel TextUsage](https://github.com/laravel/ai/blob/1d269f2359aa166aaf10b212151bf556dd86a02e/src/Responses/Data/TextUsage.php)

## GW-04 xAI 原生用量

网关用实际出站连接的 SDK 用量适配器归一化 xAI 输出及推理计数。原生 Responses 的下游 JSON 与 SSE 保留上游用量字段。账本统一以包含推理的输出总量结算，独立推理费率按现有机制从总输出中扣出对应子项。

通过 SQLite、本地 xAI 协议服务、真实 HTTP 及独立 Artisan 验证普通响应和 SSE。上游分别报告输出 3 加推理 4、输出 7 含推理 4，均报告输入 10、缓存读取 2、总量 17。两笔账本均记录输出 7、推理 4，按样例费率各结算成本 0.000053 USD、应收 0.000065 USD。上游响应费用见下节。真实账单对账和 xAI 官方账户仍待验收。

## GW-04 xAI 回报实际费用

原生 xAI Responses 的最终费用在用量结算事务中写入成本记录。整型 ticks 按 10¹⁰ ticks 每美元转换，`usage:list --billing` 保留 `reported_cost_usd`、本地费率估算和下游应收。USD 上游账号预算优先使用回报费用。账单证据沿用已有匹配和冲突规则，金额不一致时保留原回报并标记 `conflicting_evidence`。已有账本应用可空列迁移。

SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 通过独立 HTTP、Artisan 和本地协议模拟核对 31357 ticks 的精确成本 0.0000031357 USD、明确零、无终帧费用、重复终帧不重新结算。失败终帧回报按上游成本记入 USD 预算，下游应收保持不计费。导入一致账单确认实际费用，导入有差额的修订保留相互冲突的证据。21 项 SDK 回归、SDK 与网关严格 Clippy、三语文档站点通过。本批费用证据来自模拟，xAI 官方账户及正式账单仍待联调。

- [xAI 逐请求费用与 ticks](https://docs.x.ai/developers/cost-tracking)

## GW-04 逐请求供应商账单证据

`ai:gateway:billing:import` 接收带来源、账单 ID、版本、UTC 窗口和操作者的逐请求账单。按具名连接、实际出站窗口和上游请求 ID 精确匹配。源账单与明细追加保存，同版本同内容幂等，不同内容冲突。部分账单、未匹配项、歧义和币种不符保留证据。每个来源采用最新完整版本的同一请求金额，彼此冲突的完整来源保留待核对状态。`usage:list --billing` 保留配置价格结算值，通过 `actual` 提供可确认的供应商成本。

独立 HTTP 生成两次成功回合及一次失败回退，再由 Artisan 导入失败尝试的 0.03 成本。SQLite 与 PostgreSQL 18.7 均复算成本 0.23、下游应收 0.25，并验证同请求 ID 跨连接隔离、并发重复导入、部分账单、币种不符、旧版重送、更正及来源冲突。包测试、严格 Clippy 与三语文档通过。此样例从两个独立 HTTP 请求发出。单次 SDK 调用的组合验收见下节。汇总报表与资金预算见后续各节，追加调整仍待实现。

## GW-04 SDK 调用与模型回合关联

在具名 SDK 连接上启用 `forward_usage_context`。网关按通过鉴权的 Key 范围保存 SDK 调用 ID 和回合编号。每个回合有独立的网关请求 ID，每次出站有独立的尝试 ID。调用 ID 仅用于查询与归集，回退沿用同一回合的网关请求 ID。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 均以实际 Artisan 缓冲调用与 HTTP 流完成两回合工具调用，并核对 ID、两种 Key 隔离、默认关闭、异常头忽略及成本 0.20、应收 0.25。两包测试及严格 Clippy 通过。

同一 SDK 调用的组合样例也在 SQLite 与 PostgreSQL 18.7 上通过。首轮由真实 Artisan 驱动，执行一次客户端工具。应用用 SDK 步骤中间件将工具结果作为第二轮独立文本输入，去除上游工具历史和声明。两个真实网关请求共三次出站，第二轮收到 503 后切换连接与模型。Artisan 按第二轮失败尝试的上游请求 ID 补录 0.03，按单次 SDK 调用核对成本 0.23、应收 0.25 和全部尝试归属。所用合成上游经 HTTP 返回。原有工具历史和已开始串流的重放限制继续适用。

现行代码和迁移在 SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 重跑两回合三次出站样例，迟到证据入账后仍为成本 0.23 USD、应收 0.25 USD。

### xAI 原生 Responses 三条链路

在独立 Artisan、真实 HTTP 和本地 xAI 协议服务之间验证三条链路。SDK 直连完成两轮且不产生网关记录。外部 HTTP 客户端使用公开别名完成原生 Responses。另一个具名 SDK 连接用 `driver: "xai"`、下游 Key、`store: false` 及 `forward_usage_context` 经网关调用同一公开模型。普通与 SSE 均混用三种供应商工具声明和一个本地工具。第二轮发送原始加密推理、集合搜索条目和原工具调用 ID。供应商侧未收到 SDK 关联头。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 中，每次 SDK 调用有两个回合、两个网关请求 ID 与两个尝试 ID，同一关联范围。每组成功样例实际成本 0.20 USD、下游应收 0.25 USD。第二轮带工具历史收到 503 时，备用连接未收到调用。失败回合报告的 0.03 USD 连同首轮 0.12 USD 归入成本，只有首轮形成 0.15 USD 应收。三语站点检查通过。证据来自本地协议模拟，xAI 官方账户和 DSH 图形客户端的此项组合联调仍待验收。

## GW-04 用量与计费同页读取

`ai:gateway:usage:list --billing` 先限定用量页，在同一条数据库查询中读取用量、结算和完整账单证据。证据关联不会增加用量条数或改变游标。SQLite 与 PostgreSQL 18.7 实测 HTTP 请求暂停、结算与多个 Artisan 进程并发读取，分别确认未结算和已结算记录各自一致。逐条游标、空页、重复及更正证据、金额和事务回滚回归通过，网关包测试和严格 Clippy 通过。后续使用同一夹具在 MySQL 8.4.13 补跑通过。SQL Server 尚待发布矩阵验证。

## GW-04 分组应收货币预算

同组 Key 在共用数据库按币种和 UTC 日合并预占。配置上游模型已验证的可计费输入上限，网关对生成请求应用输出 Token 限制，按费率分项的最大值向上取整，先预占再出站。用量尝试、费率、输出配额与应收预算使用同一事务。已知终值按实际应收释放差额，缺少终值的预占保留。超过声明上界的实际应收继续保存在账本，本账期停止新增调用。支持可限额的 Responses、Chat Completions 和 Messages，以及按输入预占的向量请求。其他能力按分组策略拒绝或放入监测模式。通过 `ai:gateway:budget:show` 读取当前窗口的余额。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测两个网关进程在同组跨 Key 竞争。确认余额不足时无上游请求，验证输出限制、隔离分组、正常结算、失败尝试释放、回退再预占、未知策略、上界被突破后停线及进程重启时保持预占。两包测试、严格 Clippy 和三语文档通过。此批控制下游应收，运维恢复和账号成本预算见后续批次。

## GW-04 恢复未出站预占

`ai:gateway:budget:recover <group>` 在数据库用与出站标记互斥的更新条件关闭超时未出站记录，并在同一事务将原快照标记为不计费、释放应收预占及同次预留的输出 Token。可按记录年龄和条数执行，重试及并发恢复只释放一次。已有出站标记的费用继续保留供对账。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 在隔离库预置未出站记录，经 Artisan 验证回滚、分批、双进程重复恢复、费用快照保留和输出配额释放。HTTP 进程在真正向合成上游送出请求后被终止，重启后恢复指令保留该笔预占。本轮两包测试、严格 Clippy 和三语文档通过。


## GW-04 Sub2API Key 计费元数据

`GET /v1/sub2api/billing` 沿用固定 [Sub2API Key 计费处理器](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/handler/gateway_key_billing.go) 的对象标识、版本、当前倍率字段、UTC 观察时间和无缓存响应。仅已启用计费且通过鉴权的分组 Key 可查询。首批沿用公开模型费率及默认倍率 1，分组费率倍率见下节。高峰倍率关闭。重复或逗号分隔的 `include` 可请求该 Key 的分组名称，分组说明为空。此入口的回传范围限于计费元数据。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测静态 Key、Bearer 与 `x-api-key`、无凭证和无分组的拒绝、同一查询无法指定其他分组。实际 Artisan 签发、轮换和撤销托管 Key 后分别验证 HTTP 行为。停用计费时已认证请求返回 404，验证该端点未触发上游。两包测试、严格 Clippy 和三语文档通过。用户级和高峰倍率属于后续计费策略。

## GW-04 分组费率倍率

`groups.<group>.rate_multiplier` 按十进制解析，作用于下游应收及其预占上界。费用快照保存倍率，并在最终金额按半偶规则舍入。兼容计费元数据同步返回实际分组倍率，默认值为 1。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测 0.5 倍率预占 0.075、结算 0.00625、上游成本保持 0.01。精度样例验证先乘倍率再舍入。重启后应用新倍率 0.25 与新版本费率，既有账目仍可读回原始金额和快照。独立 HTTP 和 Artisan、两包测试、严格 Clippy 与三语文档通过。

## GW-04 上游账号每日成本预算

`accounts.<connection>.budget` 使用上游币种，在该连接的公开模型和下游分组间共享。账号成本与分组应收可在同一事务预占。若账号预算不足，网关可在出站前选择另一个获准连接。退回或切换时按各次出站分别结算，未知供应商成本继续挂账。余额与未出站恢复命令支持 `--account`，同笔未出站尝试的双重预占可一起恢复。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测两个进程跨分组共享上游限额、拒绝回滚另一层预占、切换独立 CNY 账号、USD 应收、异常成本保留和双重恢复。通过插入上一 UTC 账期的账本记录验证余额按窗口隔离。两包测试、严格 Clippy 和三语文档通过。已出站的未知费用按后续账务调整流程核对。

## GW-04 供应商汇总账单与差额查看

`ai:gateway:billing:import` 可导入 `totals`。网关按具名连接、币种和账期比较 Token 费用，查询在一个数据库语句内合并估算与已确认逐请求证据。部分账单、冲突、缺失价格和未知分类保留现有记录和差异状态。汇总差额不分摊到尝试。每个修订版本保存当时快照，重送原内容可读回相同结果。`billing:report` 可按账单 ID 回看汇总差额、逐请求证据及操作者。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实际 HTTP 生成用量，Artisan 导入部分与完整账单、并发重复及修订。核对正负差额、未映射分类、USD 与 CNY 分开、逐请求成本晚到和冲突、历史快照及运维回读。两包测试、严格 Clippy 和三语文档通过。分批页暂存见下节，追加贷项及关联预算的结算调整列为后续项。

## GW-04 分页账单导入与恢复

账单文件可携带 `page.number` 和 `page.total`。各页独立保存原数据、操作者和导入时间，版本的来源、账期、完整性声明及总页数一致。通过同一份账单版本的事务锁协调并发。合并前的分批账单可查看已收到的页号，账目仍等到全部分页持久化和校验后统一发布。已完成版本支持同页及整份账单的幂等重送。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测乱序、缺页、进程重启、同页冲突、页数冲突、跨页重复、并发补最后一页和已组装的部分账单。真实 HTTP 生成三次上游尝试，导入账单后二次核对成本 0.23 与应收 0.25。两包测试、严格 Clippy 和三语文档通过。

## GW-05 Sub2API 专用计费入口移除

按更新后的 E07 计划移除旧专用计费路径、响应协议和对象标识。保留历史联调记录。实际 `route:list` 在启用网关和计费时确认旧路径已移除，真实 HTTP 验证持有有效或无效 Key 访问均为 404。`/v1/usage` 与 `/v1/models` 按原有 Key 鉴权和模型授权正常工作，其他外部协议路径继续注册。应用计量、费用、分组倍率、账单及预算能力沿用现有配置和 Artisan。

## GW-04 追加借项与贷项

`ai:gateway:billing:adjust` 以原尝试、费用方向和凭据编号去重。追加记录包括金额、币种、原因、操作者与可选完整报表 ID。保留原费率快照、结算金额和供应商证据。`usage:list --billing` 同一页读取借项、贷项及净额。汇总账单按导入时的净额形成独立快照。原费用待确认时净额也保留待确认。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测 HTTP 入账后经 Artisan 重复与并发导入，验证同凭据冲突、币种错配、缺原尝试和缺关联账单的事务回滚。上游实际成本 0.03 追加 0.01 贷项后净额为 0.02。下游应收 0.15 追加 0.05 贷项和 0.02 借项后净额为 0.12。验证导入备份连接未受影响、汇总快照与重启后回读。两包测试、严格 Clippy 和三语文档通过。已出站的预算预占与晚到费用的结算调整见下节。

## GW-04 晚到费用结算与资金释放

`ai:gateway:budget:reconcile` 按尝试与费用方向核对当前证据和追加调整，留存操作者、凭据编号及预算变更。确认的净额应用到原预算窗口。同一凭据只生效一次，更正用独立凭据记录。费用超出声明的原上界时，保持原预算贡献并停止该账期新增准入，费用实值继续对账。已出站且未结束的上游流若先收到完整费用证明，可先结算成本，实际终帧仍持久化用量及费率计算值。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 实测晚到账单将 0.12 预占降为 0.03 已扣，后续更正为 0.02，分别验证并发凭据重送、注入数据库故障后的事务回滚、证据冲突、下游贷项及供应商证明与流式终值竞态。两包测试、严格 Clippy 和三语文档通过。
