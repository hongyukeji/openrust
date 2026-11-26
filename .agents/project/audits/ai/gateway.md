# GW-00 网关端点账本

基线为 Sub2API [`0f6671457132425d1b84f3d80536b53e02942424` 的路由与中间件装配](https://github.com/Wei-Shaw/sub2api/blob/0f6671457132425d1b84f3d80536b53e02942424/backend/internal/server/routes/gateway.go)。固定源码用于确定可访问的路径及准入方式。下表中「待验收」表示 OpenRust 尚无对应的端到端证据。

## 主协议

| 固定基线路由 | 鉴权和行为 | OpenRust 裁决 |
|---|---|---|
| `POST /v1/chat/completions` | 网关 Key 与分组模型白名单，依上游平台派发，普通与 SSE | GW-02 已验收 OpenAI 同协议 JSON、SSE 和下游取消。跨协议映射及多账号待 GW-03／04。 |
| `POST /v1/responses` | 网关 Key 与分组，原生 Responses 或替代上游 | GW-02 已验收同协议 JSON 和 SSE。GW-03 已验收 Responses → Messages 纯文本 JSON、SSE 和用量。反向纯文本转换也已验收。工具、结构化及高级字段转换待补。 |
| `POST /v1/responses/*subpath` | 网关 Key，路由守卫只放行允许的操作路径，`input_tokens` 可独立派发 | GW-03 待逐操作核对。GW-02 拒绝共享凭证上的 `store: true`、`background: true`、`previous_response_id`、会话与旧资源引用。持久状态先取得租户与账号归属证据再开放。 |
| `GET /v1/responses` | 网关 Key，升级 WebSocket 并走专用 Responses handler | GW-03 待验收首帧、取消、连接期间权限、账号归属和关闭码。 |
| `POST /v1/messages` | 网关 Key 与分组，按上游平台在 Messages 与 OpenAI 网关间派发 | GW-03 已接 Anthropic 同协议，原生错误、SSE、下游 `x-api-key` 与独立上游认证。Messages → OpenAI Responses 的普通与 SSE 纯文本经实际 HTTP 验收，其余能力待补。 |
| `POST /v1/messages/count_tokens` | 网关 Key，按供应商选择原生计数、桥接或本地估算 | GW-03 原生 Anthropic 计数已接通。上游返回的估算与生成用量分开；其他上游的实现仍待核对。 |
| `GET /v1/models`、`GET /v1/models/{model}` | 网关 Key，按授权返回目录或单模型；列表中 `client_version` 有 Codex 模型清单分支 | GW-02 OpenAI 列表，GW-03 单模型及 Anthropic 格式。Codex 清单及版本分支待 GW-05 验收。 |
| `GET /v1beta/models`、`GET /v1beta/models/{model}`、`POST /v1beta/models/{model}:{action}` | Google 协议 Key 与分组，原生模型目录及动作派发 | GW-03 已验收目录、单模型、`generateContent`、`streamGenerateContent?alt=sse`、`countTokens`、`x-goog-api-key` 及 Google 认证错误。JSON 流、Embeddings 等后续动作仍待核对。 |
| `POST/GET /v1beta/cachedContents`、`GET/PATCH/DELETE /v1beta/cachedContents/{id}` | Google 协议 Key 与分组，网关资源 ID 绑定上游账号 | GW-03／04 待按生命周期核对，依赖租户与账号归属。 |
| `POST /v1/embeddings` | 网关 Key 与分组，固定版本限制 OpenAI 平台 | AI-05／GW-03 已验收 OpenAI 协议 JSON、维数、批量、公开模型与真实 Token。其他原生协议待核对。 |

## 媒体、实时与运维

| 固定基线路由 | 鉴权和行为 | OpenRust 裁决 |
|---|---|---|
| `POST /v1/images/generations`、`POST /v1/images/edits` | 网关 Key，平台适配生成／编辑、JSON 或 multipart | AI-06／GW-03 待验收模型映射、上传、二进制或 JSON、限制及错误。 |
| `POST /v1/images/{generations,edits}/async`、`GET /v1/images/tasks/{id}` | 网关 Key，异步任务写入及读取 | GW-03／04 待验证任务所有权、失效及结果读取。 |
| `GET/POST /v1/images/batches`、`GET /v1/images/batches/models`、`GET/DELETE /v1/images/batches/{id}` | 网关 Key，批量任务增删查与模型目录 | GW-03／04 待按批次、租户持久化能力裁决。 |
| `GET /v1/images/batches/{id}/items`、`GET /v1/images/batches/{id}/items/{custom_id}/content`、`GET /v1/images/batches/{id}/download`、`POST /v1/images/batches/{id}/cancel`、`DELETE /v1/images/batches/{id}/outputs` | 网关 Key，任务项、结果、取消与删除 | GW-03／04 待验证资源所属、二进制结果、部分失败和可重试性。 |
| `POST /v1/videos`、`POST /v1/videos/{generations,edits,extensions}`、`GET /v1/videos/{id}[/content]`、`GET /v1/videos/{generations,edits,extensions}/{id}[/content]` | 网关 Key，依平台支持创建／编辑／扩展、状态与二进制读取 | AI-06／GW-03／04 待按平台验证任务 ID、状态、取消与内容。 |
| `POST /{api/v3,v3,v1}/contents/generations/tasks`、`GET/DELETE /{api/v3,v3,v1}/contents/generations/tasks/{id}` 及无版本同路径 | 网关 Key，Seedance 任务创建、查询和删除 | GW-03／04 待按国内视频供应商任务及资源所有权验收。 |
| `POST /v1/tts`、`POST /v1/stt`、`GET/POST /v1/custom-voices`、`GET/PATCH/DELETE /v1/custom-voices/{id}`、`GET /v1/custom-voices/{id}/audio` | 网关 Key，固定版本依平台开放；文本、上传、下载或资源管理 | AI-06／GW-03／04 待逐操作核对，音色资源要求所属校验。 |
| `POST /v1/live`、`GET /v1/live/{call_id}`、`GET /v1/realtime` | 网关 Key，Live 与 Sideband 或实时协议升级 | GW-03／04 待验收不同实时入口的上游支持、双向生命周期与归属。 |
| `POST /v1/systemone`、`POST /v1/alpha/search`、`POST /v1/{web_search,x_search}` | 网关 Key 与平台能力门控，部分动作有独立文本正文限制 | GW-03 待按原生平台能力裁决，分别验收错误和限制。 |
| `GET /v1/usage`、`GET /v1/sub2api/billing` | 网关 Key，调用方用量或账单信息 | GW-02 已交付结构化调用与实报 Token 日志。GW-04 待交付可查询用量、计量及权限；商业套餐与付费账单界面由独立项目承载。 |

## 兼容路径与账号类型

| 基线入口 | 裁决 |
|---|---|
| 无版本 `/models[/{model}]`、`/messages/count_tokens`、`/responses[/*subpath]`（POST、升级 GET）、`/chat/completions`、`/embeddings`、图片／视频／语音／实时／搜索部分路径 | GW-03／05 待逐个对应路由，并确认别名与主入口对授权、限流和记录采用同一逻辑。 |
| `/backend-api/codex` 下的 `GET /models`、`POST /responses[/*subpath]`、`GET /responses`、`POST /alpha/search`、`POST /realtime/calls`、`GET /{call_id}` | GW-05 待验证 Codex 客户端配置、模型清单与会话边界；专用账号适用性归 GW-04。 |
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
