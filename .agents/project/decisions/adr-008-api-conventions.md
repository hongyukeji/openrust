# ADR-008 HTTP API 交互规范与数据格式

- 状态：Accepted
- 日期：2026-10-25（同日修订：响应改为统一包装结构，替代 problem+json）

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

| 动作 | 方法与路径 | 成功响应（统一包装，`success = true`） |
| --- | --- | --- |
| list | `GET /api/posts` | `data` 为数组，并附带 `meta` |
| get | `GET /api/posts/{id}` | `data` 为资源对象 |
| create | `POST /api/posts` | `data` 为新建的资源，响应头带 `Location: /api/posts/{id}` |
| update | `PUT` 或 `PATCH /api/posts/{id}` | `data` 为更新后的资源 |
| delete | `DELETE /api/posts/{id}` | `data: null` |

- `PATCH` 为部分更新：字段缺省表示不修改，显式传 `null` 表示清空。`PUT` 为整体替换。
- 非 CRUD 的动作用子资源加 `POST` 表示，如 `POST /api/posts/{id}/publish`，不在路径里用冒号或动词前缀。

### 2. 数据格式

- 请求和响应都是 `application/json`，编码 UTF-8。
- **字段名用 `snake_case`**，与 Rust 字段名一致，不需要 `#[serde(rename_all)]`。先例：GitHub、Stripe、Rails、Laravel、DRF 的默认写法。
- **所有响应（成功和失败）都使用统一包装结构**，见 §7。`data` 中单个资源是对象，集合是数组。

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

- **所有错误都使用 §7 的统一包装结构**，`success = false`。请求解析失败、未匹配路由、方法不允许、超时、请求体过大、panic 都在此列。框架不返回纯文本错误，也不返回空正文。
- `code` 是稳定的 `snake_case` 机器码，客户端按它分支处理。`message` 是可以直接展示给用户的提示。字段校验失败时附带 `errors`，格式为 `{字段: [消息]}`。5xx 不暴露内部细节。
- 业务可以用 `Error::custom(code, message)` 定义自己的码，例如 `order_already_paid`。
- 框架内置的 `code`，以及 `[api] http_status = "semantic"` 时对应的 HTTP 状态码：

| 情形 | `code` | semantic 下的状态码 |
| --- | --- | --- |
| 成功 | `ok` | 200；创建为 201；删除为 200，`data: null` |
| JSON 语法错误，路径或查询参数无法解析 | `bad_request` | 400 |
| 未认证 | `unauthorized` | 401 |
| 无权限 | `forbidden` | 403 |
| 资源或路由不存在 | `not_found` | 404 |
| 方法不允许 | `method_not_allowed` | 405（附 `Allow` 头） |
| 超时 | `request_timeout` | 408 |
| 冲突 | `conflict` | 409 |
| 请求体过大 | `payload_too_large` | 413 |
| `Content-Type` 不是 JSON | `unsupported_media_type` | 415 |
| 字段校验失败 | `validation_failed` | 422 |
| 限流 | `rate_limited` | 429（附 `Retry-After` 头） |
| 服务器错误 | `internal_error` | 500 |

### 5. 请求头与响应头

- `x-request-id`：客户端可以传入；不传时由服务端生成。响应头和正文的 `request_id` 都会回传。
- 认证：API 用 `Authorization: Bearer <token>`，Web 用会话 cookie（ADR-004）。
- 限流时附带 `Retry-After`。创建成功附带 `Location`。
- CORS 与压缩见 `[server.cors]`、`[server.compression]`。

### 6. 框架提供的类型

- prelude 中的 `Json`、`Path`、`Query` 换成 OpenRust 自己的版本。它们的用法与 axum 一致，区别是解析失败时返回本 ADR 规定的错误包装：`bad_request`、`unsupported_media_type`、`payload_too_large`，字段类型不符时返回 `validation_failed`。`Json<T>` 也可以作为响应类型使用。仍然可以通过 `openrust::http::axum` 拿到 axum 原生类型。
- `Validated<Json<T>>` / `Validated<Query<T>>` 先做解析，再做 `validator` 校验。
- `Created<T>` 支持带 `Location` 头：`Created::with_location(location, body)`。
- 默认中间件栈为未匹配路由、405、408、413 统一返回错误包装。
- OpenAPI 由 `utoipa` 生成，非生产环境挂在 `/docs`，在后续里程碑实现。

### 7. 统一响应结构

成功示例：

```json
{ "success": true, "code": "ok", "message": "ok", "data": { "id": 1, "title": "Hello" }, "request_id": "b267209f-..." }
```

列表示例：

```json
{ "success": true, "code": "ok", "message": "ok", "data": [ { "id": 1 } ], "meta": { "page": 1, "per_page": 20, "total": 57, "total_pages": 3 }, "request_id": "..." }
```

失败示例：

```json
{ "success": false, "code": "validation_failed", "message": "Validation failed", "data": null, "errors": { "title": ["is required"] }, "request_id": "..." }
```

| 字段 | 出现时机 | 含义 |
| --- | --- | --- |
| `success` | 总是 | 布尔值。客户端只需要看它判断成败 |
| `code` | 总是 | 稳定的 `snake_case` 业务码。成功时为 `ok` |
| `message` | 总是 | 面向用户的提示文字 |
| `data` | 总是 | 业务数据。没有数据时为 `null` |
| `meta` | 仅分页集合 | `page`、`per_page`、`total`、`total_pages` |
| `errors` | 仅校验失败 | 格式为 `{字段: [消息]}` |
| `request_id` | 总是 | 与响应头 `x-request-id` 相同 |

- 包装由框架统一完成。handler 照常返回 `Json(T)`、`Paginated<T>`、`Created<T>`、`NoContent` 或 `Err(Error)`，不需要手写包装结构。如果需要返回非 JSON 内容（文件、流、HTML、`/health`），直接返回原始响应，不做包装。
- 配置项 `[api] http_status`：
  - `"always_ok"`（默认）：所有包装响应的 HTTP 状态码都是 200，客户端只依赖正文。
  - `"semantic"`：正文结构不变，同时按 §4 的表格设置 HTTP 状态码，适合接入网关、监控或开放给第三方的 API。
- 不采用 RFC 9457 problem+json 的理由：开发者希望所有接口使用同一种结构，客户端不依赖 HTTP 状态码；这也是 Spring Boot、Gin、ThinkPHP 等跨语言项目常见的 `Result<T>` 约定。需要标准 HTTP 语义时，切换到 `semantic` 即可。

## 影响

- `openrust-core` / `openrust-http`：新增统一响应包装及 `[api] http_status` 配置；提取器与 404/405/408/413 兜底输出包装结构；`Created::with_location`；对应测试。门面 prelude 导出新的提取器。
- 生成器（M1 `openrust new`、M6 scaffold）生成的 handler、测试和文档都按本 ADR 输出。
- `conventions.md`「框架规范」加入 API 速查，并链接本 ADR。
