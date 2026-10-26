# ADR-051：出站 HTTP 受限目标策略

- 状态：Accepted
- 日期：2026-10-27

## 背景

统一 `HttpClient` 当前默认关闭重定向、忽略环境代理，但调用方仍无法选择一套可审计的 SSRF 目标约束。仅在请求前解析 URL 或 DNS 会产生检查与实际连接之间的竞态。

## 决策

- 通过独立的 `HttpClient::restricted(config, policy)` opt-in；现有 `HttpClient::new(config)` 与配置结构保持兼容。该能力仅约束经 `openrust::http::HttpClient` 发出的请求，不约束原始 SeaORM、存储驱动或任意第三方网络调用。
- 受限策略只接受 HTTP/HTTPS、无 URL 用户信息、允许的主机/端口；拒绝调用方覆写 `Host` 头，避免白名单 URL 转向其他虚拟主机路由。代理配置与受限策略互斥。自动重定向继续关闭。请求 URL 在送入 transport 前检查。
- 受限策略默认拒绝全部主机；通过 `allow_host` 精确登记公网目标。只接受 HTTP/HTTPS 默认端口（其他公网端口需显式开放），并拒绝非公网路由地址（包含 IPv4/IPv6 私有、环回、链路本地、保留及特殊用途范围）。应用可为精确主机登记一个 CIDR 和端口集合，允许可信内网端点；同一主机只接受一条内网登记，避免 resolver 丢失 URL 端口后错误组合网段和端口。
- 使用 reqwest 的 DNS resolver 在实际连接解析点收集全部 A/AAAA 结果；若任一结果不允许或结果为空，则失败关闭，否则将同一组已检查地址交给连接器，避免二次解析竞态。IP 字面量按同一 IP 策略校验。
- 受限策略不适用于 `HttpClient::with_transport` 注入的 transport；受限客户端固定使用内建 reqwest transport。自定义 transport 可能自行建立网络连接，框架无法观察或强制其 DNS/重定向行为。
- URL 预检失败返回 `HttpClientError::TargetRejected`；DNS 解析拒绝由 reqwest 映射为不含目标 URL、解析内容、凭据或请求正文的传输错误。策略是应用级防线，不替代网络出口控制。

## 影响

默认请求行为不变；用户配置 URL、远程读取等场景可显式启用策略。允许内网时必须同时明确主机、单个 CIDR 和端口集合；新增公开类型位于 `openrust::http`，以 `ipnet` 表达网段。

## 验收

受控 resolver 覆盖混合公网/私网解析、空解析及解析变化；本地 TCP 服务验证受限入口真实连接、内网主机/网段/端口严格组合授权、Host 头不能更换路由目标及重定向不跟随；配置验证覆盖 scheme、主机、端口、用户信息、代理和 fake/custom transport；独立 consumer 确认不能通过受限入口绕过策略。完整验证命令与事实写入 `memory/testing.md`。
