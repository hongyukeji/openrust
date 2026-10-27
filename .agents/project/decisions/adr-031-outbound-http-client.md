# ADR-031：统一出站 HTTP 客户端

- 状态：Accepted
- 日期：2026-10-26

## 背景

邮件、短信及应用供应商适配需要共享连接池、超时和错误边界。框架没有统一出站 HTTP API；各驱动自行创建客户端会造成超时、重定向和敏感日志策略不一致。

## 决策

- 将能力公开为 `openrust::http::HttpClient`，使用可克隆的共享客户端；不隐式安装到 `HttpPlugin`，应用通过 `AppBuilder::provide` 显式注册。
- `http-client` feature 可选且不改变默认 feature。底层使用 `reqwest` + Rustls，启用 JSON、form、multipart、stream；不启用系统 TLS。
- 默认连接超时 5 秒、总超时 30 秒、响应体上限 1 MiB、禁止自动重定向。应用可显式调整；配置非法时构造失败。
- 默认不重试。允许调用方以显式策略重试幂等请求，必须限制尝试数并使用有界退避；客户端不猜测请求幂等性。
- 提供 JSON 请求/响应入口、基本请求构造入口与有界响应读取。HTTP 非成功状态由响应对象返回，不自动变成传输错误。错误展示不暴露 URL、头或请求/响应正文。
- 取消遵循 Tokio future 取消语义；丢弃请求 future 即停止等待，不承诺撤销已到达远端的操作。
- 应用可通过公开 `HttpTransport` / `HttpResponseBody` contract 和 `HttpClient::with_transport` 注入测试 transport；默认 transport 才能访问网络。假实现必须在本地返回响应，不能回退真实网络。自定义 transport 也受总超时与响应体上限约束。
- `HttpClient::new` 与注入 transport 的客户端不提供 SSRF 目标防护；应用可选 `HttpClient::restricted` 的 URL/DNS/连接校验，具体信任边界与内网例外见 [ADR-051](adr-051-http-target-security.md)。自动重定向保持关闭以限制隐式跨域凭据转发。

## 影响

仅 `http-client` 启用时依赖 `reqwest`。不引入第二套 HTTP server，也不改变入站 `HttpPlugin`。统一出站客户端后续由供应商适配复用，真实供应商发送仍需独立授权与凭据。

## 验收

默认 feature 不拉入出站网络依赖；feature 下覆盖配置校验、JSON、form、multipart、请求/响应流、状态、体积限制、超时、显式有界重试、取消及 fake 不访问网络。真实本地 TCP 服务验证连接与错误行为，生成项目与 `根目录集成宿主` 分别验证 feature 接入。
