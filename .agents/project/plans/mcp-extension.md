# MCP 可选扩展计划

> 状态：需求评估与排期；未实现、未验收，本轮不领取开发。
> 定位：阶段二 R11 的通用可选扩展，不是 Laravel 原生对应项，不启动 AI模型平台或可选快速开发模块。

## 范围决策

优先规划 MCP Server，让业务模块向 AI 客户端公开 tools、resources、prompts；框架本身不要求接入 LLM。通用Client不扩成独立平台；[AI SDK可选扩展](ai-capabilities.md)对标官方MCP工具调用时，按该项真实需要补最小Client能力和互操作验收，不能因Server优先而跳过SDK已纳入行为。其他Client用途仍待实际需求。用户已授权评估和制定计划，具体实现仍须按唯一 todo 队列领取，不抢占 C04-D1。

当前在 src/、modules/、tools/cli/ 和根 Cargo.toml 未发现原生 MCP 或 rmcp；Tokio/Axum、Container/Provider 和模块装配只是集成基础，不代表协议已支持。AI模型平台 OR-21/P7 是独立产品需求，不能替代本通用扩展的验收，也不构成本产品开工授权。

## Laravel 风格与实现边界

- 沿既有 ServiceProvider、容器、领域配置/.env、包/模块装配与受管生命周期接入；提供业务模块注册公开方法，由开发者决定挂载路径，不预置强制 /mcp 路由。
- 优先评估官方 Rust SDK rmcp，不自造协议解析、版本协商或传输栈。仅按需引入，不选择对应依赖/feature时不编译专属依赖、不注册服务、不连接或监听；Cargo.lock 解析与构建隔离分别说明。
- 业务工具复用已有应用服务，经唯一 modules/example 展示。禁止自动将所有 Controller、数据库操作或 shell 暴露为工具；不创建第二示例或复制业务逻辑。
- 远程优先 Streamable HTTP；stdio 只在确认本地客户端需求和进程生命周期后纳入。Provider 加载仅注册和校验，不暗启监听/子进程。
- schema 校验、身份上下文、授权及适用租户隔离、审计、超时/取消、限流与有界资源回收必须通过公开机制组合。远程 MCP 鉴权按选定协议验收，不能用现有 JWT/OIDC 名称代替兼容证据。
- MCP 响应、错误及流式消息保持标准协议，不套统一业务 JSON 包装；适配框架错误与可观测性，不建立第二套认证/事件/日志系统。

## 依赖、分簇与验收

| 顺序 | 交付 | 通过条件 |
| --- | --- | --- |
| MCP-0 Spec | 固定协议 revision、SDK commit/version/features/MSRV/许可证、客户端版本与能力清单；收敛公开 API、传输、鉴权和错误边界 | 源码/规范证据明确，SDK 与协议版本真实对应；可选依赖与现有 runtime/Cargo 图兼容，不照抄最新版本 |
| MCP-1 Provider/模块 | 注册与容器作用域、模块生命周期、schema/错误/取消、HTTP 公开挂载 | 未引入不编译、不监听；加载不监听；模块与外部 consumer 不改核心即可扩展；普通 HTTP 与 MCP 格式互不污染 |
| MCP-2 安全与互操作 | tools/resources/prompts 的实际协议流程与身份边界，按 Spec 验收 Streamable HTTP | 至少一个真实 MCP 客户端发现/调用/读取/模板流程；错误、未授权/越权、适用租户隔离及取消/关闭有证据；声明多客户端兼容须逐一实测 |
| MCP-3 开发体验与门禁 | modules/example、生成应用、README/config、公开 API 与适用验证 | 示例真实调用，生成项目 check/test；fmt/clippy/test/doc 与依赖隔离检查；工具有数据库副作用时另验 PG18/MySQL8.4，涉及多节点再验实际共享状态 |

前置为当前 HTTP/请求与响应、安全/授权、模块装配及扩展加载的对应 contract 稳定；在阶段二 R11 的上述前置具备后进入 todo。管理 UI、外部 MCP 服务商、LLM 账户与 AI 产品均不是通用 Server 的强制前置。无法取得真实客户端或授权环境时保留未验收，不以 mock/编译通过替代。

## 来源与待锁定项

2026-10-29 重新核对：[MCP Server 概念](https://modelcontextprotocol.io/docs/learn/server-concepts)、[官方 Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)、[协议版本规则](https://modelcontextprotocol.io/specification/versioning)。官方版本页本次标示 current 为 2026-07-28；这不证明任意 SDK/客户端已兼容该版本。正式 revision、SDK 和客户端基线由 MCP-0 共同锁定，当前不增加 Cargo 依赖或发布支持承诺。

采用官方 SDK解决协议维护与互操作问题；OpenRust 仅承担 Laravel 式集成、生命周期和扩展体验。未直接复制其他语言 API，Rust 的类型、所有权和 async 必要适配在 MCP-0 说明。
