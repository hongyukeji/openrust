# ADR-028 可信代理后的客户端 IP

- 状态：Accepted（已实现；PostgreSQL 16 / MySQL 8.4 生成项目 HTTP 测试通过）
- 日期：2026-10-26
- 依据：[ADR-024](adr-024-authentication-flow-service.md)、[ADR-025](adr-025-shared-rate-limiter.md)

## 决策

- `[server].trusted_proxy_ranges` 接受 IPv4/IPv6 地址或 CIDR；默认空列表，始终以 TCP `ConnectInfo<SocketAddr>` 为连接事实。
- 公共 `ClientIp` extractor 仅在直接 peer 命中可信范围时读取 `X-Forwarded-For`；否则完全忽略该头并返回 peer IP。
- 可信代理链按 XFF 从右向左剥离可信 hop，返回第一个不可信地址；格式错误返回统一 bad request，不回退到用户可控值。
- 缺失 XFF 时返回直接 peer IP；配置在 router/serve 初始化时校验，非法范围阻止服务启动。
- 只解释 `X-Forwarded-For`，不解释 `Forwarded`、`X-Real-IP` 或未经配置的 header；代理必须覆盖或正确追加链信息。
- 生成认证路由用 `ClientIp` 作为 RateLimiter source；服务层仍接收调用方明确提供的 trusted source 字符串。

## 边界与验收

该机制只解析来源地址，不提供代理认证、TLS 终止、host/proto 重写或网络访问控制。单测覆盖默认拒绝、伪造头忽略、可信代理链、malformed header、IPv4/IPv6 CIDR 边界；生成项目真实 HTTP 测试覆盖可信链可改变来源、非可信 peer 无法伪造来源。PostgreSQL/MySQL 认证流程仍分别运行真实隔离库验收。
