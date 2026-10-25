# ADR-008 HTTP API 交互规范与数据格式

- 状态：Accepted
- 日期：2026-10-25

## 背景

框架已经实现了 problem+json 错误、`Created` / `NoContent` / `Paginated`、`PageParams`、`x-request-id`，但没有一份统一的 API 规范。与此同时存在两处不一致：

- prelude 导出的 axum `Json` / `Path` / `Query` 在请求解析失败时返回纯文本；
- 未匹配路由的 404、请求超时的 408 返回空正文。

API 格式是框架对外的契约，生成器、文档和所有用户项目都依赖它，需要先定下来。判定规则同 ADR-006：先看主流先例，没有共识时用跨语言开发者最熟悉的写法。先例包括 RFC 9110 / 9457、GitHub REST API、Stripe API、Google AIP、Microsoft REST API Guidelines，以及 Rails、Laravel、Django REST Framework 的默认行为。

## 决策

### 1. 路径与方法

- 资源路径用复数、kebab-case 的名词：`/api/posts`、`/api/blog-posts`；成员路径为 `/api/posts/{id}`。嵌套最多一层：`/api/posts/{post_id}/comments`。先例：GitHub、Google AIP-122。
- JSON API 统一挂在 `/api` 前缀下（loco scaffold 先例）。出现破坏性变更时才引入 `/api/v2`，首版不带版本号。
- 资源动作与方法、状态码的映射（ADR-006 §4）：

| 动作 | 方法与路径 | 成功响应 |
| --- | --- | --- |
| list | `GET /api/posts` | 200，返回分页集合 |
| get | `GET /api/posts/{id}` | 200，返回资源对象 |
| create | `POST /api/posts` | 201，返回资源对象，带 `Location: /api/posts/{id}` |
| update | `PUT` 或 `PATCH /api/posts/{id}` | 200，返回更新后的资源对象 |
| delete | `DELETE /api/posts/{id}` | 204，无正文 |

- `PATCH` 为部分更新：字段缺省表示不修改，显式传 `null` 表示清空。`PUT` 为整体替换。
- 非 CRUD 的动作用子资源加 `POST` 表示，如 `POST /api/posts/{id}/publish`，不在路径里用冒号或动词前缀。

### 2. 数据格式

- 请求和响应都是 `application/json`，编码 UTF-8。错误响应用 `application/problem+json`。
- **字段名用 `snake_case`**，与 Rust 字段一致，不需要 `#[serde(rename_all)]`。先例：GitHub、Stripe、Rails、Laravel、DRF 默认。
- **单个资源直接返回对象**，不包 `data` 外壳（GitHub、Stripe、Rails、DRF 先例）：

  ```json
  { "id": 1, "title": "Hello", "published_at": null, "created_at": "2026-10-25T10:00:00Z", "updated_at": "2026-10-25T10:00:00Z" }
  ```

- **集合统一用 `Paginated<T>`**，外层为 `data` 加 `meta`（Stripe 列表的 `data`、JSON:API 与 Laravel 的 `meta` 先例）：

  ```json
  { "data": [ { "id": 1 } ], "meta": { "page": 1, "per_page": 20, "total": 57, "total_pages": 3 } }
  ```

- 字段值的写法：

| 类型 | 规则 |
| --- | --- |
| 时间 | RFC 3339 UTC，带 `Z`，例如 `2026-10-25T10:00:00Z`；字段名以 `_at` 结尾（`created_at`）。纯日期用 `YYYY-MM-DD`，字段名以 `_on` 或 `_date` 结尾 |
| ID | 自增主键为 JSON 整数；UUIDv7 主键为小写带连字符的字符串 |
| 金额 | 最小货币单位的整数（`amount: 1999`）加 `currency: "usd"`，与 Stripe 一致；不使用浮点 |
| 枚举 | `snake_case` 字符串 |
| 空值 | `Option` 为 `None` 时输出 `null`，字段保留，不省略；空集合输出 `[]`，不输出 `null` |
| 布尔 | 字段名用 `is_` 或 `has_` 前缀，或用形容词（`published`） |

### 3. 查询参数

| 用途 | 规则 |
| --- | --- |
| 分页 | `page`（从 1 开始，默认 1）、`per_page`（默认 20，范围 1–100），由 `PageParams` 负责校验 |
| 排序 | `sort=-created_at,title`，前缀 `-` 表示降序（JSON:API 先例） |
| 过滤 | 直接用字段名作参数，例如 `?status=published&author_id=3`；范围查询用 `_from` / `_to` 后缀 |
| 搜索 | `q=` |

后续如需游标分页，另立 ADR，不影响现有 `page` 参数。

### 4. 错误

- **所有错误响应都必须是 RFC 9457 problem+json**，包括请求解析失败、未匹配路由、方法不允许、超时、请求体过大和 panic。框架不得返回纯文本或空正文的错误。
- 格式（现有实现）：

  ```json
  { "type": "urn:openrust:problem:validation_failed", "title": "Unprocessable Entity", "status": 422, "code": "validation_failed", "detail": "...", "errors": { "title": ["is required"] } }
  ```

  - `code` 是稳定的 `snake_case` 机器码，客户端按它判断分支。
  - `errors` 只在校验失败时出现，形如 `字段: [消息]`。
  - 5xx 不暴露内部细节。
  - 请求关联只靠响应头 `x-request-id`，不在正文里重复。
- 状态码与 `code` 的对应：

| 情形 | 状态码 | `code` |
| --- | --- | --- |
| JSON 语法错误、路径或查询参数无法解析 | 400 | `bad_request` |
| 未认证 | 401 | `unauthorized` |
| 无权限 | 403 | `forbidden` |
| 资源或路由不存在 | 404 | `not_found` |
| 方法不允许（带 `Allow` 头） | 405 | `method_not_allowed` |
| 超时 | 408 | `request_timeout` |
| 冲突（唯一约束、版本冲突） | 409 | `conflict` |
| 请求体过大 | 413 | `payload_too_large` |
| `Content-Type` 不是 JSON | 415 | `unsupported_media_type` |
| 字段校验失败（含 JSON 结构正确但字段类型不符） | 422 | `validation_failed` |
| 限流（带 `Retry-After` 头） | 429 | `rate_limited` |
| 服务器错误 | 500 | `internal_error` |

### 5. 请求头与响应头

- `x-request-id`：客户端可以传入；不传时由服务端生成。响应一律回传。
- 认证：API 用 `Authorization: Bearer <token>`，Web 用会话 cookie（ADR-004）。
- 限流时附带 `Retry-After`。创建成功附带 `Location`。
- CORS 与压缩见 `[server.cors]`、`[server.compression]`。

### 6. 框架提供的类型

- prelude 中的 `Json`、`Path`、`Query` 换成 OpenRust 自己的版本。它们的用法与 axum 一致，区别是解析失败时返回本 ADR 规定的 problem+json：400、415，或字段类型不符时的 422。`Json<T>` 也可以作为响应类型使用。仍然可以通过 `openrust::http::axum` 拿到 axum 原生类型。
- `Validated<Json<T>>` / `Validated<Query<T>>` 先做解析，再做 `validator` 校验。
- `Created<T>` 支持带 `Location` 头：`Created::with_location(location, body)`。
- 默认中间件栈为未匹配路由提供 404 problem 兜底，405、408、413 也统一返回 problem。
- OpenAPI 由 `utoipa` 生成，非生产环境挂在 `/docs`，在后续里程碑实现。

## 影响

- `openrust-http`：新增提取器、404/405/408/413 的 problem 兜底、`Created::with_location`，以及对应测试。门面 prelude 改为导出新的提取器。
- 生成器（M1 `openrust new`、M6 scaffold）生成的 handler、测试和文档都按本 ADR 输出。
- `conventions.md`「框架规范」加入 API 速查，并链接本 ADR。
