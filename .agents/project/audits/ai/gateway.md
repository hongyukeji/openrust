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
