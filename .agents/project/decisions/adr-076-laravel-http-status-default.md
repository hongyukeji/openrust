# ADR-076：Laravel 语义 HTTP 状态默认值

- 状态：已接受
- 日期：2026-10-29
- 上游：Laravel 13.39.0；Laravel 文档固定 commit `960eedf0911155ad16326ade49c0ea5d18eda0b8`

## 背景

ADR-070 已重开旧的强制响应包装与 `always_ok` 默认值，要求在 R3 按 Laravel 的 Response/异常行为定稿。变更前，`ApiConfig`、根 `config/api.toml` 与生成项目模板默认将所有 JSON 状态压为 200；这会让 HTTP 客户端、代理与浏览器无法按标准区分创建、校验、授权、资源不存在、限流和服务故障。

Laravel 为返回的 Response 保留 HTTP 状态；校验失败且请求期望 JSON 时返回 422。OpenRust 的统一 JSON envelope 是获准增强，应保持其 `success/code/message/data/errors/request_id`，并与真实 HTTP 状态一致。

## 决策

1. `semantic` 是框架、应用、未显式配置的 HTTP 请求上下文及生成项目的默认 `HttpStatusMode`。
2. 现有 `always_ok` 保留为明确配置的可选响应模式，不再是模板或运行默认；启用它时，框架 JSON 包装将状态改为 200，但不改 envelope 中的稳定机器码。
3. `Created` 与错误类型继续提供语义状态；`NoContent` 保持现有 200 成功 envelope，文件、HTML、SSE、原始流及 gRPC 不被 JSON 状态包装器改写。
4. 成功/失败 JSON 结构、脱敏、请求 ID、校验字段错误和请求错误状态映射按当前公开契约保留。默认值调整不得让错误返回 200 或把 HTTP 错误改为成功。
5. 配置文件省略 `[api].http_status`、环境变量缺失及 framework 默认值均得到 `semantic`；应用仍可用 TOML 或 `APP_API_HTTP_STATUS=always_ok` 显式覆盖。

## 依据与验收

- [Laravel 13.x HTTP Responses](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md)：Response 可携带状态码和 headers。
- [Laravel 13.x Validation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md)：JSON 校验失败返回 HTTP 422。
- [总计划 C04/C06 与 R3](../plans/laravel-rebuild.md)：真实 HTTP 状态默认、统一响应增强与错误渲染。
- 行为测试需覆盖默认创建 201、404/405/413/415/422/429/500/503/timeout 状态及相同 envelope；显式 `always_ok` 单独验证全 200 行为。
- 配置测试覆盖 serde/default、配置文件与环境覆盖；生成项目验证默认值及真实 HTTP；刷新 `modules/example`、根默认 TCP 服务、文档和 CHANGELOG。
- 完整实现与运行记录归 [R3-A 测试记录](../memory/testing.md#r3-a-laravel-语义-http-状态默认与统一-json-响应)。

旧 ADR-008 对 always-ok 默认的约束由 ADR-070 与本决策取代；其错误码和字段语义继续有效。
